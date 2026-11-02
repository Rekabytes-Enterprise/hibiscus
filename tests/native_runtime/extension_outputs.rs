use super::*;

fn declare(directory: &std::path::Path) {
    let path = directory.join("extension.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["tools"][0]["outputSchema"] = json!({"type":"object","properties":{"checked":{"type":"boolean"}},"required":["checked"],"additionalProperties":false});
    fs::write(path, manifest.to_string()).unwrap();
}

fn saved_tool(f: &Fixture) -> Value {
    let path = fs::read_dir(f.root.join("app/sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .map(|row| row["message"].clone())
        .find(|m| m["role"] == "toolResult")
        .expect("persisted tool result")
}

#[test]
fn extension_output_schemas_recover_for_every_provider_without_saving_invalid_output() {
    for api in ["responses", "chat_completions", "anthropic"] {
        for invalid in [false, true] {
            let f = Fixture::new(
                api,
                vec![
                    provider_tool(api, json!({"name":"test"})),
                    provider_done(api),
                ],
            );
            let script = if invalid {
                PROGRAM
                    .replace("EXTENSION_OUTPUT", "REJECTED_SUCCESS_TEXT")
                    .replace("true", r#""REJECTED_DATA_VALUE""#)
            } else {
                PROGRAM.into()
            };
            let directory = extension(&f, &script);
            declare(&directory);
            let output = f.run("", &["use the extension"]);
            good(&output);
            let shown = String::from_utf8_lossy(&output.stdout);
            assert!(shown.contains("EXTENSION_TURN_DONE"));
            let requests = f.captured();
            assert_eq!(requests.len(), 2);
            let result = saved_tool(&f);
            assert_eq!(result["toolCallId"], "tool-0");
            assert_eq!(result["toolName"], "demo__greet");
            assert_eq!(result["isError"], invalid);
            assert_eq!(result["details"]["exitCode"], 0);
            if invalid {
                assert!(result["details"].get("data").is_none());
                assert_eq!(result["details"]["outputSchemaError"], true);
                assert!(requests[1].to_string().contains("outputSchema"));
                for marker in ["REJECTED_DATA_VALUE", "REJECTED_SUCCESS_TEXT"] {
                    assert!(!result.to_string().contains(marker));
                    assert!(!requests[1].to_string().contains(marker));
                    assert!(!shown.contains(marker));
                }
                if api == "anthropic" {
                    assert!(requests[1]["messages"].as_array().unwrap().iter().any(|m| m
                        ["content"]
                        .as_array()
                        .is_some_and(|blocks| blocks
                            .iter()
                            .any(|b| b["type"] == "tool_result" && b["is_error"] == true))));
                }
            } else {
                assert_eq!(result["details"]["data"], json!({"checked":true}));
                assert!(requests[1].to_string().contains("EXTENSION_OUTPUT"));
            }
        }
    }
}

#[test]
fn agent_can_author_an_output_schema_and_use_it_on_the_next_step() {
    let mut args = authoring_args();
    args["manifest"]["tools"][0]["outputSchema"] = json!({"type":"object","required":["checked"]});
    let f = Fixture::new(
        "responses",
        vec![
            calls(vec![("create_extension", args)]),
            calls(vec![("made__greet", json!({"name":"test"}))]),
            provider_done("responses"),
        ],
    );
    good(&f.run("", &["create and use the extension"]));
    assert_eq!(f.captured().len(), 3);
    let manifest: Value = serde_json::from_slice(
        &fs::read(f.root.join("app/extensions/made/extension.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["tools"][0]["outputSchema"]["required"],
        json!(["checked"])
    );
    assert!(f.captured()[2].to_string().contains("EXTENSION_OUTPUT"));
}
