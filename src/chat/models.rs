use crate::{
    client::bridge::Client,
    ui::dialog::Dialogs,
    ui::{
        display::thinking_label,
        terminal::{self, RawMode},
        Frontend,
    },
    Result,
};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::mpsc::Receiver;

/// CLI lists stay numbered up to this size; larger catalogs print a provider summary.
const CLI_LIST_LIMIT: usize = 50;

/// `request` is the text after `/models`: an exact `provider/model` selects it,
/// a leading provider name opens that provider, and the rest filters models.
pub(crate) fn choose_model<R: BufRead>(
    rpc: &mut Client,
    input: &mut R,
    output: &mut Frontend,
    request: Option<&str>,
    interactive: bool,
    events: Option<&Receiver<u8>>,
    raw: &mut Option<RawMode>,
) -> Result<()> {
    let mut dialogs = Dialogs;
    let available = rpc.request(
        json!({"type":"get_available_models"}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    // Discovery can revalidate the selected thinking level. Read state after
    // that refresh, including when the user cancels the model picker.
    let state = rpc.request(
        json!({"type":"get_state"}),
        input,
        output,
        &mut dialogs,
        raw,
        events,
        interactive,
    )?;
    if let Some(error) = available["catalogError"].as_str() {
        writeln!(output, "{error}")?;
    }
    let Some(all) = available["models"].as_array() else {
        return Err("invalid get_available_models response".into());
    };
    let models = sorted_models(all);
    if models.is_empty() {
        writeln!(output, "No available models from runtime.")?;
        return Ok(());
    }
    let active = active_index(&models, &state);
    let name = |model: &Value| {
        format!(
            "{}/{}",
            model["provider"].as_str().unwrap(),
            model["id"].as_str().unwrap()
        )
    };
    let request = request.map(str::trim).filter(|r| !r.is_empty());
    let exact = request.and_then(|r| models.iter().position(|m| name(m) == r));
    let mut providers = models
        .iter()
        .map(|m| m["provider"].as_str().unwrap())
        .collect::<Vec<_>>();
    providers.dedup();
    let (tab, query) = match request.map(|r| r.split_once(' ').unwrap_or((r, ""))) {
        Some((first, rest)) => match providers.iter().position(|p| p.eq_ignore_ascii_case(first)) {
            Some(tab) => (Some(tab), rest.trim()),
            None => (None, request.unwrap()),
        },
        None => (None, ""),
    };
    let selected = if exact.is_some() {
        exact
    } else if output.is_full() {
        let Some(keys) = events else { return Ok(()) };
        let width = models
            .iter()
            .map(|m| m["id"].as_str().unwrap().chars().count())
            .max()
            .unwrap_or(0)
            .min(40);
        let labels = models
            .iter()
            .map(|m| {
                let id = m["id"].as_str().unwrap();
                match details(m) {
                    d if d.is_empty() => id.to_owned(),
                    d => format!("{id:<width$}  {d}"),
                }
            })
            .collect::<Vec<_>>();
        let start = tab.unwrap_or(if request.is_some() {
            providers.len()
        } else {
            active
                .and_then(|i| providers.iter().position(|p| *p == models[i]["provider"]))
                .unwrap_or(0)
        });
        let tabs = crate::tui::Tabs {
            names: providers.iter().map(|p| p.to_string()).collect(),
            groups: models
                .iter()
                .map(|m| providers.iter().position(|p| *p == m["provider"]).unwrap())
                .collect(),
            prefixes: models
                .iter()
                .map(|m| format!("{}/", m["provider"].as_str().unwrap()))
                .collect(),
            tab: start,
            query: query.to_owned(),
        };
        output.select_searchable("Models · all providers", &labels, active, Some(tabs), keys)?
    } else {
        if request.is_none() && models.len() > CLI_LIST_LIMIT {
            writeln!(output, "Model providers:")?;
            for provider in &providers {
                let count = models.iter().filter(|m| m["provider"] == *provider).count();
                writeln!(output, "  {provider}  {count} models")?;
            }
            writeln!(output, "List one with /models <provider> [search], or select with /models <provider>/<model>.")?;
            return Ok(());
        }
        let terms = query.to_lowercase();
        let shown = (0..models.len())
            .filter(|&i| tab.is_none_or(|t| models[i]["provider"] == providers[t]))
            .filter(|&i| {
                let label = name(models[i]).to_lowercase();
                terms.split_whitespace().all(|term| label.contains(term))
            })
            .collect::<Vec<_>>();
        if shown.is_empty() {
            writeln!(output, "No matching models.")?;
            return Ok(());
        }
        writeln!(output, "Models (sorted by provider):")?;
        for (number, &index) in shown.iter().enumerate() {
            let marker = if Some(index) == active {
                "  (current)"
            } else {
                ""
            };
            writeln!(output, "  {}. {}{marker}", number + 1, name(models[index]))?;
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
            .and_then(|number| number.checked_sub(1))
            .and_then(|number| shown.get(number).copied())
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
    output: &mut Frontend,
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
            .map(|level| thinking_label(level).to_owned())
            .collect::<Vec<_>>();
        output.select("Thinking · Runtime model", &labels, active, keys)?
    } else {
        writeln!(output, "Thinking levels (current marked):")?;
        for (index, level) in levels.iter().enumerate() {
            writeln!(
                output,
                "  {}. {}{}",
                index + 1,
                thinking_label(level),
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
        if !output.is_full() {
            writeln!(
                output,
                "Already using {} thinking.",
                thinking_label(levels[index])
            )?;
        }
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
    if !output.is_full() {
        writeln!(output, "Thinking level: {}.", thinking_label(level))?;
    }
    Ok(())
}

/// Short capability notes copied from the runtime catalog, e.g. "1M ctx · img".
fn details(model: &Value) -> String {
    let mut parts = vec![];
    if let Some(n) = model["contextWindow"].as_u64() {
        parts.push(if n >= 1_000_000 {
            let tenths = (n + 50_000) / 100_000;
            if tenths % 10 == 0 {
                format!("{}M ctx", tenths / 10)
            } else {
                format!("{}.{}M ctx", tenths / 10, tenths % 10)
            }
        } else {
            format!("{}K ctx", (n + 500) / 1000)
        });
    }
    if model["images"] == true {
        parts.push("img".into());
    }
    if model["reasoning"] == true {
        parts.push("reasoning".into());
    }
    parts.join(" · ")
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
    fn details_summarize_reported_capabilities_only() {
        assert_eq!(
            details(&json!({"contextWindow":1048576,"images":true,"reasoning":true})),
            "1M ctx · img · reasoning"
        );
        assert_eq!(details(&json!({"contextWindow":1_050_000})), "1.1M ctx");
        assert_eq!(details(&json!({"contextWindow":262144})), "262K ctx");
        assert_eq!(details(&json!({"contextWindow":null})), "");
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
