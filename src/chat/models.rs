use crate::{
    pi::{dialog::Dialogs, rpc::Rpc},
    tui::{
        screen::Screen,
        terminal::{self, RawMode},
    },
    Result,
};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::mpsc::Receiver;

pub(crate) fn choose_model<R: BufRead>(
    rpc: &mut Rpc,
    input: &mut R,
    output: &mut Screen,
    interactive: bool,
    events: Option<&Receiver<u8>>,
    raw: &mut Option<RawMode>,
) -> Result<()> {
    let mut dialogs = Dialogs;
    let state = rpc.request(
        json!({"type":"get_state"}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    let available = rpc.request(
        json!({"type":"get_available_models"}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    let Some(all) = available["models"].as_array() else {
        return Err("invalid get_available_models response".into());
    };
    let models = sorted_models(all);
    if models.is_empty() {
        writeln!(output, "No available models from Pi.")?;
        return Ok(());
    }
    let active = active_index(&models, &state);
    let selected = if output.is_full() {
        let Some(keys) = events else { return Ok(()) };
        let labels = models
            .iter()
            .map(|model| {
                format!(
                    "{}/{}",
                    model["provider"].as_str().unwrap(),
                    model["id"].as_str().unwrap()
                )
            })
            .collect::<Vec<_>>();
        output.select("Models · all providers", &labels, active, keys)?
    } else {
        writeln!(output, "Models (sorted by provider):")?;
        for (index, model) in models.iter().enumerate() {
            let provider = model["provider"].as_str().unwrap();
            let id = model["id"].as_str().unwrap();
            let marker = if Some(index) == active {
                "  (current)"
            } else {
                ""
            };
            writeln!(output, "  {}. {provider}/{id}{marker}", index + 1)?;
        }
        let answer = if let (Some(keys), Some(raw)) = (events, raw.as_mut()) {
            raw.write("Choose a number (Enter to cancel): ")?;
            terminal::read_line(keys, raw)?
        } else if interactive {
            write!(output, "Choose a number (Enter to cancel): ")?;
            output.flush()?;
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                None
            } else {
                Some(line)
            }
        } else {
            writeln!(output, "Model selection requires an interactive terminal.")?;
            return Ok(());
        };
        answer
            .and_then(|answer| answer.trim().parse::<usize>().ok())
            .and_then(|index| index.checked_sub(1))
            .filter(|index| *index < models.len())
    };
    let Some(index) = selected else { return Ok(()) };
    let provider = models[index]["provider"].as_str().unwrap();
    let model_id = models[index]["id"].as_str().unwrap();
    if Some(index) == active {
        writeln!(output, "Already using {provider}/{model_id}.")?;
        return Ok(());
    }
    rpc.request(
        json!({"type":"set_model","provider":provider,"modelId":model_id}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    writeln!(output, "Selected {provider}/{model_id}.")?;
    Ok(())
}

fn active_index(models: &[&Value], state: &Value) -> Option<usize> {
    let provider = state["model"]["provider"].as_str()?;
    let id = state["model"]["id"].as_str()?;
    models.iter().position(|model| {
        model["provider"].as_str() == Some(provider) && model["id"].as_str() == Some(id)
    })
}

/// Keep Pi's model catalog authoritative; sorting only changes presentation.
fn sorted_models(all: &[Value]) -> Vec<&Value> {
    let mut models = all
        .iter()
        .filter(|model| {
            model["provider"].as_str().is_some_and(|s| !s.is_empty())
                && model["id"].as_str().is_some_and(|s| !s.is_empty())
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| {
        let key = |model: &Value| {
            (
                model["provider"].as_str().unwrap().to_ascii_lowercase(),
                model["id"].as_str().unwrap().to_ascii_lowercase(),
            )
        };
        key(a).cmp(&key(b))
    });
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_are_grouped_by_provider_then_id_without_restricting_current_provider() {
        let models = vec![
            json!({"provider":"openrouter","id":"Z"}),
            json!({"provider":"anthropic","id":"sonnet"}),
            json!({"provider":"openai-codex","id":"gpt"}),
            json!({"provider":"openrouter","id":"a"}),
            json!({"provider":"anthropic","id":"Haiku"}),
            json!({"provider":"","id":"invalid"}),
        ];
        let ordered = sorted_models(&models);
        assert_eq!(
            ordered
                .iter()
                .map(|m| format!(
                    "{}/{}",
                    m["provider"].as_str().unwrap(),
                    m["id"].as_str().unwrap()
                ))
                .collect::<Vec<_>>(),
            [
                "anthropic/Haiku",
                "anthropic/sonnet",
                "openai-codex/gpt",
                "openrouter/a",
                "openrouter/Z"
            ]
        );
    }

    #[test]
    fn active_model_matches_provider_and_id_and_can_be_absent() {
        let catalog = [
            json!({"provider":"anthropic","id":"shared"}),
            json!({"provider":"openrouter","id":"shared"}),
        ];
        let models = sorted_models(&catalog);
        assert_eq!(
            active_index(
                &models,
                &json!({"model":{"provider":"openrouter","id":"shared"}})
            ),
            Some(1)
        );
        assert_eq!(active_index(&models, &json!({"model":null})), None);
    }
}
