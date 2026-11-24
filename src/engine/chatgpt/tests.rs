use super::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fixture_identity() -> String {
    // A synthetic ID token signed offline. The repository holds only public
    // verification material and these pre-signed fixture segments.
    let segments: [String; 3] = serde_json::from_str(include_str!(
        "../../../tests/fixtures/chatgpt-test-signature.json"
    ))
    .unwrap();
    segments.join(".")
}

async fn server(
    responses: Vec<(&'static str, String)>,
    checks: impl Fn(usize, &str) + Send + Sync + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    (base, serve(listener, responses, checks))
}

fn serve(
    listener: tokio::net::TcpListener,
    responses: Vec<(&'static str, String)>,
    checks: impl Fn(usize, &str) + Send + Sync + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        for (index, (route, body)) in responses.into_iter().enumerate() {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                request.push(socket.read_u8().await.unwrap());
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let header = String::from_utf8(request.clone()).unwrap();
            assert_eq!(header.split_whitespace().nth(1), Some(route));
            let len = header
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap_or_default();
            let mut input = vec![0; len];
            socket.read_exact(&mut input).await.unwrap();
            checks(index, std::str::from_utf8(&input).unwrap());
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    })
}

#[tokio::test]
async fn browser_registration_exchanges_issued_client_and_verifies_signed_identity() {
    let mut flow = BrowserSignIn::prepare("fixture-host", None).unwrap();
    let args: std::collections::HashMap<_, _> = flow.url.query_pairs().into_owned().collect();
    assert_eq!(args["client_id"], ENTRYPOINT);
    assert_eq!(args["agent_name_hint"], "Hibiscus");
    assert_eq!(args["ext_agent_host_id"], "fixture-host");
    assert_eq!(args["scope"], SCOPES);
    assert_eq!(args["resource"], RESOURCE);
    assert!(args["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert!(valid(&args["nonce"], 128));
    // Match the nonce in the offline-signed identity fixture.
    flow.nonce = "fixture-nonce".into();
    let client = "oaiapp_fixture";
    let receipt = json!({"token_type":"Bearer", "access_token":"fixture-access", "refresh_token":"fixture-refresh",
        "id_token":fixture_identity(), "expires_in":600, "scope":SCOPES});
    let keys = include_str!("../../../tests/fixtures/chatgpt-test-jwks.json");
    // The HTTP server needs its own URL in the OIDC discovery response.
    let receipt = receipt.to_string();
    let verifier = flow.pending.verifier.clone();
    let redirect = args["redirect_uri"].clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let discovery = json!({"issuer": ISSUER, "jwks_uri":format!("{base}/keys")}).to_string();
    let task = serve(
        listener,
        vec![
            ("/token", receipt),
            ("/discovery", discovery),
            ("/keys", keys.into()),
        ],
        move |index, form| {
            if index == 0 {
                let url = reqwest::Url::parse(&format!("https://fixture.invalid/?{form}")).unwrap();
                let form: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                assert_eq!(form["client_id"], client);
                assert_eq!(form["code"], "fixture-code");
                assert_eq!(form["grant_type"], "authorization_code");
                assert_eq!(form["code_verifier"], verifier);
                assert_eq!(form["redirect_uri"], redirect);
                assert_eq!(form["resource"], RESOURCE);
                assert!(!form.contains_key("client_secret"));
            }
        },
    );
    let (paste, manual) = tokio::sync::mpsc::channel(1);
    let address = reqwest::Url::parse(&args["redirect_uri"]).unwrap();
    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", address.port().unwrap()))
        .await
        .unwrap();
    let callback = format!("GET /auth/callback?code=fixture-code&state={}&client_id={client} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", args["state"], address.port().unwrap());
    socket.write_all(callback.as_bytes()).await.unwrap();
    let registration = flow
        .complete_at(
            manual,
            &format!("{base}/token"),
            &format!("{base}/discovery"),
        )
        .await
        .unwrap();
    let registration = registration.registration;
    drop(paste);
    task.await.unwrap();
    assert_eq!(registration.client_id, client);
    assert_eq!(registration.subject, "fixture-account");
    assert_eq!(
        registration.grant.as_ref().unwrap().access,
        "fixture-access"
    );

    let returning = BrowserSignIn::prepare("fixture-host", Some(registration)).unwrap();
    let args: std::collections::HashMap<_, _> = returning.url.query_pairs().into_owned().collect();
    assert_eq!(args["client_id"], client);
    assert!(!args.contains_key("agent_name_hint"));
    assert_eq!(args["prompt"], "consent");
    assert!(registered_client(Some(client), Some("other")).is_err());
    assert!(registered_client(None, Some(ENTRYPOINT)).is_err());
}

#[tokio::test]
async fn refresh_rotates_credentials_and_declined_scope_disables_inference() {
    let dir = std::env::temp_dir().join(format!("hibiscus-plan-test-{}", random().unwrap()));
    let store = Store::at(dir.join("auth.json"));
    let registration = Registration {
        client_id: "oaiapp_fixture".into(),
        subject: "fixture-account".into(),
        grant: Some(Grant {
            access: "initial".into(),
            scope: SCOPES.into(),
            refresh: "refresh-initial".into(),
            expires: 1,
            refresh_after: 0,
        }),
    };
    let lock = store.try_lock().unwrap();
    store.write_json(&registration).unwrap();
    drop(lock);
    let rotated = json!({"token_type":"Bearer","access_token":"rotated","refresh_token":"refresh-rotated","expires_in":30});
    let declined = json!({"token_type":"Bearer","access_token":"no-plan","expires_in":600,"scope":"openid profile email"});
    let (base, task) = server(
        vec![
            ("/token", rotated.to_string()),
            ("/token", declined.to_string()),
        ],
        |index, form| {
            assert!(form.contains("grant_type=refresh_token"));
            assert!(form.contains("client_id=oaiapp_fixture"));
            assert!(!form.contains("scope="));
            let expected = if index == 0 {
                "refresh-initial"
            } else {
                "refresh-rotated"
            };
            assert!(form.contains(&format!("refresh_token={expected}")));
        },
    )
    .await;
    let endpoint = format!("{base}/token");
    assert_eq!(
        credential_at(&store, &endpoint, "").await.unwrap(),
        "rotated"
    );
    assert_eq!(
        credential_at(&store, &endpoint, "")
            .await
            .unwrap_err()
            .to_string(),
        CONSENT
    );
    assert!(store
        .read_json::<Registration>()
        .unwrap()
        .unwrap()
        .grant
        .is_none());
    task.await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn host_identifier_is_stable_and_separate_from_the_plan_grant() {
    let dir = std::env::temp_dir().join(format!("hibiscus-plan-host-test-{}", random().unwrap()));
    let store = Store::at(dir.join("host.json"));
    let first = host_id(&store).unwrap();
    assert_eq!(host_id(&store).unwrap(), first);
    assert!(valid(&first, 128));
    std::fs::remove_dir_all(dir).unwrap();
}
