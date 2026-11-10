use super::config::{self, Config, Model, Provider};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

/// Which session an engine opens.
#[derive(Clone)]
pub(crate) enum SessionStart {
    New,
    Latest,
    Selected(PathBuf),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Block {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image", rename_all = "camelCase")]
    Image { mime_type: String, data: String },
    #[serde(rename = "toolCall")]
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User { content: Vec<Block> },
    #[serde(rename = "assistant")]
    Assistant {
        content: Vec<Block>,
        #[serde(rename = "stopReason")]
        stop_reason: String,
        #[serde(default)]
        provider_data: Option<Value>,
    },
    #[serde(rename = "toolResult", rename_all = "camelCase")]
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: Vec<Block>,
        is_error: bool,
        details: Value,
    },
}
impl Message {
    pub fn text(&self) -> String {
        let blocks = match self {
            Self::User { content }
            | Self::Assistant { content, .. }
            | Self::ToolResult { content, .. } => content,
        };
        blocks
            .iter()
            .filter_map(|b| {
                if let Block::Text { text } = b {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub(super) rooxcode_session: super::rooxcode::SessionId,
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub model: Option<(String, String)>,
    pub thinking: String,
    pub messages: Vec<Message>,
    pub steps: Vec<Value>,
    pub usage: Value,
    pub context: Option<u64>,
    /// A name the user gave the session, shown in place of its first message.
    pub name: Option<String>,
}

/// A session name as stored: one trimmed line of at most 200 characters, or
/// `None` for an empty name, which restores the automatic title.
pub fn clean_name(name: &str) -> Option<String> {
    let line: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line: String = line.trim().chars().take(200).collect();
    let line = line.trim_end();
    (!line.is_empty()).then(|| line.to_owned())
}

/// Where saved sessions live.
fn sessions_dir() -> Result<PathBuf> {
    Ok(crate::paths::home()?.join("sessions"))
}

/// Names a saved session that no engine has open, by rewriting its header.
/// An empty name restores the automatic title.
pub fn rename_saved(path: &Path, name: &str) -> Result<()> {
    rename_in(&sessions_dir()?, path, name)
}

/// Deletes a saved session file.
pub fn delete_saved(path: &Path) -> Result<()> {
    delete_in(&sessions_dir()?, path)
}

fn delete_in(dir: &Path, path: &Path) -> Result<()> {
    Ok(fs::remove_file(owned(dir, path)?)?)
}

/// `path`, resolved, if it is a session file directly inside `dir`.
fn owned(dir: &Path, path: &Path) -> Result<PathBuf> {
    let dir = dir.canonicalize()?;
    let file = path.canonicalize()?;
    if file.parent() != Some(dir.as_path())
        || file.extension().is_none_or(|ext| ext != "jsonl")
        || !file.is_file()
    {
        return Err("Not a saved Hibiscus session".into());
    }
    Ok(file)
}

fn rename_in(dir: &Path, path: &Path, name: &str) -> Result<()> {
    let file = owned(dir, path)?;
    let text = fs::read_to_string(&file)?;
    let (head, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let mut head: Value = serde_json::from_str(head)?;
    if head["type"] != "session" || head["format"] != "hibiscus" {
        return Err("Only Hibiscus sessions can be renamed".into());
    }
    match clean_name(name) {
        Some(name) => head["name"] = name.into(),
        None => {
            if let Some(head) = head.as_object_mut() {
                head.remove("name");
            }
        }
    }
    let mut bytes = serde_json::to_vec(&head)?;
    bytes.push(b'\n');
    bytes.extend_from_slice(rest.as_bytes());
    config::atomic(&file, &bytes)
}

impl Session {
    /// Opens a session for `workspace`, the folder its tools work in.
    pub fn open(
        start: &SessionStart,
        workspace: &std::path::Path,
        config: &Config,
    ) -> Result<Self> {
        let cwd = workspace.canonicalize()?;
        let path = match start {
            SessionStart::Selected(p) => Some(p.clone()),
            SessionStart::Latest => crate::chat::saved_sessions::list(&cwd)?
                .first()
                .map(|s| s.path.clone()),
            SessionStart::New => None,
        };
        if let Some(path) = path {
            return Self::load(path, &cwd, config);
        }
        let id = format!("{:x}", config::unique());
        let selected = config.model.clone();
        let model = selected
            .map(|s| {
                s.split_once('/')
                    .map(|(p, m)| (p.to_owned(), m.to_owned()))
                    .ok_or("model must be provider/model")
            })
            .transpose()?;
        let session = Self {
            id: id.clone(),
            rooxcode_session: super::rooxcode::SessionId::new()?,
            path: crate::paths::home()?
                .join("sessions")
                .join(format!("{id}.jsonl")),
            cwd,
            model,
            thinking: "off".into(),
            messages: vec![],
            steps: vec![],
            usage: json!({"input":0,"output":0,"cacheRead":0}),
            context: None,
            name: None,
        };
        if session
            .model
            .as_ref()
            // Rooxcode and catalog providers load their models after the session opens.
            .is_some_and(|(p, _)| {
                p != super::rooxcode::ID && !super::catalog::discovered(config, p)
            })
        {
            session.selected(config)?;
        }
        Ok(session)
    }
    fn load(path: PathBuf, cwd: &std::path::Path, config: &Config) -> Result<Self> {
        let file = fs::File::open(&path)?;
        if file.metadata()?.len() > 128 * 1024 * 1024 {
            return Err("Session exceeds 128 MiB".into());
        }
        let mut lines = BufReader::new(file).lines();
        let head: Value = serde_json::from_str(&lines.next().ok_or("Empty session")??)?;
        if head["format"] != "hibiscus"
            || head["version"] != 1
            || head["cwd"].as_str() != cwd.to_str()
        {
            return Err("Not a supported Hibiscus session for this workspace. Original sessions are never modified.".into());
        }
        let mut session = Self::open(&SessionStart::New, cwd, config)?;
        session.path = path;
        session.id = head["id"].as_str().ok_or("Invalid session ID")?.into();
        if let Some(value) = head.get("rooxcodeSession") {
            session.rooxcode_session = serde_json::from_value(value.clone())
                .map_err(|_| "Invalid Rooxcode conversation UUID in session")?;
        }
        session.model = serde_json::from_value(head["model"].clone())?;
        session.thinking = head["thinking"].as_str().unwrap_or("off").into();
        session.steps = serde_json::from_value(head["steps"].clone()).unwrap_or_default();
        session.usage = head["usage"].clone();
        session.context = head["context"].as_u64();
        session.name = head["name"].as_str().and_then(clean_name);
        for line in lines {
            let value: Value = serde_json::from_str(&line?)?;
            if value["type"] == "message" {
                session
                    .messages
                    .push(serde_json::from_value(value["message"].clone())?);
            }
        }
        if session
            .model
            .as_ref()
            // Rooxcode and catalog providers load their models after the session opens.
            .is_some_and(|(p, _)| {
                p != super::rooxcode::ID && !super::catalog::discovered(config, p)
            })
        {
            session.selected(config)?;
        }
        let mut pending = std::collections::BTreeMap::<String, String>::new();
        for message in &session.messages {
            match message {
                Message::Assistant { content, .. } => {
                    for block in content {
                        if let Block::ToolCall { id, name, .. } = block {
                            pending.insert(id.clone(), name.clone());
                        }
                    }
                }
                Message::ToolResult { tool_call_id, .. } => {
                    pending.remove(tool_call_id);
                }
                _ => {}
            }
        }
        for (id, name) in pending {
            session.messages.push(Message::ToolResult{tool_call_id:id,tool_name:name,content:vec![Block::Text{text:"Interrupted before a result was recorded. Tool outcome is unknown; inspect the workspace before retrying.".into()}],is_error:true,details:json!({})});
        }
        Ok(session)
    }
    pub fn save(&self) -> Result<()> {
        let mut head = json!({"type":"session","format":"hibiscus","version":1,"id":self.id,"rooxcodeSession":self.rooxcode_session,"cwd":self.cwd,"model":self.model,"thinking":self.thinking,"steps":self.steps,"usage":self.usage,"context":self.context});
        if let Some(name) = &self.name {
            head["name"] = name.as_str().into();
        }
        let mut bytes = serde_json::to_vec(&head)?;
        bytes.push(b'\n');
        for message in &self.messages {
            serde_json::to_writer(&mut bytes, &json!({"type":"message","message":message}))?;
            bytes.push(b'\n');
        }
        config::atomic(&self.path, &bytes)
    }
    pub fn selected<'a>(&self, config: &'a Config) -> Result<(&'a Provider, &'a Model)> {
        let (p, m) = self
            .model
            .as_ref()
            .ok_or("No active model. Use /login to sign in, then choose a model with /models.")?;
        let provider = config
            .providers
            .iter()
            .find(|v| &v.id == p)
            .ok_or("Session provider is no longer configured")?;
        let model = provider
            .models
            .iter()
            .find(|v| &v.id == m)
            .ok_or(if p == super::rooxcode::ID { "Rooxcode selected model is unavailable. Open /models to refresh and choose explicitly." } else if super::catalog::discovered(config, p) { "The selected model is not in the provider's model list. Use /login to add its API key, or open /models to choose another." } else { "Session model is no longer configured" })?;
        Ok((provider, model))
    }
    pub fn state(&self, config: &Config) -> Value {
        let model = self
            .selected(config)
            .ok()
            .filter(|(p, _)| config::access(p).is_ok())
            .map(|(p, m)| json!({"provider":p.id,"id":m.id,"contextWindow":m.context_window}));
        let has_credentials = config.providers.iter().any(|p| {
            p.authentication != config::Authentication::Rooxcode && config::key(p).is_ok()
        });
        let has_access = config
            .providers
            .iter()
            .any(|p| !p.models.is_empty() && config::access(p).is_ok());
        let thinking = if self.thinking == "off"
            && self
                .model
                .as_ref()
                .is_some_and(|(p, _)| p == super::rooxcode::ID)
        {
            "default"
        } else {
            &self.thinking
        };
        json!({"sessionFile":if self.path.exists(){Some(&self.path)}else{None},"sessionId":self.id,"model":model,"thinkingLevel":thinking,"hasCredentials":has_credentials,"hasAccess":has_access})
    }
    /// Call only after authoritative discovery or an explicit model change.
    pub fn reconcile_rooxcode_thinking(&mut self, config: &Config) -> bool {
        if self.thinking != "off"
            && self
                .model
                .as_ref()
                .is_some_and(|(p, _)| p == super::rooxcode::ID)
            && !self.selected(config).is_ok_and(|(p, m)| {
                super::thinking::Policy::for_model(p, m).supports_stored(&self.thinking)
            })
        {
            self.thinking = "off".into();
            true
        } else {
            false
        }
    }
    pub fn stats(&self, config: &Config) -> Value {
        let window = self
            .selected(config)
            .ok()
            .and_then(|(_, m)| m.context_window);
        json!({"tokens":self.usage,"cost":null,"contextUsage":{"tokens":self.context,"contextWindow":window,"percent":self.context.zip(window).map(|(n,w)|100.0*n as f64/w as f64)}})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("hibiscus-{name}-{:x}", config::unique()));
        fs::create_dir(&root).unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn rename_names_a_session_and_an_empty_name_restores_its_title() {
        let root = temp("rename");
        let file = root.join("a.jsonl");
        let message = json!({"type":"message","message":{"role":"user","content":"first"}});
        fs::write(
            &file,
            format!(
                "{}\n{message}\n",
                json!({"type":"session","format":"hibiscus","version":1,"id":"a","cwd":root})
            ),
        )
        .unwrap();
        let head = |file: &Path| -> Value {
            serde_json::from_str(fs::read_to_string(file).unwrap().lines().next().unwrap()).unwrap()
        };
        rename_in(&root, &file, "  My\nsession  ").unwrap();
        assert_eq!(head(&file)["name"], "My session");
        assert!(fs::read_to_string(&file)
            .unwrap()
            .ends_with(&format!("\n{message}\n")));
        rename_in(&root, &file, "   ").unwrap();
        assert!(head(&file).get("name").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_and_delete_refuse_files_outside_the_sessions_directory() {
        let root = temp("owned");
        let sessions = root.join("sessions");
        fs::create_dir(&sessions).unwrap();
        let outside = root.join("a.jsonl");
        let header = json!({"type":"session","format":"hibiscus","version":1,"id":"a","cwd":root});
        fs::write(&outside, format!("{header}\n")).unwrap();
        let text = sessions.join("notes.txt");
        fs::write(&text, "keep").unwrap();
        let escape = sessions.join("..").join("a.jsonl");
        for path in [&outside, &text, &escape] {
            assert!(rename_in(&sessions, path, "x").is_err());
            assert!(delete_in(&sessions, path).is_err());
        }
        assert!(outside.exists() && text.exists());
        let inside = sessions.join("b.jsonl");
        fs::write(&inside, format!("{header}\n")).unwrap();
        delete_in(&sessions, &inside).unwrap();
        assert!(!inside.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_name_keeps_one_short_line() {
        assert_eq!(clean_name("  a\nb  ").as_deref(), Some("a b"));
        assert_eq!(clean_name(" \t "), None);
        assert_eq!(clean_name(&"x".repeat(300)).unwrap().len(), 200);
    }

    #[test]
    fn a_saved_name_survives_save_and_load() {
        let root = std::env::temp_dir().join(format!("hibiscus-name-{:x}", config::unique()));
        fs::create_dir(&root).unwrap();
        let cwd = root.canonicalize().unwrap();
        let config: Config = serde_json::from_value(json!({"providers":[]})).unwrap();
        let mut session = Session::open(&SessionStart::New, &cwd, &config).unwrap();
        session.path = cwd.join("s.jsonl");
        session.name = Some("Named".into());
        session.save().unwrap();
        let loaded =
            Session::open(&SessionStart::Selected(session.path.clone()), &cwd, &config).unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Named"));
        loaded.save().unwrap();
        assert!(fs::read_to_string(&session.path)
            .unwrap()
            .contains("\"name\":\"Named\""));
        fs::remove_dir_all(root).unwrap();
    }
}
