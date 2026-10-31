use super::event::Event;
use super::{
    config::{self, Config},
    provider::{self, emit},
    session::{Block, Message, Session},
    tools,
};
use crate::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
};

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    id: String,
    #[serde(flatten)]
    command: Command,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Command {
    Prompt {
        message: String,
        #[serde(default)]
        images: Vec<Image>,
        #[serde(default, rename = "streamingBehavior")]
        streaming_behavior: Option<String>,
    },
    GetState,
    #[serde(rename = "extension_command")]
    RunExtension {
        name: String,
        input: String,
    },
    GetMessages,
    GetToolOutputs,
    GetToolOutput {
        index: Option<std::num::NonZeroUsize>,
    },
    GetAvailableModels,
    GetAvailableThinkingLevels,
    GetSessionStats,
    SetModel {
        provider: String,
        #[serde(rename = "modelId")]
        model_id: String,
    },
    SetThinkingLevel {
        level: String,
    },
    NewSession,
    SwitchSession {
        #[serde(rename = "sessionPath")]
        session_path: std::path::PathBuf,
    },
    ClearQueue,
    Abort,
    Compact {
        #[serde(default, rename = "customInstructions")]
        custom_instructions: Option<String>,
    },
}
#[derive(Clone, serde::Serialize, Deserialize)]
struct Image {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}
struct Prompt {
    text: String,
    images: Vec<Image>,
    steer: bool,
}
#[derive(Default)]
struct Queue {
    running: bool,
    items: VecDeque<Prompt>,
}
fn response(id: &str, data: Value) -> Value {
    json!({"type":"response","id":id,"success":true,"data":data})
}
fn failure(id: &str, message: &str) -> Value {
    json!({"type":"response","id":id,"success":false,"error":super::error::safe_message(message)})
}
fn queued(q: &Queue) -> Value {
    json!({"type":"queue_update","steering":q.items.iter().filter(|p|p.steer).map(|p|&p.text).collect::<Vec<_>>(),"followUp":q.items.iter().filter(|p|!p.steer).map(|p|&p.text).collect::<Vec<_>>()})
}
pub fn serve(socket: UnixStream, config: Config, session: Session) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(serve_async(socket, config, session))
}
async fn serve_async(socket: UnixStream, config: Config, session: Session) -> Result<()> {
    socket.set_nonblocking(true)?;
    let (reader, mut writer) = tokio::net::UnixStream::from_std(socket)?.into_split();
    let (commands, mut input) = mpsc::channel(64);
    let reading = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(reader);
        loop {
            let mut line = Vec::new();
            let r = (&mut reader)
                .take(64 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .await;
            match r {
                Ok(0) | Err(_) => break,
                Ok(_) if line.len() > 64 * 1024 * 1024 => break,
                Ok(_) => {
                    if commands.send(line).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    let (out, mut output) = mpsc::channel::<Value>(256);
    let writing = tokio::spawn(async move {
        while let Some(v) = output.recv().await {
            let mut bytes = serde_json::to_vec(&v)?;
            bytes.push(b'\n');
            writer.write_all(&bytes).await?;
        }
        Ok::<_, std::io::Error>(())
    });
    let session = Arc::new(Mutex::new(session));
    let queue = Arc::new(Mutex::new(Queue::default()));
    let config = Arc::new(config);
    let extensions = Arc::new(crate::extensions::Catalog::new(
        crate::extensions::Registry::discover()
            .unwrap_or_else(|e| crate::extensions::Registry::unavailable(&e.to_string())),
    ));
    {
        let initial = extensions.snapshot();
        for warning in initial.warnings() {
            emit(&out, json!({"type":"extension_error","error":warning})).await?;
        }
    }
    let (cancel, _) = watch::channel(false);
    let mut active: Option<tokio::task::JoinHandle<()>> = None;
    while let Some(bytes) = input.recv().await {
        let request: Envelope = match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(_) => {
                let v: Value = serde_json::from_slice(&bytes).unwrap_or_default();
                emit(
                    &out,
                    failure(
                        v["id"].as_str().unwrap_or(""),
                        "Invalid Hibiscus runtime command",
                    ),
                )
                .await?;
                continue;
            }
        };
        let id = request.id;
        let busy = queue.lock().unwrap().running;
        match request.command {
            Command::Abort => {
                cancel.send_replace(true);
                emit(&out, response(&id, json!({}))).await?;
            }
            Command::ClearQueue => {
                let update = {
                    let mut q = queue.lock().unwrap();
                    q.items.clear();
                    queued(&q)
                };
                emit(&out, response(&id, json!({}))).await?;
                emit(&out, update).await?;
            }
            Command::Prompt {
                message,
                images,
                streaming_behavior,
            } => {
                let valid = message.len() <= 65536
                    && images.len() <= 4
                    && images.iter().all(|i| {
                        matches!(
                            i.mime_type.as_str(),
                            "image/png" | "image/jpeg" | "image/webp" | "image/gif"
                        ) && i.data.len() <= 14 * 1024 * 1024
                    });
                if !valid {
                    emit(
                        &out,
                        failure(&id, "Prompt or image exceeds supported bounds"),
                    )
                    .await?;
                    continue;
                }
                let validation = session
                    .lock()
                    .unwrap()
                    .selected(&config)
                    .and_then(|(p, m)| {
                        config::key(p)?;
                        if !images.is_empty() && !m.images {
                            return Err("Selected model does not support images".into());
                        }
                        Ok(())
                    });
                if let Err(e) = validation {
                    emit(&out, failure(&id, &e.to_string())).await?;
                    continue;
                }
                if busy {
                    if !matches!(streaming_behavior.as_deref(), Some("steer" | "followUp"))
                        || queue.lock().unwrap().items.len() >= 16
                    {
                        emit(&out, failure(&id, "Agent is busy or its queue is full")).await?;
                        continue;
                    }
                    let update = {
                        let mut q = queue.lock().unwrap();
                        let prompt = Prompt {
                            text: message,
                            images,
                            steer: streaming_behavior.as_deref() == Some("steer"),
                        };
                        if prompt.steer {
                            let pos = q
                                .items
                                .iter()
                                .position(|p| !p.steer)
                                .unwrap_or(q.items.len());
                            q.items.insert(pos, prompt);
                        } else {
                            q.items.push_back(prompt);
                        }
                        queued(&q)
                    };
                    emit(&out, response(&id, json!({}))).await?;
                    emit(&out, update).await?;
                    continue;
                }
                if let Some(handle) = active.take() {
                    let _ = handle.await;
                }
                cancel.send_replace(false);
                queue.lock().unwrap().running = true;
                emit(&out, response(&id, json!({}))).await?;
                let (s, c, o, q, mut stop) = (
                    session.clone(),
                    config.clone(),
                    out.clone(),
                    queue.clone(),
                    cancel.subscribe(),
                );
                let registry = extensions.clone();
                active = Some(tokio::spawn(async move {
                    let result = run_turns(
                        Prompt {
                            text: message,
                            images,
                            steer: false,
                        },
                        s,
                        c,
                        registry,
                        &o,
                        &q,
                        &mut stop,
                    )
                    .await;
                    if let Err(e) = result {
                        let lost = {
                            let mut queue = q.lock().unwrap();
                            queue.running = false;
                            let lost = queue.items.pop_back();
                            queue.items.clear();
                            lost
                        };
                        let _ = emit(&o, Event::Failed(&e.to_string()).wire()).await;
                        if let Some(lost) = lost {
                            let _=emit(&o,json!({"type":"queue_rejected","message":lost.text,"images":lost.images,"steer":lost.steer,"error":"Run failed before queued delivery; review this message before retrying."})).await;
                        }
                    }
                    {
                        let mut queue = q.lock().unwrap();
                        queue.running = false;
                        queue.items.clear();
                    }
                    let _ = emit(
                        &o,
                        json!({"type":"queue_update","steering":[],"followUp":[]}),
                    )
                    .await;
                    let _ = emit(&o, Event::Settled.wire()).await;
                }));
            }
            Command::RunExtension { name, input } if !busy => {
                if let Some(handle) = active.take() {
                    let _ = handle.await;
                }
                cancel.send_replace(false);
                queue.lock().unwrap().running = true;
                let registry = extensions.snapshot();
                let snapshot = session.lock().unwrap().clone();
                let (o, q, mut stop) = (out.clone(), queue.clone(), cancel.subscribe());
                active = Some(tokio::spawn(async move {
                    let result = registry
                        .execute_command(
                            &id,
                            &name,
                            &input,
                            (&snapshot.cwd, &snapshot.id),
                            &mut stop,
                        )
                        .await;
                    q.lock().unwrap().running = false;
                    let result = match result {
                        Ok(data) => {
                            let _ = emit(
                                &o,
                                json!({"type":"extension_command_output","text":data["text"]}),
                            )
                            .await;
                            if data["isError"] == true {
                                failure(&id, "Extension command reported failure")
                            } else {
                                response(&id, json!({}))
                            }
                        }
                        Err(e) => failure(&id, &e.to_string()),
                    };
                    let _ = emit(&o, result).await;
                }));
            }
            Command::Compact {
                custom_instructions,
            } if !busy => {
                if let Some(handle) = active.take() {
                    let _ = handle.await;
                }
                cancel.send_replace(false);
                queue.lock().unwrap().running = true;
                let (s, c, o, q, mut stop) = (
                    session.clone(),
                    config.clone(),
                    out.clone(),
                    queue.clone(),
                    cancel.subscribe(),
                );
                active = Some(tokio::spawn(async move {
                    let result = compact(s, c, &o, custom_instructions.as_deref(), &mut stop).await;
                    let reply = match result {
                        Ok(v) => response(&id, v),
                        Err(e) => failure(&id, &e.to_string()),
                    };
                    let _ = emit(&o, reply).await;
                    q.lock().unwrap().running = false;
                }));
            }
            command => {
                let result = (|| -> Result<Value> {
                    let mut s = session.lock().unwrap();
                    match command {
                        Command::GetState => {
                            let mut state = s.state(&config);
                            state["extensionCommands"] =
                                serde_json::to_value(extensions.snapshot().commands())?;
                            Ok(state)
                        }
                        Command::GetMessages => Ok(json!({"messages":s.messages})),
                        Command::GetToolOutputs => {
                            Ok(json!({"outputs":super::tool_output::list(&s.messages)}))
                        }
                        Command::GetToolOutput { index } => {
                            Ok(json!({"output":super::tool_output::get(&s.messages, index)}))
                        }
                        Command::GetSessionStats => Ok(s.stats(&config)),
                        Command::GetAvailableModels => Ok(
                            json!({"models":config.providers.iter().flat_map(|p|p.models.iter().map(move|m|json!({"provider":p.id,"id":m.id,"contextWindow":m.context_window,"reasoning":m.reasoning}))).collect::<Vec<_>>()}),
                        ),
                        Command::GetAvailableThinkingLevels => Ok(
                            json!({"levels":if s.selected(&config).is_ok_and(|(p,m)|m.reasoning&&p.api==config::Api::Responses){vec!["off","low","medium","high"]}else{vec!["off"]}}),
                        ),
                        Command::SetModel { provider, model_id } if !busy => {
                            let selected = config
                                .providers
                                .iter()
                                .find(|p| p.id == provider)
                                .and_then(|p| p.models.iter().find(|m| m.id == model_id));
                            if selected.is_none() {
                                return Err("Model is not configured".into());
                            }
                            s.model = Some((provider, model_id));
                            s.thinking = "off".into();
                            s.context = None;
                            s.save()?;
                            Ok(s.state(&config))
                        }
                        Command::SetThinkingLevel { level } if !busy => {
                            let (p, m) = s.selected(&config)?;
                            if level != "off"
                                && (!m.reasoning
                                    || p.api != config::Api::Responses
                                    || !matches!(level.as_str(), "low" | "medium" | "high"))
                            {
                                return Err("Unsupported thinking level for selected model".into());
                            }
                            s.thinking = level;
                            s.save()?;
                            Ok(json!({}))
                        }
                        Command::NewSession if !busy => {
                            *s = Session::open(&super::bridge::SessionStart::New, &config)?;
                            Ok(json!({}))
                        }
                        Command::SwitchSession { session_path } if !busy => {
                            *s = Session::open(
                                &super::bridge::SessionStart::Selected(session_path),
                                &config,
                            )?;
                            Ok(json!({}))
                        }
                        _ => Err("Command unavailable while a run is active".into()),
                    }
                })();
                emit(
                    &out,
                    match result {
                        Ok(data) => response(&id, data),
                        Err(e) => failure(&id, &e.to_string()),
                    },
                )
                .await?;
            }
        }
    }
    cancel.send_replace(true);
    if let Some(handle) = active {
        let _ = handle.await;
    }
    drop(out);
    reading.abort();
    writing.abort();
    Ok(())
}
async fn run_turns(
    mut prompt: Prompt,
    session: Arc<Mutex<Session>>,
    config: Arc<Config>,
    extensions: Arc<crate::extensions::Catalog>,
    out: &mpsc::Sender<Value>,
    queue: &Arc<Mutex<Queue>>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<()> {
    emit(out, Event::Started.wire()).await?;
    loop {
        maybe_compact(&session, &config, out, cancel).await?;
        {
            let mut s = session.lock().unwrap();
            let mut blocks = vec![Block::Text { text: prompt.text }];
            blocks.extend(prompt.images.into_iter().map(|i| Block::Image {
                mime_type: i.mime_type,
                data: i.data,
            }));
            let message = Message::User { content: blocks };
            s.messages.push(message);
            s.save()?;
        }
        let user = session.lock().unwrap().messages.last().cloned().unwrap();
        emit(out, Event::MessageStarted(&user).wire()).await?;
        let mut completed = false;
        for _ in 0..config.max_steps {
            if *cancel.borrow() {
                break;
            }
            maybe_compact(&session, &config, out, cancel).await?;
            let steering = {
                let mut q = queue.lock().unwrap();
                if q.items.front().is_some_and(|p| p.steer) {
                    q.items.pop_front()
                } else {
                    None
                }
            };
            if let Some(steering) = steering {
                let mut content = vec![Block::Text {
                    text: steering.text,
                }];
                content.extend(steering.images.into_iter().map(|i| Block::Image {
                    mime_type: i.mime_type,
                    data: i.data,
                }));
                let message = Message::User { content };
                {
                    let mut s = session.lock().unwrap();
                    s.messages.push(message.clone());
                    s.save()?;
                }
                emit(out, Event::MessageStarted(&message).wire()).await?;
                let state = queued(&queue.lock().unwrap());
                emit(out, state).await?;
            }
            let snapshot = session.lock().unwrap().clone();
            let (p, m) = snapshot.selected(&config)?;
            let instructions = super::policy::compose(&snapshot.cwd);
            let registry = extensions.snapshot();
            let mut definitions = tools::definitions();
            definitions.extend(registry.definitions().cloned());
            let call = provider::request(
                p,
                m,
                &snapshot.messages,
                &instructions,
                &snapshot.thinking,
                Some(&definitions),
                out,
            );
            let reply = tokio::select! {r=call=>r?,_=cancel.changed()=>break};
            let calls = if let Message::Assistant { content, .. } = &reply.message {
                content
                    .iter()
                    .filter_map(|b| {
                        if let Block::ToolCall {
                            id,
                            name,
                            arguments,
                        } = b
                        {
                            Some((id.clone(), name.clone(), arguments.clone()))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![]
            };
            emit(out, Event::MessageFinished(&reply.message).wire()).await?;
            {
                let mut s = session.lock().unwrap();
                s.messages.push(reply.message);
                record_usage(&mut s, reply.input, reply.output, reply.cache);
                s.context = reply.context;
                s.save()?;
            }
            if calls.is_empty() {
                completed = true;
                break;
            }
            for (id, name, args) in calls {
                emit(
                    out,
                    Event::ToolStarted {
                        id: &id,
                        name: &name,
                        args: &args,
                    }
                    .wire(),
                )
                .await?;
                let mut steps = session.lock().unwrap().steps.clone();
                let cwd = snapshot.cwd.clone();
                let started = std::time::Instant::now();
                let result = if *cancel.borrow() {
                    Err("Tool cancelled before execution".into())
                } else if registry.has_tool(&name) {
                    registry
                        .execute_tool(&id, &name, &args, (&cwd, &snapshot.id), cancel)
                        .await
                } else if let Some(tool) = crate::extensions::AgentTool::parse(&name) {
                    extensions
                        .execute(tool, &args, (&cwd, &snapshot.id), cancel)
                        .await
                } else {
                    tools::execute(&name, &args, &cwd, &mut steps, cancel).await
                };
                let elapsed = started.elapsed();
                let (is_error, result) = match result {
                    Ok(v) => (v["isError"] == true, v),
                    Err(e) => {
                        let details = match e.downcast_ref::<tools::ShellExit>() {
                            Some(shell) => json!({"exitCode":shell.code()}),
                            None => match e.downcast_ref::<crate::extensions::Exit>() {
                                Some(exit) => json!({"exitCode":exit.code}),
                                None => json!({}),
                            },
                        };
                        (
                            true,
                            json!({"content":[{"type":"text","text":e.to_string()}],"details":details}),
                        )
                    }
                };
                let message = Message::ToolResult {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    content: vec![Block::Text {
                        text: result["content"][0]["text"].as_str().unwrap_or("").into(),
                    }],
                    is_error,
                    details: result["details"].clone(),
                };
                {
                    let mut s = session.lock().unwrap();
                    s.messages.push(message);
                    s.steps = steps;
                    s.save()?;
                }
                emit(
                    out,
                    Event::ToolFinished {
                        id: &id,
                        name: &name,
                        failed: is_error,
                        result: &result,
                        elapsed,
                    }
                    .wire(),
                )
                .await?;
            }
        }
        if !completed && !*cancel.borrow() {
            return Err(
                "Agent reached maxSteps. Review the work and submit a new prompt to continue."
                    .into(),
            );
        }
        if *cancel.borrow() {
            queue.lock().unwrap().items.clear();
            return Ok(());
        }
        let next = {
            let mut q = queue.lock().unwrap();
            let next = q.items.pop_front();
            if next.is_none() {
                q.running = false;
            }
            next
        };
        let update = queued(&queue.lock().unwrap());
        emit(out, update).await?;
        match next {
            Some(next) => prompt = next,
            None => break,
        }
    }
    Ok(())
}
async fn compact(
    session: Arc<Mutex<Session>>,
    config: Arc<Config>,
    out: &mpsc::Sender<Value>,
    instructions: Option<&str>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Value> {
    let snapshot = session.lock().unwrap().clone();
    if snapshot.messages.is_empty() {
        return Err("No conversation to compact".into());
    }
    let (p, m) = snapshot.selected(&config)?;
    let prompt=format!("Summarize this conversation for continuation. Preserve user requirements, decisions, changed files, verified results, unresolved work and tool outcomes. Do not execute tools. {}",instructions.unwrap_or(""));
    let reply = tokio::select! {r=provider::request(p,m,&snapshot.messages,&prompt,"off",None,out)=>r?,_=cancel.changed()=>return Err("Cancelled compact".into())};
    let summary = reply.message.text();
    if summary.trim().is_empty() {
        return Err("Empty compaction summary; history unchanged".into());
    }
    // Retain original history in a separate immutable archive before replacing active context.
    let mut s = session.lock().unwrap();
    let archive =
        crate::paths::home()?
            .join("archives")
            .join(format!("{}-{}.jsonl", s.id, config::unique()));
    if s.path.exists() {
        config::atomic(&archive, &std::fs::read(&s.path)?)?;
    }
    s.messages = vec![Message::User {
        content: vec![Block::Text {
            text: format!("Previous conversation summary:\n{summary}"),
        }],
    }];
    record_usage(&mut s, reply.input, reply.output, reply.cache);
    s.context = None;
    s.save()?;
    Ok(json!({"summary":summary,"tokensBefore":snapshot.context}))
}

fn record_usage(s: &mut Session, input: Option<u64>, output: Option<u64>, cache: Option<u64>) {
    for (key, next) in [("input", input), ("output", output), ("cacheRead", cache)] {
        s.usage[key] = match (s.usage[key].as_u64(), next) {
            (Some(old), Some(next)) => json!(old.saturating_add(next)),
            _ => Value::Null,
        };
    }
}

async fn maybe_compact(
    session: &Arc<Mutex<Session>>,
    config: &Arc<Config>,
    out: &mpsc::Sender<Value>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<()> {
    let needs_compaction = {
        let s = session.lock().unwrap();
        s.selected(config)
            .ok()
            .and_then(|(_, m)| m.context_window)
            .zip(s.context)
            .is_some_and(|(window, used)| used > window.saturating_mul(4) / 5)
    };
    if needs_compaction {
        emit(out, json!({"type":"compaction_start"})).await?;
        let value = compact(session.clone(), config.clone(), out, None, cancel).await?;
        emit(out, json!({"type":"compaction_end","result":value})).await?;
    }
    Ok(())
}
