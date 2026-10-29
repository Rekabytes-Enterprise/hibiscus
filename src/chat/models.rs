use crate::{
    runtime::{bridge::Client, dialog::Dialogs},
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
    rpc: &mut Client,
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
        writeln!(output, "No available models from runtime.")?;
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
        output.select_searchable("Models · all providers", &labels, active, keys)?
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
        if !output.is_full() {
            writeln!(output, "Already using {provider}/{model_id}.")?;
        }
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
    if !output.is_full() {
        writeln!(output, "Selected {provider}/{model_id}.")?;
    }
    Ok(())
}

/// Query the active model's supported levels from runtime; never guess reasoning
/// support or maintain a second per-model configuration in Hibiscus.
#[allow(clippy::too_many_arguments)]
pub(crate) fn choose_thinking<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Screen,
    requested: Option<&str>,
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
    if !state["model"].is_object() {
        writeln!(
            output,
            "Choose a model with /models before setting thinking."
        )?;
        return Ok(());
    }
    let available = rpc.request(
        json!({"type":"get_available_thinking_levels"}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    let Some(levels) = available["levels"].as_array() else {
        return Err("invalid get_available_thinking_levels response".into());
    };
    let levels: Vec<&str> = levels
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 32
                        && s.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                })
                .ok_or("invalid thinking level from runtime")
        })
        .collect::<std::result::Result<_, _>>()?;
    if levels.is_empty() {
        return Err("Runtime reported no thinking levels".into());
    }
    let active = state["thinkingLevel"]
        .as_str()
        .and_then(|level| levels.iter().position(|candidate| *candidate == level));
    let selected = if let Some(level) = requested {
        let Some(index) = levels
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(level))
        else {
            writeln!(
                output,
                "Unknown thinking level. Available: {}",
                levels.join(", ")
            )?;
            return Ok(());
        };
        Some(index)
    } else if output.is_full() {
        let Some(keys) = events else { return Ok(()) };
        let labels = levels
            .iter()
            .map(|level| (*level).to_owned())
            .collect::<Vec<_>>();
        output.select("Thinking · Runtime model", &labels, active, keys)?
    } else {
        writeln!(output, "Thinking levels (current marked):")?;
        for (index, level) in levels.iter().enumerate() {
            writeln!(
                output,
                "  {}. {}{}",
                index + 1,
                level,
                if Some(index) == active {
                    "  (current)"
                } else {
                    ""
                }
            )?;
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
            None
        };
        answer
            .and_then(|answer| answer.trim().parse::<usize>().ok())
            .and_then(|index| index.checked_sub(1))
            .filter(|index| *index < levels.len())
    };
    let Some(index) = selected else { return Ok(()) };
    if Some(index) == active {
        writeln!(output, "Already using {} thinking.", levels[index])?;
        return Ok(());
    }
    rpc.request(
        json!({"type":"set_thinking_level","level":levels[index]}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    let confirmed = rpc.request(
        json!({"type":"get_state"}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    let level = confirmed["thinkingLevel"]
        .as_str()
        .ok_or("Runtime did not confirm thinking level")?;
    if !levels.contains(&level) {
        return Err("Runtime returned an unsupported thinking level".into());
    }
    writeln!(output, "Thinking level: {level} (Pi).")?;
    Ok(())
}

fn active_index(models: &[&Value], state: &Value) -> Option<usize> {
    let provider = state["model"]["provider"].as_str()?;
    let id = state["model"]["id"].as_str()?;
    models.iter().position(|model| {
        model["provider"].as_str() == Some(provider) && model["id"].as_str() == Some(id)
    })
}

/// Keep Hibiscus's model catalog authoritative; sorting only changes presentation.
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
