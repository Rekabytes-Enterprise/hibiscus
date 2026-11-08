use super::http::authenticated_request;
use super::*;
use crate::runtime::session::Block;
#[test]
fn codex_done_requires_completed_status_before_accepting_tools() {
    for status in ["incomplete", "failed", "in_progress", "completed"] {
        let mut stream = Stream::default();
        let event = json!({"type":"response.done","response":{"status":status,"output":[
            {"type":"function_call","call_id":"call","name":"write","arguments":"{\"path\":\"file\",\"content\":\"text\"}"}
        ]}});
        let result = stream.observe(&Api::Responses, &event);
        if status == "completed" {
            result.unwrap();
            assert!(stream.done);
            let reply = stream.reply(&Api::Responses).unwrap();
            assert!(
                matches!(reply.message, Message::Assistant { content, .. } if matches!(content.first(),Some(Block::ToolCall{..})))
            );
        } else {
            assert!(result.is_err());
            assert!(!stream.done);
        }
    }
}
#[test]
fn codex_credentials_are_pinned_to_subscription_transport() {
    let client = reqwest::Client::new();
    let token = super::super::oauth::Token {
        access: "FAKE-ACCESS".into(),
        refresh: "FAKE-REFRESH".into(),
        expires: 1,
        account_id: "FAKE-ACCOUNT".into(),
    };
    let body = json!({"model":"test","store":false,"stream":true,"max_output_tokens":4096,
        "input":[{"role":"user","content":"hello"}]});
    for url in [
        "https://evil.example/responses",
        "https://api.openai.com/v1/responses",
        "https://chatgpt.com/backend-api/codex/responses?redirect=evil",
    ] {
        assert!(
            authenticated_request(&client, url, body.clone(), "OTHER-KEY", Some(&token)).is_err()
        );
    }
    let request = authenticated_request(
        &client,
        &format!("{}/responses", super::super::oauth::RESOURCE),
        body,
        "OTHER-KEY",
        Some(&token),
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(request.headers()["authorization"], "Bearer FAKE-ACCESS");
    assert_eq!(request.headers()["chatgpt-account-id"], "FAKE-ACCOUNT");
    assert_eq!(request.headers()["originator"], "hibiscus");
    assert_eq!(request.headers()["accept"], "text/event-stream");
    let body: Value = serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    assert!(body.get("max_output_tokens").is_none());
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert!(!body.to_string().contains("FAKE-REFRESH"));
}
#[test]
fn anthropic_context_includes_cached_input_and_final_usage() {
    let mut stream = Stream::default();
    stream.observe(&Api::Anthropic,&json!({"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30}}})).unwrap();
    stream.observe(&Api::Anthropic,&json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":15,"output_tokens":5}})).unwrap();
    stream
        .observe(&Api::Anthropic, &json!({"type":"message_stop"}))
        .unwrap();
    let reply = stream.reply(&Api::Anthropic).unwrap();
    assert_eq!(reply.input, Some(15));
    assert_eq!(reply.cache, Some(20));
    assert_eq!(reply.context, Some(70));
}

fn chat_round(events: &[Value]) -> Stream {
    let mut stream = Stream::default();
    for event in events {
        stream.observe(&Api::ChatCompletions, event).unwrap();
    }
    stream
}
fn text(content: &str, finish: Option<&str>) -> Value {
    json!({"choices":[{"delta":{"content":content},"finish_reason":finish}]})
}
fn call(id: &str, args: &str, finish: Option<&str>) -> Value {
    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":id,"function":{"name":"write","arguments":args}}]},"finish_reason":finish}]})
}
fn reply_text(reply: &Reply) -> String {
    reply.message.text()
}

#[test]
fn continuation_usage_sums_rounds_without_turning_missing_values_into_zero() {
    for missing in [false, true] {
        let mut continuation = Continuation::default();
        let mut first = Stream {
            finish: "length".into(),
            input: Some(u64::MAX),
            output: Some(2),
            cache: (!missing).then_some(1),
            ..Stream::default()
        };
        assert!(continuation
            .next(&Api::ChatCompletions, &mut first)
            .is_some());
        let mut last = Stream {
            finish: "stop".into(),
            input: Some(15),
            output: Some(3),
            cache: Some(2),
            ..Stream::default()
        };
        assert!(continuation
            .next(&Api::ChatCompletions, &mut last)
            .is_none());
        assert_eq!(
            continuation.usage,
            Some([Some(u64::MAX), Some(5), (!missing).then_some(3)])
        );
        assert_eq!(last.reply(&Api::ChatCompletions).unwrap().context, Some(18));
    }
}

#[test]
fn length_is_continued_and_text_is_joined() {
    let mut continuation = Continuation::default();
    let mut first = chat_round(&[text("Hello, ", Some("length"))]);
    let follow_up = continuation
        .next(&Api::ChatCompletions, &mut first)
        .unwrap();
    assert_eq!(
        follow_up,
        [
            json!({"role":"assistant","content":"Hello, "}),
            json!({"role":"user","content":CONTINUE_PROMPT})
        ]
    );
    let mut second = chat_round(&[text("world.", Some("stop"))]);
    assert!(continuation
        .next(&Api::ChatCompletions, &mut second)
        .is_none());
    let reply = second.reply(&Api::ChatCompletions).unwrap();
    assert_eq!(reply_text(&reply), "Hello, world.");
}

#[test]
fn partial_tool_call_is_discarded_and_the_reissued_call_is_returned() {
    let mut continuation = Continuation::default();
    let mut first = chat_round(&[
        text("Writing.", None),
        call("cut", "{\"path\":\"fi", Some("length")),
    ]);
    let follow_up = continuation
        .next(&Api::ChatCompletions, &mut first)
        .unwrap();
    assert_eq!(follow_up[1]["content"], REISSUE_PROMPT);
    assert!(!follow_up.iter().any(|m| m.get("tool_calls").is_some()));
    let mut second = chat_round(&[call("full", "{\"path\":\"file\"}", Some("tool_calls"))]);
    assert!(continuation
        .next(&Api::ChatCompletions, &mut second)
        .is_none());
    let reply = second.reply(&Api::ChatCompletions).unwrap();
    let Message::Assistant { content, .. } = &reply.message else {
        panic!("assistant reply expected")
    };
    let ids: Vec<_> = content
        .iter()
        .filter_map(|b| match b {
            Block::ToolCall { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["full"]);
    assert_eq!(reply_text(&reply), "Writing.");

    // A re-issued call that is cut again is an error, never executed.
    let mut continuation = Continuation::default();
    let mut first = chat_round(&[call("a", "{", Some("length"))]);
    continuation
        .next(&Api::ChatCompletions, &mut first)
        .unwrap();
    let mut second = chat_round(&[call("b", "{", Some("length"))]);
    assert!(continuation
        .next(&Api::ChatCompletions, &mut second)
        .is_none());
    let error = second
        .reply(&Api::ChatCompletions)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("tool call exceeded the provider output limit"),
        "{error}"
    );
}

#[test]
fn continuations_stop_after_three_and_keep_the_text() {
    let mut continuation = Continuation::default();
    for round in 0..MAX_CONTINUATIONS {
        let mut stream = chat_round(&[text(&format!("{round}"), Some("length"))]);
        assert!(continuation
            .next(&Api::ChatCompletions, &mut stream)
            .is_some());
    }
    let mut last = chat_round(&[text("3", Some("length"))]);
    assert!(continuation
        .next(&Api::ChatCompletions, &mut last)
        .is_none());
    let reply = last.reply(&Api::ChatCompletions).unwrap();
    assert_eq!(reply_text(&reply), "0123");
}

#[test]
fn content_filter_and_missing_finish_reason_are_errors() {
    let error = chat_round(&[text("x", Some("content_filter"))])
        .reply(&Api::ChatCompletions)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("content policy"), "{error}");
    assert!(!error.contains("truncated"), "{error}");
    let mut missing = chat_round(&[text("x", None)]);
    assert!(Continuation::default()
        .next(&Api::ChatCompletions, &mut missing)
        .is_none());
    let error = missing
        .reply(&Api::ChatCompletions)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("truncated or incomplete"), "{error}");
}
