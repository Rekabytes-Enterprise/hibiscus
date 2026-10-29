use super::{
    bridge::SessionStart,
    config::{self, Config, Model, Provider},
};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
};

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
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub model: Option<(String, String)>,
    pub thinking: String,
    pub messages: Vec<Message>,
    pub steps: Vec<Value>,
    pub usage: Value,
    pub context: Option<u64>,
}
impl Session {
    pub fn open(start: &SessionStart, config: &Config) -> Result<Self> {
        let cwd = std::env::current_dir()?.canonicalize()?;
        let path = match start {
            SessionStart::Selected(p) => Some(p.clone()),
            SessionStart::Latest => crate::chat::sessions::list(&cwd)?
                .first()
                .map(|s| s.path.clone()),
            SessionStart::New => None,
        };
        if let Some(path) = path {
            return Self::load(path, &cwd, config);
        }
        let id = format!("{:x}", config::unique());
        let selected = config.model.clone().or_else(|| {
            config
                .providers
                .iter()
                .flat_map(|p| p.models.iter().map(move |m| format!("{}/{}", p.id, m.id)))
                .next()
        });
        let model = selected
            .map(|s| {
                s.split_once('/')
                    .map(|(p, m)| (p.to_owned(), m.to_owned()))
                    .ok_or("model must be provider/model")
            })
            .transpose()?;
        let session = Self {
            id: id.clone(),
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
        };
        if session.model.is_some() {
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
        let mut session = Self::open(&SessionStart::New, config)?;
        session.path = path;
        session.id = head["id"].as_str().ok_or("Invalid session ID")?.into();
        session.model = serde_json::from_value(head["model"].clone())?;
        session.thinking = head["thinking"].as_str().unwrap_or("off").into();
        session.steps = serde_json::from_value(head["steps"].clone()).unwrap_or_default();
        session.usage = head["usage"].clone();
        session.context = head["context"].as_u64();
        for line in lines {
            let value: Value = serde_json::from_str(&line?)?;
            if value["type"] == "message" {
                session
                    .messages
                    .push(serde_json::from_value(value["message"].clone())?);
            }
        }
        if session.model.is_some() {
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
        let mut bytes = serde_json::to_vec(
            &json!({"type":"session","format":"hibiscus","version":1,"id":self.id,"cwd":self.cwd,"model":self.model,"thinking":self.thinking,"steps":self.steps,"usage":self.usage,"context":self.context}),
        )?;
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
            .ok_or("No model configured. Add providers/models to config.json and use /models.")?;
        let provider = config
            .providers
            .iter()
            .find(|v| &v.id == p)
            .ok_or("Session provider is no longer configured")?;
        let model = provider
            .models
            .iter()
            .find(|v| &v.id == m)
            .ok_or("Session model is no longer configured")?;
        Ok((provider, model))
    }
    pub fn state(&self, config: &Config) -> Value {
        let model = self
            .selected(config)
            .ok()
            .map(|(p, m)| json!({"provider":p.id,"id":m.id,"contextWindow":m.context_window}));
        json!({"sessionFile":if self.path.exists(){Some(&self.path)}else{None},"sessionId":self.id,"model":model,"thinkingLevel":self.thinking})
    }
    pub fn stats(&self, config: &Config) -> Value {
        let window = self
            .selected(config)
            .ok()
            .and_then(|(_, m)| m.context_window);
        json!({"tokens":self.usage,"cost":null,"contextUsage":{"tokens":self.context,"contextWindow":window,"percent":self.context.zip(window).map(|(n,w)|100.0*n as f64/w as f64)}})
    }
}
