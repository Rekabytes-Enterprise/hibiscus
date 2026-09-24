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
    let provider = state["model"]["provider"].as_str().unwrap_or("");
    if provider.is_empty() {
        writeln!(output, "No current provider; use Pi /login first.")?;
        return Ok(());
    }
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
    let models: Vec<&Value> = all
        .iter()
        .filter(|model| model["provider"] == provider)
        .collect();
    if models.is_empty() {
        writeln!(output, "No available models for {provider}.")?;
        return Ok(());
    }
    let current = state["model"]["id"].as_str();
    let selected = if output.is_full() {
        let Some(keys) = events else { return Ok(()) };
        let labels = models
            .iter()
            .map(|model| model["id"].as_str().unwrap_or("unknown").to_owned())
            .collect::<Vec<_>>();
        let active = models
            .iter()
            .position(|model| model["id"].as_str() == current);
        output.select(&format!("Models · {provider}"), &labels, active, keys)?
    } else {
        writeln!(output, "Models for {provider}:")?;
        for (index, model) in models.iter().enumerate() {
            let id = model["id"].as_str().unwrap_or("unknown");
            let marker = if Some(id) == current {
                "  (current)"
            } else {
                ""
            };
            writeln!(output, "  {}. {id}{marker}", index + 1)?;
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
    let model_id = models[index]["id"].as_str().ok_or("invalid model id")?;
    if Some(model_id) == current {
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
