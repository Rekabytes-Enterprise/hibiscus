//! Search discovery and bounded, credential-free reads of public sources.
use crate::Result;
use reqwest::{header, Client, Response, StatusCode, Url};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::sync::watch;

const KEY: &str = "brave-search";
const ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
const RESULTS: usize = 6;
const MAX_BODY: usize = 1024 * 1024;
const MAX_TEXT: usize = 12_000;
const TIMEOUT: Duration = Duration::from_secs(20);

fn env_key() -> Option<String> {
    std::env::var("BRAVE_SEARCH_API_KEY")
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
}

fn key() -> Result<Option<String>> {
    Ok(super::auth::connector_key(KEY)?.or_else(env_key))
}

pub fn status() -> Result<Option<&'static str>> {
    Ok(if super::auth::connector_key(KEY)?.is_some() {
        Some("key")
    } else {
        env_key().map(|_| "environment")
    })
}

pub async fn connect(key: &str) -> Result<()> {
    let key = key.trim();
    let token = token(key)?;
    search_at(
        &client().build()?,
        ENDPOINT,
        token,
        &Search {
            query: "Hibiscus".into(),
            freshness: None,
        },
        1,
    )
    .await?;
    super::auth::save_connector_key(KEY, Some(key.to_owned()))
}

pub fn disconnect() -> Result<()> {
    super::auth::save_connector_key(KEY, None)
}

fn token(key: &str) -> Result<header::HeaderValue> {
    if key.is_empty() || key.len() > 8192 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("Paste a valid Brave Search API key.".into());
    }
    let mut token =
        header::HeaderValue::from_str(key).map_err(|_| "Paste a valid Brave Search API key.")?;
    token.set_sensitive(true);
    Ok(token)
}

