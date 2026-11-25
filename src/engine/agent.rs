use super::event::Event;
use super::{
    config::{self, Config},
    provider::{self, emit},
    session::{Block, Message, Session, SessionKind, SessionStart},
    tools,
};
use crate::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
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
    SetWorkingRepository {
        path: Option<std::path::PathBuf>,
    },
    #[serde(rename = "extension_command")]
    RunExtension {
        name: String,
        input: String,
    },
    GetMessages {
        /// Paged for the desktop chat when set (#171).
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        before: Option<usize>,
    },
    GetToolResult {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
    },
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
    NewSession {
        #[serde(default)]
        kind: SessionKind,
    },
    SwitchSession {
        #[serde(rename = "sessionPath")]
        session_path: std::path::PathBuf,
    },
    SetSessionName {
        name: String,
    },
    ClearQueue,
    Abort,
    Compact {
        #[serde(default, rename = "customInstructions")]
        custom_instructions: Option<String>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryPath {
    path: String,
}

pub(super) fn working_repository_path(args: &Value) -> Result<std::path::PathBuf> {
    let RepositoryPath { path } = serde_json::from_value(args.clone())?;
    if path.trim().is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err("Provide the exact checkout path.".into());
    }
    Ok(path.into())
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
    json!({"type":"response","id":id,"success":false,"error":super::safe_message(message)})
}
fn queued(q: &Queue) -> Value {
    json!({"type":"queue_update","steering":q.items.iter().filter(|p|p.steer).map(|p|&p.text).collect::<Vec<_>>(),"followUp":q.items.iter().filter(|p|!p.steer).map(|p|&p.text).collect::<Vec<_>>()})
}
fn select_working_repository(session: &mut Session, args: &Value) -> Result<Value> {
    let requested = working_repository_path(args)?;
    if let Some(repo) = &session.working_repository {
        if session.cwd.join(&requested).canonicalize().as_ref().ok() != Some(repo) {
            return Err("A working repository is already selected. Ask the user to switch it in the composer.".into());
        }
        return Ok(tools::result(
            format!("Already working in {}.", repo.display()),
            json!({}),
        ));
    }
    if session.messages.iter().rev().take_while(|message| !matches!(message, Message::User { .. })).any(|message| matches!(
        message,
        Message::ToolResult { tool_name, .. }
            if matches!(tool_name.as_str(), "edit" | "write" | "batch_edit" | "checkpoint_restore")
    )) {
        return Err("Work has already changed files in this turn. Ask the user to choose the working repository in the composer.".into());
    }
    session.set_working_repository(Some(&requested))?;
    Ok(tools::result(
        format!(
            "Working repository is {}. Relative paths now start there.",
            session.working_dir().display()
        ),
        json!({"workingDirectory":session.working_dir(),"repositoryChanged":true}),
    ))
}

