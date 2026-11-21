use super::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinHandle,
};

struct Reply<'a> {
    status: u16,
    media: &'a str,
    extra: &'a str,
    body: &'a str,
}

async fn fixture(replies: Vec<Reply<'static>>) -> (Url, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint =
        Url::parse(&format!("http://{}/source", listener.local_addr().unwrap())).unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut request = String::new();
            loop {
                let offset = request.len();
                assert_ne!(reader.read_line(&mut request).await.unwrap(), 0);
                if &request[offset..] == "\r\n" {
                    break;
                }
            }
            requests.push(request);
            let Reply {
                status,
                media,
                extra,
                body,
            } = reply;
            socket.write_all(format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
                body.len()
            ).as_bytes()).await.unwrap();
        }
        requests
    });
    (endpoint, task)
}

fn search_reply(status: u16, extra: &'static str, body: &'static str) -> Reply<'static> {
    Reply {
        status,
        extra,
        body,
        media: "application/json",
    }
}

async fn query(endpoint: &Url) -> Result<Vec<Hit>> {
    search_at(
        &client().build().unwrap(),
        endpoint.as_str(),
        token("synthetic-local-fixture").unwrap(),
        &Search {
            query: "NVIDIA update".into(),
            freshness: Some(Freshness::Week),
        },
        RESULTS,
    )
    .await
}

#[tokio::test]
async fn retries_transient_status_once_and_returns_ordered_dated_snippets() {
    let (url, requests) = fixture(vec![
        search_reply(503, "", "{}"),
        search_reply(200, "", r#"{"web":{"results":[
          {"url":"https://news.example.org/a","title":"<b>News</b> &amp; results","description":"Snippet","profile":{"long_name":"Example News"},"page_age":"2026-11-21T06:00:00"},
          {"url":"https://news.example.org/a#duplicate","title":"Duplicate"},
          {"url":"http://127.0.0.1/hidden","title":"Ignored"},
          {"url":"https://example.org/b","title":"Second"}
        ]}}"#),
    ]).await;
    let hits = serde_json::to_value(query(&url).await.unwrap()).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 2);
    assert_eq!(hits[0]["publisher"], "Example News");
    assert_eq!(hits[0]["title"], "News & results");
    assert_eq!(hits[0]["page_date"], "2026-11-21T06:00:00");
    assert_eq!(hits[0]["snippet"], "Snippet");
    assert_eq!(hits[1]["publisher"], "example.org");
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.contains("q=NVIDIA+update&count=6&result_filter=web&freshness=pw")));
    assert!(requests.iter().all(|request| request
        .to_lowercase()
        .contains("x-subscription-token: synthetic-local-fixture")));
}

#[tokio::test]
async fn handles_no_results_rate_limits_rejected_keys_and_retry_exhaustion() {
    for (replies, expected) in [
        (
            vec![search_reply(200, "", r#"{"web":{"results":[]}}"#)],
            None,
        ),
        (
            vec![search_reply(
                429,
                "Retry-After: 17\r\n",
                "synthetic-private-error",
            )],
            Some("17 seconds"),
        ),
        (
            vec![search_reply(401, "", "synthetic-private-error")],
            Some("rejected the API key"),
        ),
        (
            vec![search_reply(503, "", "{}"), search_reply(503, "", "{}")],
            Some("HTTP 503"),
        ),
    ] {
        let count = replies.len();
        let (url, requests) = fixture(replies).await;
        let result = query(&url).await;
        if let Some(expected) = expected {
            let message = result.err().unwrap().to_string();
            assert!(message.contains(expected), "{message}");
            assert!(!message.contains("synthetic"));
        } else {
            assert!(result.unwrap().is_empty());
        }
        assert_eq!(requests.await.unwrap().len(), count);
    }
}

#[tokio::test]
async fn source_read_distinguishes_page_text_from_blocked_or_unreadable_sources() {
    let cases = [
        (200, "text/html", "", "<title>Example News</title><nav>Skip</nav><article>Verified &amp; public.<script>private script</script><p hidden>hidden</p></article>", None),
        (200, "text/html", "", "<title>Verify you are human</title>", Some("CAPTCHA")),
        (403, "text/html", "cf-mitigated: challenge\r\n", "", Some("browser challenge")),
        (200, "text/html", "", "<title>Log in to continue</title>", Some("sign-in")),
        (402, "text/html", "", "", Some("paid access")),
        (200, "text/html", "", "<meta name='isAccessibleForFree' content='false'><main>Preview</main>", Some("paid access")),
        (403, "text/html", "", "", Some("blocked")),
        (429, "text/html", "Retry-After: 30\r\n", "", Some("30 seconds")),
        (200, "application/pdf", "", "", Some("unsupported")),
        (200, "text/html", "", "<div id='app'></div><script>render()</script><noscript>Enable JavaScript</noscript>", Some("no readable static text")),
    ];
    for (status, media, extra, body, expected) in cases {
        let (url, request) = fixture(vec![Reply {
            status,
            media,
            extra,
            body,
        }])
        .await;
        let response = client()
            .build()
            .unwrap()
            .get(url.clone())
            .send()
            .await
            .unwrap();
        let result = fetch::read_source(response, &url).await;
        if let Some(expected) = expected {
            let message = result.unwrap_err().to_string();
            assert!(message.contains(expected), "{message}");
        } else {
            let article = result.unwrap();
            assert_eq!(article["url"], url.as_str());
            assert_eq!(article["title"], "Example News");
            assert_eq!(article["text"], "Verified & public.");
            assert_eq!(article["evidence"], "page text");
        }
        let headers = request.await.unwrap().pop().unwrap().to_lowercase();
        assert!(!headers.contains("authorization:"));
        assert!(!headers.contains("cookie:"));
        assert!(!headers.contains("x-subscription-token:"));
    }
}

#[test]
fn public_url_rejects_local_addresses_and_credentials() {
    for url in [
        "file:///tmp/source",
        "https://alice:secret@example.org",
        "https://@example.org",
        "http://localhost/",
        "http://foo.local/",
        "http://127.1/",
        "http://0x7f000001/",
        "http://169.254.169.254/",
        "http://100.100.100.200/",
        "http://[::ffff:127.0.0.1]/",
        "http://[fd00::1]/",
        "https://example.org:8080/",
        "https://example.org\u{1b}/",
    ] {
        assert!(public_url(url).is_err(), "{url}");
    }
    let url = public_url("https://example.org/news?day=21#heading").unwrap();
    assert_eq!(url.as_str(), "https://example.org/news?day=21");
}

#[tokio::test]
async fn cancelled_web_call_settles_without_running() {
    let (_send, mut cancel) = watch::channel(true);
    let result = run("web_fetch", &json!({"url":"invalid"}), &mut cancel).await;
    assert_eq!(result.unwrap_err().to_string(), "Web request cancelled");
}