pub(super) fn definitions() -> [Value; 2] {
    [
        json!({"name":"web_search","description":"Search the public web for current information using the configured Brave Search connector. Returns ranked results with URLs, publishers, dates when available, and unverified snippets. Open sources with web_fetch before relying on details.","parameters":{"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":600},"freshness":{"type":"string","enum":["day","week","month","year"]}},"required":["query"],"additionalProperties":false}}),
        json!({"name":"web_fetch","description":"Read text from a public HTTP or HTTPS source. Use the returned URL when citing it. Reports blocked pages, unsupported formats and pages without readable static text. It cannot sign in, run JavaScript or bypass access restrictions.","parameters":{"type":"object","properties":{"url":{"type":"string","minLength":1,"maxLength":2048}},"required":["url"],"additionalProperties":false}}),
    ]
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Freshness {
    Day,
    Week,
    Month,
    Year,
}
impl Freshness {
    fn parameter(self) -> &'static str {
        match self {
            Self::Day => "pd",
            Self::Week => "pw",
            Self::Month => "pm",
            Self::Year => "py",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    freshness: Option<Freshness>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fetch {
    url: String,
}

pub(super) async fn run(
    name: &str,
    args: &Value,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Value> {
    let work = async {
        if name == "web_search" {
            let search: Search = serde_json::from_value(args.clone()).map_err(|_| {
                "Web search requires a query and an optional day, week, month or year filter."
            })?;
            let query = search.query.trim();
            if query.is_empty()
                || query.chars().count() > 600
                || query.split_whitespace().count() > 75
            {
                return Err(
                    "Search query must contain 1 to 600 characters and at most 75 words.".into(),
                );
            }
            let key = key()?.ok_or(
                "Web search is unavailable. Connect Brave Search in Settings → Connectors or set BRAVE_SEARCH_API_KEY. You can still use web_fetch with a public URL.",
            )?;
            let results =
                search_at(&client().build()?, ENDPOINT, token(&key)?, &search, RESULTS).await?;
            Ok(json!({
                "provider":"Brave Search",
                "evidence":"search snippets",
                "results":results,
            }))
        } else {
            let args: Fetch =
                serde_json::from_value(args.clone()).map_err(|_| "Web fetch requires a URL.")?;
            fetch(public_url(&args.url)?).await
        }
    };
    tokio::select! {
        biased;
        _ = cancel.wait_for(|stopped| *stopped) => Err("Web request cancelled".into()),
        result = tokio::time::timeout(TIMEOUT, work) => result.map_err(|_| "Web request timed out")?,
    }
}

fn client() -> reqwest::ClientBuilder {
    Client::builder()
        .no_proxy()
        .referer(false)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .user_agent(concat!("Hibiscus/", env!("CARGO_PKG_VERSION")))
}

#[derive(Deserialize)]
struct SearchResponse {
    web: Option<WebResults>,
}
#[derive(Deserialize)]
struct WebResults {
    results: Vec<SearchHit>,
}
#[derive(Deserialize)]
struct SearchHit {
    title: String,
    url: String,
    description: Option<String>,
    profile: Option<Profile>,
    page_age: Option<String>,
}
#[derive(Deserialize)]
struct Profile {
    long_name: Option<String>,
    name: Option<String>,
}
#[derive(Serialize)]
struct Hit {
    title: String,
    url: String,
    publisher: String,
    page_date: Option<String>,
    snippet: String,
}

fn plain(text: &str, limit: usize) -> String {
    bounded(
        Html::parse_fragment(text)
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .as_str(),
        limit,
    )
}

fn bounded(text: &str, limit: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

async fn search_at(
    client: &Client,
    endpoint: &str,
    token: header::HeaderValue,
    search: &Search,
    count: usize,
) -> Result<Vec<Hit>> {
    let count_param = count.to_string();
    let mut parameters = vec![
        ("q", search.query.trim()),
        ("count", &count_param),
        ("result_filter", "web"),
    ];
    if let Some(freshness) = search.freshness {
        parameters.push(("freshness", freshness.parameter()));
    }
    let request = || {
        client
            .get(endpoint)
            .header("X-Subscription-Token", token.clone())
            .header(header::ACCEPT, "application/json")
            .query(&parameters)
            .send()
    };
    let mut response = request().await;
    if match &response {
        Ok(response) => transient_status(response.status()),
        Err(error) => error.is_timeout() || error.is_connect(),
    } {
        tokio::time::sleep(Duration::from_millis(300)).await;
        response = request().await;
    }
    let response = response.map_err(|_| "Could not reach Brave Search.")?;
    match response.status() {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            return Err(
                "Brave Search rejected the API key or its access. Check the connector settings."
                    .into(),
            );
        }
        StatusCode::TOO_MANY_REQUESTS => return Err(limited(&response, "Brave Search").into()),
        status if !status.is_success() => {
            return Err(format!("Brave Search returned HTTP {status}.").into());
        }
        _ => {}
    }

    let (body, truncated) = read_body(response).await?;
    if truncated {
        return Err("Brave Search returned an oversized response.".into());
    }
    let response: SearchResponse = serde_json::from_slice(&body)
        .map_err(|_| "Brave Search returned an unreadable response.")?;
    let mut seen = HashSet::new();
    Ok(response
        .web
        .into_iter()
        .flat_map(|web| web.results)
        .filter_map(|hit| {
            let url = public_url(&hit.url).ok()?;
            let title = plain(&hit.title, 240);
            if title.is_empty() || !seen.insert(url.clone()) {
                return None;
            }
            let publisher = hit
                .profile
                .and_then(|p| p.long_name.or(p.name))
                .map(|name| plain(&name, 120))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| url.host_str().unwrap_or_default().to_owned());
            Some(Hit {
                title,
                url: url.to_string(),
                publisher,
                page_date: hit.page_age.map(|date| bounded(&date, 40)),
                snippet: plain(hit.description.as_deref().unwrap_or_default(), 400),
            })
        })
        .take(count)
        .collect())
}

fn transient_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
    )
}

fn limited(response: &Response, service: &str) -> String {
    let wait = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    match wait {
        Some(seconds) => format!("{service} is rate limited. Try again in {seconds} seconds."),
        None => format!("{service} is rate limited. Try again later."),
    }
}

mod fetch;
use fetch::{fetch, public_url};

async fn read_body(mut response: Response) -> Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Web response was interrupted.")?
    {
        let available = MAX_BODY - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(available)]);
        if chunk.len() > available {
            return Ok((bytes, true));
        }
    }
    Ok((bytes, false))
}

#[cfg(test)]
mod tests;
