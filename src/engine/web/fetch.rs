use super::*;

pub(super) fn public_url(raw: &str) -> Result<Url> {
    if raw.len() > 2048 || raw.chars().any(char::is_control) {
        return Err("Web fetch requires an HTTP or HTTPS URL of at most 2048 bytes.".into());
    }
    let mut url = Url::parse(raw).map_err(|_| "Supply a full HTTP or HTTPS URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(
            raw.split_once("://"),
            Some((_, rest)) if !rest.split(['/', '?', '#', '\\']).next().unwrap_or_default().contains('@')
        )
    {
        return Err(
            "Web fetch requires HTTP or HTTPS on its usual port, without credentials.".into(),
        );
    }
    let host = url.host_str().ok_or("Web URL needs a public host.")?;
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        if !public_ip(ip) {
            return Err("Web fetch can only reach public addresses.".into());
        }
    } else {
        let host = host.trim_end_matches('.');
        if !host.contains('.')
            || [".localhost", ".local", ".internal", ".home.arpa", ".onion"]
                .iter()
                .any(|suffix| host.ends_with(suffix))
        {
            return Err("Web fetch can only reach public addresses.".into());
        }
    }
    url.set_fragment(None);
    Ok(url)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && ((b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99) || b == 168))
                || (a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let [a, b, ..] = ip.segments();
            // Global unicast only, excluding the special-purpose, documentation
            // and IPv4-tunneling blocks within it.
            (0x2000..0x4000).contains(&a)
                && !(a == 0x2001 && (b < 0x0200 || b == 0x0db8))
                && a != 0x2002
                && a != 0x3fff
        }
    }
}

pub(super) async fn fetch(mut url: Url) -> Result<Value> {
    for _ in 0..=3 {
        let host = url.host_str().ok_or("Web URL needs a public host.")?;
        let port = url
            .port_or_known_default()
            .ok_or("Unsupported web URL scheme.")?;
        let mut builder = client();
        // IP literals were checked during parsing. Pin DNS names to their
        // checked addresses so the connection cannot resolve them a second time.
        if host.trim_matches(['[', ']']).parse::<IpAddr>().is_err() {
            let addresses: Vec<SocketAddr> = tokio::time::timeout(
                Duration::from_secs(4),
                tokio::net::lookup_host((host, port)),
            )
            .await
            .map_err(|_| "The web source's address lookup timed out.")?
            .map_err(|_| "Could not resolve the web source.")?
            .collect();
            if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
                return Err(
                    "The web source did not resolve exclusively to public addresses.".into(),
                );
            }
            builder = builder.resolve_to_addrs(host, &addresses);
        }
        let response = builder
            .build()?
            .get(url.clone())
            .header(
                header::ACCEPT,
                "text/html, application/xhtml+xml, text/plain",
            )
            .send()
            .await
            .map_err(|_| "Could not open the web source.")?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("Web source returned a redirect without a URL.")?;
            let redirect = url
                .join(location)
                .map_err(|_| "Web source returned an invalid redirect.")?;
            url = public_url(redirect.as_str())?;
            continue;
        }
        return read_source(response, &url).await;
    }
    Err("The web source redirected too many times.".into())
}

pub(super) async fn read_source(response: Response, url: &Url) -> Result<Value> {
    if response
        .headers()
        .get("cf-mitigated")
        .is_some_and(|value| value == "challenge")
    {
        return Err("The web source returned a browser challenge.".into());
    }
    match response.status() {
        StatusCode::UNAUTHORIZED => return Err("The web source requires sign-in.".into()),
        StatusCode::PAYMENT_REQUIRED => return Err("The web source requires paid access.".into()),
        StatusCode::FORBIDDEN | StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS => {
            return Err("Access to the web source was blocked.".into())
        }
        StatusCode::TOO_MANY_REQUESTS => return Err(limited(&response, "The web source").into()),
        status if !status.is_success() => {
            return Err(format!("The web source returned HTTP {status}.").into())
        }
        _ => {}
    }
    let media = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        media.as_str(),
        "text/html" | "application/xhtml+xml" | "text/plain"
    ) {
        return Err(
            "The web source format is unsupported. Web fetch reads HTML and plain text.".into(),
        );
    }
    let (body, clipped) = read_body(response).await?;
    let body = String::from_utf8_lossy(&body);
    let (title, text) = if media == "text/plain" {
        (None, bounded(&body, MAX_TEXT + 1))
    } else {
        extract_html(&body)?
    };
    if text.is_empty() {
        return Err(
            "The web source has no readable static text. It may require JavaScript or sign-in."
                .into(),
        );
    }
    let length = text.floor_char_boundary(MAX_TEXT);
    Ok(json!({
        "url": url.as_str(),
        "title":title,
        "evidence":"page text",
        "truncated":clipped || length < text.len(),
        "text": &text[..length],
    }))
}

fn select(query: &str) -> Selector {
    Selector::parse(query).expect("valid static selector")
}

fn extract_html(body: &str) -> Result<(Option<String>, String)> {
    let doc = Html::parse_document(body);
    let title = doc
        .select(&select("title"))
        .next()
        .map(|element| bounded(&element.text().collect::<Vec<_>>().join(" "), 240));
    let heading = title.as_deref().unwrap_or_default().to_ascii_lowercase();
    if ["just a moment", "attention required", "verify you are human", "captcha"]
        .iter().any(|needle| heading.starts_with(needle))
        || doc.select(&select("#challenge-form, #cf-challenge-running, iframe[src*='recaptcha'], iframe[src*='challenges.cloudflare.com']")).next().is_some()
    {
        return Err("The web source returned a CAPTCHA or browser challenge.".into());
    }
    if heading.starts_with("sign in") || heading.starts_with("log in") {
        return Err("The web source requires sign-in.".into());
    }
    if doc.select(&select("meta[name='isAccessibleForFree'][content='false'], meta[property='article:content_tier'][content='locked']")).next().is_some() {
        return Err("The web source requires paid access.".into());
    }
    for scope in ["article", "main, [role='main']", "body"] {
        let mut output = String::new();
        for element in doc.select(&select(scope)).take(32) {
            let text = visible_text(element);
            if !text.is_empty() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&text);
            }
            if output.len() > MAX_TEXT {
                break;
            }
        }
        if !output.is_empty() {
            return Ok((title, output));
        }
    }
    Ok((title, String::new()))
}

fn hidden(element: ElementRef<'_>) -> bool {
    let tag = element.value();
    matches!(
        tag.name(),
        "script" | "style" | "noscript" | "template" | "nav" | "footer" | "aside" | "form" | "svg"
    ) || tag.attr("hidden").is_some()
        || tag.attr("aria-hidden") == Some("true")
}

fn visible_text(element: ElementRef<'_>) -> String {
    if hidden(element) || element.ancestors().filter_map(ElementRef::wrap).any(hidden) {
        return String::new();
    }
    let mut output = String::new();
    let mut stack = vec![element.children()];
    while let Some(children) = stack.last_mut() {
        let Some(node) = children.next() else {
            stack.pop();
            continue;
        };
        if let Some(text) = node.value().as_text() {
            let text = bounded(text, MAX_TEXT + 1);
            if !text.is_empty() {
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&text);
                if output.len() > MAX_TEXT {
                    break;
                }
            }
        } else if !ElementRef::wrap(node).is_some_and(hidden) {
            stack.push(node.children());
        }
    }
    output
}
