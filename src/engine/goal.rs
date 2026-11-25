//! Explicit checklists and the verification gate for implementation phases.
use super::session::Message;
use crate::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Goal {
    Set {
        steps: Vec<String>,
        scope: Option<String>,
    },
    Complete {
        id: usize,
        evidence: Option<Evidence>,
    },
    Clear,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    unit: String,
    entry_point: String,
    failure_path: String,
}

pub(super) fn definition(verification_pending: bool) -> Value {
    let mut definition = json!({
        "name":"goal",
        "description":"Track an explicit checklist. Set scope for phased implementation. Complete steps by one-based id.",
        "parameters":{"type":"object","properties":{
            "action":{"type":"string","enum":["set","complete","clear"]},
            "steps":{"type":"array","items":{"type":"string"}},
            "scope":{"type":"string","maxLength":500},
            "id":{"type":"integer"}
        },"required":["action"],"additionalProperties":false}
    });
    if verification_pending {
        definition["parameters"]["properties"]["evidence"] = json!({
            "type":"object",
            "description":"Three successful foreground bash call IDs. entryPoint runs the real feature. failurePath checks an expected failure.",
            "properties":{"unit":{"type":"string"},"entryPoint":{"type":"string"},"failurePath":{"type":"string"}},
            "required":["unit","entryPoint","failurePath"],
            "additionalProperties":false
        });
    }
    definition
}

pub(super) fn validate(args: &Value) -> Result<()> {
    let _: Goal = serde_json::from_value(args.clone())?;
    Ok(())
}

pub(super) fn pending(steps: &[Value]) -> Option<(&str, &str)> {
    let scope = steps.first()?["scope"].as_str()?;
    let step = steps.iter().find(|step| step["done"] != true)?;
    Some((scope, step["label"].as_str()?))
}

fn verify(evidence: Evidence, history: &[Message]) -> Result<Value> {
    let refs = [
        ("unit", evidence.unit),
        ("entryPoint", evidence.entry_point),
        ("failurePath", evidence.failure_path),
    ];
    let mut distinct = HashSet::new();
    if refs
        .iter()
        .any(|(_, id)| id.is_empty() || id.len() > 256 || !distinct.insert(id.as_str()))
    {
        return Err(
            "Provide three distinct call IDs for unit, entryPoint, and failurePath checks.".into(),
        );
    }
    let mut valid = HashSet::new();
    for message in history.iter().rev() {
        match message {
            Message::User { .. } => break,
            Message::ToolResult {
                tool_name, details, ..
            } if matches!(
                tool_name.as_str(),
                "write" | "edit" | "batch_edit" | "checkpoint_restore"
            ) || (tool_name == "goal" && details["phaseBoundary"] == true) =>
            {
                break
            }
            Message::ToolResult {
                tool_call_id,
                tool_name,
                details,
                is_error: false,
                ..
            } if tool_name == "bash"
                && details["exitCode"].as_i64() == Some(0)
                && details["reportedCrash"] != true =>
            {
                valid.insert(tool_call_id.as_str());
            }
            _ => {}
        }
    }
    let missing = refs
        .iter()
        .filter_map(|(kind, id)| (!valid.contains(id.as_str())).then_some(*kind))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("Phase still unverified. Run successful foreground checks for {} after the latest edit, then cite their call IDs.", missing.join(", ")).into());
    }
    Ok(json!({"unit":refs[0].1, "entryPoint":refs[1].1, "failurePath":refs[2].1}))
}

pub(super) fn execute(args: &Value, steps: &mut Vec<Value>, history: &[Message]) -> Result<Value> {
    let action: Goal = serde_json::from_value(args.clone())?;
    let mut phase_boundary = false;
    match action {
        Goal::Set {
            steps: labels,
            scope,
        } => {
            let mut seen = HashSet::new();
            if labels.is_empty()
                || labels.len() > 50
                || labels
                    .iter()
                    .any(|s| s.trim().is_empty() || s.len() > 200 || !seen.insert(s))
            {
                return Err("Set needs 1–50 distinct named steps, each at most 200 bytes".into());
            }
            if scope
                .as_ref()
                .is_some_and(|scope| scope.trim().is_empty() || scope.len() > 500)
            {
                return Err(
                    "An implementation scope must be nonempty and at most 500 bytes.".into(),
                );
            }
            *steps = labels
                .into_iter()
                .map(|label| json!({"label":label,"done":false}))
                .collect();
            if let Some(scope) = scope {
                steps[0]["scope"] = json!(scope);
                phase_boundary = true;
            }
        }
        Goal::Complete { id, evidence } => {
            let index = id
                .checked_sub(1)
                .filter(|i| *i < steps.len())
                .ok_or("Invalid step number")?;
            if steps[index]["done"] == true {
                return Err("Step already complete".into());
            }
            if pending(steps).is_some() {
                if steps.iter().position(|step| step["done"] != true) != Some(index) {
                    return Err("Complete implementation phases in order.".into());
                }
                let evidence = evidence.ok_or("Phase still unverified. Supply successful call IDs for unit, entryPoint, and failurePath.")?;
                steps[index]["evidence"] = verify(evidence, history)?;
                phase_boundary = true;
            }
            steps[index]["done"] = json!(true);
        }
        Goal::Clear => steps.clear(),
    }
    let completed = steps.iter().filter(|step| step["done"] == true).count();
    let status =
        pending(steps).map_or_else(String::new, |(_, phase)| format!("\nOpen phase: {phase}"));
    Ok(super::tools::result(
        format!("Goal: {completed}/{} steps completed{status}", steps.len()),
        json!({"hibiscusGoal":{"steps":steps,"completed":completed,"total":steps.len()},"phaseBoundary":phase_boundary}),
    ))
}