pub fn serve(
    socket: crate::client::socket::Socket,
    config: Config,
    session: Session,
    desktop: bool,
) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(serve_async(socket, config, session, desktop))
}
async fn serve_async(
    socket: crate::client::socket::Socket,
    mut config: Config,
    session: Session,
    desktop: bool,
) -> Result<()> {
    socket.set_nonblocking(true)?;
    let (reader, mut writer) = crate::client::socket::AsyncSocket::from_std(socket)?.into_split();
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
    let mut catalog_error = None;
    let discover = session
        .model
        .as_ref()
        .is_none_or(|(p, _)| p == super::rooxcode::ID);
    let session = Arc::new(Mutex::new(session));
    if discover {
        catalog_error = refresh_rooxcode(&mut config, &session, &out).await?;
    }
    // Errors already shown are not repeated each time /models opens.
    let mut provider_errors = super::catalog::refresh(&mut config).await;
    let queue = Arc::new(Mutex::new(Queue::default()));
    let mut config = Arc::new(config);
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
    for error in catalog_error.iter().chain(&provider_errors) {
        emit(&out, json!({"type":"provider_error","error":error})).await?;
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
                        config::access(p)?;
                        // Refused before it's added to the conversation, so
                        // the prompt can be sent again after sign-in (#185).
                        if p.authentication == config::Authentication::Rooxcode {
                            super::reka::signed_in()?;
                        }
                        if !images.is_empty() && !m.images {
                            return Err("Selected model does not support images".into());
                        }
                        Ok(())
                    });
                if let Err(e) = validation {
                    let message = if e.to_string() != super::reka::SIGN_IN
                        && session
                            .lock()
                            .unwrap()
                            .model
                            .as_ref()
                            .is_none_or(|(p, _)| p == super::rooxcode::ID)
                    {
                        catalog_error.as_deref().unwrap_or("").to_owned()
                    } else {
                        String::new()
                    };
                    let message = if message.is_empty() {
                        e.to_string()
                    } else {
                        message
                    };
                    emit(&out, failure(&id, &message)).await?;
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
                        desktop,
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
                        let _ = emit(&o, Event::Failed(e.as_ref()).wire()).await;
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
                    let result = match snapshot.check_working_dir() {
                        Err(error) => Err(error),
                        Ok(()) => {
                            registry
                                .execute_command(
                                    &id,
                                    &name,
                                    &input,
                                    (snapshot.working_dir(), &snapshot.id),
                                    &mut stop,
                                )
                                .await
                        }
                    };
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
            Command::NewSession { kind } if !busy => {
                let start = match kind {
                    SessionKind::Chat => SessionStart::New,
                    SessionKind::Canvas if desktop => SessionStart::Canvas,
                    SessionKind::Canvas => {
                        emit(
                            &out,
                            failure(&id, "Open Canvas sessions in Hibiscus Desktop."),
                        )
                        .await?;
                        continue;
                    }
                };
                if config
                    .model
                    .as_deref()
                    .is_none_or(|m| m.starts_with("rooxcode/"))
                {
                    catalog_error = match super::rooxcode::refresh(Arc::make_mut(&mut config)).await
                    {
                        Ok(status) => status.warning().map(str::to_owned),
                        Err(error) => Some(error.to_string()),
                    };
                }
                let cwd = session.lock().unwrap().cwd.clone();
                match Session::open(&start, &cwd, &config) {
                    Ok(mut next) => {
                        select_rooxcode(&mut next, &config);
                        *session.lock().unwrap() = next;
                        if let Some(error) = &catalog_error {
                            emit(&out, json!({"type":"provider_error","error":error})).await?;
                        }
                        emit(&out, response(&id, json!({}))).await?;
                    }
                    Err(error) => emit(&out, failure(&id, &error.to_string())).await?,
                }
            }
            Command::SwitchSession { session_path } if !busy => {
                let cwd = session.lock().unwrap().cwd.clone();
                match Session::open(
                    &super::session::SessionStart::Selected(session_path),
                    &cwd,
                    &config,
                ) {
                    Ok(next) => {
                        let discover = next
                            .model
                            .as_ref()
                            .is_some_and(|(p, _)| p == super::rooxcode::ID);
                        *session.lock().unwrap() = next;
                        // The catalog loaded at startup, by a new session or by /models
                        // is enough to check this session; fetching it again would hold
                        // up every command after the switch (#159).
                        if discover && rooxcode_loaded(&config) {
                            reconcile_rooxcode(&config, &session, &out).await?;
                        } else if discover {
                            catalog_error =
                                refresh_rooxcode(Arc::make_mut(&mut config), &session, &out)
                                    .await?;
                        }
                        emit(&out, response(&id, json!({}))).await?;
                    }
                    Err(error) => emit(&out, failure(&id, &error.to_string())).await?,
                }
            }
            Command::GetAvailableModels => {
                let mut errors = vec![];
                if !busy {
                    catalog_error =
                        refresh_rooxcode(Arc::make_mut(&mut config), &session, &out).await?;
                    let fresh = super::catalog::refresh(Arc::make_mut(&mut config)).await;
                    errors = fresh
                        .iter()
                        .filter(|e| {
                            !provider_errors.contains(e)
                                || config.providers.iter().any(|p| {
                                    p.discover_models && e.starts_with(&format!("{}: ", p.id))
                                })
                        })
                        .cloned()
                        .collect();
                    provider_errors = fresh;
                }
                let catalog_error = catalog_error
                    .iter()
                    .chain(&errors)
                    .cloned()
                    .reduce(|a, b| format!("{a}\n{b}"));
                // Only providers the user can use; /login lists the rest.
                let models = config.providers.iter().filter(|p| config::access(p).is_ok()).flat_map(|p| p.models.iter().map(move |m| json!({"provider":p.id,"id":m.id,"contextWindow":m.context_window,"reasoning":m.reasoning,"images":m.images,"thinkingLevels":super::thinking::Policy::for_model(p, m).levels()}))).collect::<Vec<_>>();
                emit(
                    &out,
                    response(&id, json!({"models":models,"catalogError":catalog_error})),
                )
                .await?;
            }
            command => {
                let mut thinking_reset = false;
                let result = (|| -> Result<Value> {
                    let mut s = session.lock().unwrap();
                    match command {
                        Command::GetState => {
                            let mut state = s.state(&config);
                            state["extensionCommands"] =
                                serde_json::to_value(extensions.snapshot().commands())?;
                            Ok(state)
                        }
                        Command::SetWorkingRepository { path }
                            if desktop && !busy && s.kind == SessionKind::Chat =>
                        {
                            s.set_working_repository(path.as_deref())?;
                            Ok(json!({"workingDirectory":s.working_dir()}))
                        }
                        Command::GetMessages { limit: None, .. } => {
                            Ok(json!({"messages":s.messages}))
                        }
                        Command::GetMessages {
                            limit: Some(limit),
                            before,
                        } => Ok(super::paging::page(&s.messages, limit.min(1000), before)),
                        Command::GetToolResult { tool_call_id } => {
                            super::paging::tool_result(&s.messages, &tool_call_id)
                                .ok_or_else(|| "No tool result with that id".into())
                        }
                        Command::GetToolOutputs => {
                            Ok(json!({"outputs":super::tool_output::list(&s.messages)}))
                        }
                        Command::GetToolOutput { index } => {
                            Ok(json!({"output":super::tool_output::get(&s.messages, index)}))
                        }
                        Command::GetSessionStats => Ok(s.stats(&config)),
                        Command::GetAvailableThinkingLevels => {
                            let (p, m) = s.selected(&config)?;
                            Ok(json!({"levels":super::thinking::Policy::for_model(p, m).levels()}))
                        }
                        Command::SetModel { provider, model_id } if !busy => {
                            let selected = config
                                .providers
                                .iter()
                                .find(|p| p.id == provider)
                                .and_then(|p| p.models.iter().find(|m| m.id == model_id));
                            if selected.is_none() {
                                return Err("Model is not configured".into());
                            }
                            let previous = (s.model.clone(), s.thinking.clone(), s.context);
                            let rooxcode = provider == super::rooxcode::ID;
                            s.model = Some((provider, model_id));
                            if rooxcode {
                                thinking_reset = s.reconcile_rooxcode_thinking(&config);
                            } else {
                                s.thinking = "off".into();
                            }
                            s.context = None;
                            if let Err(error) = s.save() {
                                (s.model, s.thinking, s.context) = previous;
                                thinking_reset = false;
                                return Err(error);
                            }
                            Ok(s.state(&config))
                        }
                        Command::SetThinkingLevel { level } if !busy => {
                            let (p, m) = s.selected(&config)?;
                            let level = super::thinking::Policy::for_model(p, m)
                                .requested(&level)
                                .ok_or("Unsupported thinking level for selected model")?;
                            let previous = std::mem::replace(&mut s.thinking, level);
                            if let Err(error) = s.save() {
                                s.thinking = previous;
                                return Err(error);
                            }
                            Ok(json!({}))
                        }
                        Command::SetSessionName { name } if !busy => {
                            let previous =
                                std::mem::replace(&mut s.name, super::session::clean_name(&name));
                            // A session without a file yet keeps the name for its first save.
                            if s.path.exists() {
                                if let Err(error) = s.save() {
                                    s.name = previous;
                                    return Err(error);
                                }
                            }
                            Ok(json!({}))
                        }
                        _ => Err("Command unavailable while a run is active".into()),
                    }
                })();
                if thinking_reset {
                    emit(&out, Event::ThinkingReset.wire()).await?;
                }
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
#[allow(clippy::too_many_arguments)]
async fn run_turns(
    mut prompt: Prompt,
    session: Arc<Mutex<Session>>,
    config: Arc<Config>,
    extensions: Arc<crate::extensions::Catalog>,
    out: &mpsc::Sender<Value>,
    queue: &Arc<Mutex<Queue>>,
    cancel: &mut watch::Receiver<bool>,
    desktop: bool,
) -> Result<()> {
    emit(out, Event::Started.wire()).await?;
    loop {
        maybe_compact(&session, &config, out, cancel).await?;
        {
            let mut s = session.lock().unwrap();
            let text = crate::wsl::agent_text(s.working_dir(), prompt.text);
            let mut blocks = vec![Block::Text { text }];
            blocks.extend(prompt.images.into_iter().map(|i| Block::Image {
                mime_type: i.mime_type,
                data: i.data,
            }));
            let message = Message::User { content: blocks };
            s.messages.push(message);
            s.save()?;
        }
        let mut checkpoint_request = crate::checkpoints::Request::new(&session.lock().unwrap().id);
        let user = session.lock().unwrap().messages.last().cloned().unwrap();
        emit(out, Event::MessageStarted(&user).wire()).await?;
        let mut awaiting_answer = false;
        'steps: loop {
            if *cancel.borrow() {
                break;
            }
            let steering = {
                let mut q = queue.lock().unwrap();
                if q.items.front().is_some_and(|p| p.steer) {
                    q.items.pop_front()
                } else {
                    None
                }
            };
            if let Some(steering) = steering {
                checkpoint_request = crate::checkpoints::Request::new(&session.lock().unwrap().id);
                let cwd = session.lock().unwrap().working_dir().to_owned();
                let mut content = vec![Block::Text {
                    text: crate::wsl::agent_text(&cwd, steering.text),
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
            let surface = match (desktop, snapshot.kind) {
                (false, _) => tools::Surface::Terminal,
                (true, SessionKind::Chat) => tools::Surface::Chat,
                (true, SessionKind::Canvas) => tools::Surface::Canvas,
            };
            snapshot.check_working_dir()?;
            let working_dir = snapshot.working_dir();
            let parent = snapshot
                .working_repository
                .as_ref()
                .map(|_| snapshot.cwd.as_path());
            let mut instructions = super::policy::compose(working_dir, surface, parent);
            let pending_phase = super::goal::pending(&snapshot.steps);
            if let Some((scope, phase)) = pending_phase {
                instructions.push_str(&format!(
                    "\nActive implementation plan (task data; follow the latest user scope): {}\n",
                    json!({"scope":scope,"openPhase":phase})
                ));
            }
            let registry = extensions.snapshot();
            let mut definitions =
                tools::definitions(Some(working_dir), surface, pending_phase.is_some());
            definitions.extend(registry.definitions().cloned());
            let rooxcode = p.authentication == config::Authentication::Rooxcode;
            let exceeds = if rooxcode {
                super::context::pressure(p, m, &snapshot, &instructions, Some(&definitions))?
            } else {
                snapshot.messages.len() > 1
                    && super::context::known_context_pressure(
                        p,
                        m,
                        &snapshot,
                        &instructions,
                        &definitions,
                    )?
            };
            if exceeds {
                compact_automatically(&session, &config, out, cancel).await?;
            }
            let snapshot = session.lock().unwrap().clone();
            let (p, m) = snapshot.selected(&config)?;
            if p.authentication == config::Authentication::Rooxcode
                && super::context::pressure(p, m, &snapshot, &instructions, Some(&definitions))?
            {
                return Err("Rooxcode instructions, tools or recent conversation exceed the request budget after compaction. Reduce their size before continuing.".into());
            }
            let mut retry = false;
            let mut snapshot = snapshot;
            let mut reply = loop {
                let call = provider::request(
                    p,
                    m,
                    &snapshot,
                    &instructions,
                    &snapshot.thinking,
                    Some(&definitions),
                    out,
                );
                let result = tokio::select! { r = call => r, _ = cancel.changed() => break 'steps };
                match result {
                    Ok(reply) => break reply,
                    Err(error)
                        if !retry
                            && !rooxcode
                            && m.context_window.is_some()
                            && error.is::<provider::ContextOverflow>()
                            && snapshot.messages.len() > 1 =>
                    {
                        retry = true;
                        compact_automatically(&session, &config, out, cancel).await?;
                        snapshot = session.lock().unwrap().clone();
                    }
                    Err(error) => return Err(error),
                }
            };
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
            let mut phase_notice = None;
            if calls.is_empty() {
                let s = session.lock().unwrap();
                if let (Some((_, phase)), Message::Assistant { content, .. }) =
                    (super::goal::pending(&s.steps), &mut reply.message)
                {
                    let notice = format!(
                        "\n\nImplementation phase still open: {phase}. Verification is incomplete."
                    );
                    content.push(Block::Text {
                        text: notice.clone(),
                    });
                    phase_notice = Some(notice);
                }
            }
            let mut finished = Event::MessageFinished(&reply.message).wire();
            if let Some(notice) = phase_notice {
                finished["phaseNotice"] = json!(notice);
            }
            emit(out, finished).await?;
            {
                let mut s = session.lock().unwrap();
                s.messages.push(reply.message);
                record_usage(&mut s, reply.input, reply.output, reply.cache);
                s.context = reply.context;
                s.save()?;
            }
            if calls.is_empty() {
                break;
            }
            let mut output_budget = super::context::tool_output_budget(p, m);
            let total_calls = calls.len();
            let mixed_repository_call = surface == tools::Surface::Chat
                && total_calls > 1
                && calls
                    .iter()
                    .any(|(_, name, _)| name == "working_repository");
            for (index, (id, name, args)) in calls.into_iter().enumerate() {
                let output_limit = output_budget.map(|remaining| remaining / (total_calls - index));
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
                let cwd = snapshot.working_dir().to_owned();
                let started = std::time::Instant::now();
                let result = if *cancel.borrow() {
                    Err("Tool cancelled before execution".into())
                } else if mixed_repository_call {
                    Err("No calls ran. Call working_repository on its own so the next request can load that repository's instructions.".into())
                } else if name == "working_repository" && surface == tools::Surface::Chat {
                    let mut s = session.lock().unwrap();
                    select_working_repository(&mut s, &args)
                } else if registry.has_tool(&name) {
                    registry
                        .execute_tool(&id, &name, &args, (&cwd, &snapshot.id), cancel)
                        .await
                } else if let Some(tool) = crate::extensions::AgentTool::parse(&name) {
                    extensions
                        .execute(tool, &args, (&cwd, &snapshot.id), cancel)
                        .await
                } else if name == "goal" {
                    super::goal::execute(&args, &mut steps, &session.lock().unwrap().messages)
                } else {
                    tools::execute(
                        &name,
                        &args,
                        &mut crate::checkpoints::Context {
                            cwd: &cwd,
                            request: &mut checkpoint_request,
                            call_id: &id,
                        },
                        cancel,
                        output_limit,
                        surface,
                    )
                    .await
                };
                let elapsed = started.elapsed();
                let (is_error, mut result) = match result {
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
                if !is_error
                    && name == "bash"
                    && super::goal::pending(&steps).is_some()
                    && result["details"]["exitCode"].as_i64() == Some(0)
                    && result["details"]["reportedCrash"] != true
                {
                    let text = result["content"][0]["text"].as_str().unwrap_or("");
                    result["content"][0]["text"] =
                        json!(format!("{text}\nVerification call ID: {id}"));
                }
                if let Some(text) = result["content"][0]["text"].as_str() {
                    let capped = match output_limit {
                        Some(limit) if text.len() > limit => {
                            let limit = if super::capped::exempt(&name) {
                                limit
                            } else {
                                limit.min(super::capped::INLINE)
                            };
                            Some(super::capped::cap_to(text, limit))
                        }
                        _ if !super::capped::exempt(&name) => Some(super::capped::cap(text)),
                        _ => None,
                    };
                    if let Some(capped) = capped {
                        result["content"][0]["text"] = json!(capped);
                    }
                    if let Some(budget) = &mut output_budget {
                        *budget = budget.saturating_sub(
                            result["content"][0]["text"].as_str().map_or(0, str::len),
                        );
                    }
                }
                let message = Message::ToolResult {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    // The text, and any image a tool returns, such as a browser screenshot.
                    content: std::iter::once(Block::Text {
                        text: result["content"][0]["text"].as_str().unwrap_or("").into(),
                    })
                    .chain(
                        result["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|b| b["type"] == "image")
                            .filter_map(|b| serde_json::from_value(b.clone()).ok()),
                    )
                    .collect(),
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
                if name == "working_repository"
                    && !is_error
                    && result["details"]["repositoryChanged"] == true
                {
                    checkpoint_request = crate::checkpoints::Request::new(&snapshot.id);
                    emit(out, json!({"type":"working_repository_changed","workingDirectory":result["details"]["workingDirectory"]})).await?;
                }
                awaiting_answer |= !is_error && name == "ask_user";
                if !is_error && matches!(name.as_str(), "write" | "edit") {
                    if let Ok(scope) = serde_json::from_value(
                        result["details"]["checkpoint"]["memoryScope"].clone(),
                    ) {
                        emit(out, Event::MemorySaved(scope).wire()).await?;
                    }
                }
            }
            if awaiting_answer {
                break 'steps;
            }
        }
        if awaiting_answer {
            let pending = {
                let mut q = queue.lock().unwrap();
                q.running = false;
                std::mem::take(&mut q.items)
            };
            for item in pending {
                emit(out, json!({"type":"queue_rejected","message":item.text,"images":item.images,"steer":item.steer,"error":"The agent is waiting for an answer. Review this message before sending it again."})).await?;
            }
            let update = queued(&queue.lock().unwrap());
            emit(out, update).await?;
            break;
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
    if p.authentication == config::Authentication::Rooxcode {
        snapshot.save()?;
    }
    let prompt=format!("Summarize this conversation for continuation. Preserve user requirements, decisions, changed files, verified results, unresolved work and tool outcomes. Do not execute tools. {}",instructions.unwrap_or(""));
    let (reply, summary, tail) =
        super::context::summarize(p, m, &snapshot, &prompt, out, cancel).await?;
    if *cancel.borrow() {
        return Err("Cancelled compact; history unchanged".into());
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
    let mut updated = snapshot.clone();
    updated.messages = vec![Message::User {
        content: vec![Block::Text {
            text: format!("Previous conversation summary:\n{summary}"),
        }],
    }];
    updated.messages.extend(tail);
    record_usage(&mut updated, reply.input, reply.output, reply.cache);
    updated.context = None;
    updated.save()?;
    *s = updated;
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
            .filter(|(p, _)| p.authentication != config::Authentication::Rooxcode)
            .and_then(|(_, m)| m.context_window)
            .zip(s.context)
            .is_some_and(|(window, used)| used > window.saturating_mul(4) / 5)
    };
    if needs_compaction {
        compact_automatically(session, config, out, cancel).await?;
    }
    Ok(())
}

async fn compact_automatically(
    session: &Arc<Mutex<Session>>,
    config: &Arc<Config>,
    out: &mpsc::Sender<Value>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<()> {
    emit(out, json!({"type":"compaction_start"})).await?;
    let result = compact(session.clone(), config.clone(), out, None, cancel).await?;
    emit(out, json!({"type":"compaction_end","result":result})).await
}

fn select_rooxcode(session: &mut Session, config: &Config) {
    if session.model.is_none() && config.model.is_none() {
        session.model = config
            .providers
            .iter()
            .find(|p| p.id == super::rooxcode::ID)
            .and_then(|p| p.models.first().map(|m| (p.id.clone(), m.id.clone())));
    }
}

/// A failed discovery clears the unusable catalog but is not evidence that a
/// saved effort was removed. Only an authoritative catalog can reset it.
async fn refresh_rooxcode(
    config: &mut Config,
    session: &Mutex<Session>,
    out: &mpsc::Sender<Value>,
) -> Result<Option<String>> {
    match super::rooxcode::refresh(config).await {
        Ok(status) => {
            reconcile_rooxcode(config, session, out).await?;
            Ok(status.warning().map(str::to_owned))
        }
        Err(error) => Ok(Some(error.to_string())),
    }
}

/// Whether a Rooxcode catalog is already loaded.
fn rooxcode_loaded(config: &Config) -> bool {
    config
        .providers
        .iter()
        .any(|p| p.id == super::rooxcode::ID && !p.models.is_empty())
}

/// Checks the session's Rooxcode model and thinking level against the loaded catalog.
async fn reconcile_rooxcode(
    config: &Config,
    session: &Mutex<Session>,
    out: &mpsc::Sender<Value>,
) -> Result<()> {
    let reset = {
        let mut s = session.lock().unwrap();
        select_rooxcode(&mut s, config);
        let previous = s.thinking.clone();
        let reset = s.reconcile_rooxcode_thinking(config);
        if reset {
            if let Err(error) = s.save() {
                s.thinking = previous;
                return Err(error);
            }
        }
        reset
    };
    if reset {
        emit(out, Event::ThinkingReset.wire()).await?;
    }
    Ok(())
}
