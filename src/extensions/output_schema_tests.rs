use super::*;

fn output_schema(directory: &Path, schema: Value) {
    let path = directory.join("extension.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["tools"][0]["outputSchema"] = schema;
    fs::write(path, manifest.to_string()).unwrap();
}

#[tokio::test]
async fn output_schemas_validate_data_and_keep_commands_and_logical_errors_compatible() {
    let home = Home::new();
    let directory = home.extension("test", REPLY);
    output_schema(
        &directory,
        json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"],"additionalProperties":false}),
    );
    let registry = Registry::load(&home.0).unwrap();
    assert!(
        registry.has_tool("test__greet"),
        "{:?}",
        registry.warnings()
    );
    let (_cancel, mut stop) = watch::channel(false);
    let result = registry
        .execute_tool(
            "valid",
            "test__greet",
            &json!({"name":"ok"}),
            (&home.0, "session"),
            &mut stop,
        )
        .await
        .unwrap();
    assert_eq!(result["isError"], false);
    assert_eq!(result["details"]["data"], json!({"count":1}));
    for data in [
        json!({"count":"INVALID_DATA_MARKER"}),
        json!({}),
        json!(null),
    ] {
        // Keep the request ID dynamic, just as a real extension must.
        let script = REPLY.replace(r#""data":{"count":1}"#, &format!(r#""data":{data}"#));
        fs::write(directory.join("main.sh"), script).unwrap();
        let registry = Registry::load(&home.0).unwrap();
        let result = registry
            .execute_tool(
                "invalid",
                "test__greet",
                &json!({"name":"ok"}),
                (&home.0, "session"),
                &mut stop,
            )
            .await
            .unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(result["details"]["exitCode"], 0);
        assert_eq!(result["details"]["extension"], "test");
        assert!(result["details"].get("data").is_none());
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("outputSchema"));
        assert!(!result.to_string().contains("INVALID_DATA_MARKER"));
        assert!(!result.to_string().contains("hello"));
        let command = registry
            .execute_command(
                "command",
                "/test:greet",
                "",
                (&home.0, "session"),
                &mut stop,
            )
            .await
            .unwrap();
        assert_eq!(command["text"], "hello");
        assert_eq!(command["isError"], false);
    }
    fs::write(
        directory.join("main.sh"),
        REPLY.replace(r#""data":{"count":1}"#, r#""isError":true"#),
    )
    .unwrap();
    let registry = Registry::load(&home.0).unwrap();
    let result = registry
        .execute_tool(
            "logical",
            "test__greet",
            &json!({"name":"ok"}),
            (&home.0, "session"),
            &mut stop,
        )
        .await
        .unwrap();
    assert_eq!(result["isError"], true);
    assert_eq!(result["content"][0]["text"], "hello");
    assert_eq!(result["details"]["exitCode"], 0);
}

#[test]
fn output_schemas_compile_offline_and_are_advertised_for_authoring() {
    let home = Home::new();
    let directory = home.extension("test", REPLY);
    let path = directory.join("extension.json");
    let mut value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let definition = agent_definitions()
        .into_iter()
        .find(|t| t["name"] == "create_extension")
        .unwrap();
    let authoring = jsonschema::validator_for(&definition["parameters"]).unwrap();
    for schema in [
        json!(true),
        json!(false),
        json!({}),
        json!({"type":"string"}),
        json!({"definitions":{"count":{"type":"integer"}},"$ref":"#/definitions/count"}),
    ] {
        value["tools"][0]["outputSchema"] = schema.clone();
        assert!(
            manifest::parse(value.to_string().as_bytes()).is_ok(),
            "{schema}"
        );
        assert!(authoring.is_valid(&json!({"manifest":value})));
    }
    for schema in [
        json!(null),
        json!(3),
        json!([]),
        json!({"type":"unknown"}),
        json!({"$ref":"file:///must-not-read.json"}),
        json!({"$ref":"https://example.invalid/must-not-fetch.json"}),
        json!({"type":"string","pattern":"(?=nonlinear)"}),
    ] {
        value["tools"][0]["outputSchema"] = schema.clone();
        let error = manifest::parse(value.to_string().as_bytes())
            .err()
            .expect("invalid schema accepted");
        assert!(!error.to_string().contains("must-not-"));
    }
}

#[tokio::test]
async fn boolean_output_schemas_and_omitted_data_have_json_schema_semantics() {
    for (schema, rejected) in [
        (json!(true), false),
        (json!(false), true),
        (json!({"type":"null"}), false),
        (json!({"type":"object"}), true),
    ] {
        let home = Home::new();
        let directory = home.extension("test", &REPLY.replace(r#","data":{"count":1}"#, ""));
        output_schema(&directory, schema);
        let registry = Registry::load(&home.0).unwrap();
        assert!(
            registry.has_tool("test__greet"),
            "{:?}",
            registry.warnings()
        );
        let (_cancel, mut stop) = watch::channel(false);
        let result = registry
            .execute_tool(
                "call",
                "test__greet",
                &json!({"name":"ok"}),
                (&home.0, "session"),
                &mut stop,
            )
            .await
            .unwrap();
        assert_eq!(result["isError"], rejected);
    }
}
