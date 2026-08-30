//! 落盘：设置 + 全部会话。
//!
//! 位置统一在 `<配置目录>/trialog/`（Windows 上是
//! `%APPDATA%\trialog`），两个文件：`config.json` 和 `conversations.json`。
//!
//! 读失败一律降级成默认值而不是报错退出：一份坏掉的配置文件不该让
//! 应用打不开。写失败只记日志 —— 用户此刻正聊着天，弹窗没有意义。

use crate::model::{new_id, Conversation, Message};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 接口协议。
///
/// 只分这两族，是因为 wire format 真的不同：URL 后缀、鉴权头、请求体结构、
/// SSE 的事件形状都不一样。至于 DeepSeek、通义、月之暗面、本地 Ollama —— 它们
/// 都是 OpenAI 那一族的方言，选 `OpenAi` 改个地址就行，不需要各自的枚举值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    #[default]
    OpenAi,
    Claude,
}

impl ServerKind {
    pub fn label(&self) -> &'static str {
        match self {
            ServerKind::OpenAi => "OpenAI 兼容",
            ServerKind::Claude => "Claude",
        }
    }

    /// 换协议时用来填默认的地址与模型。
    pub fn default_url(&self) -> &'static str {
        match self {
            ServerKind::OpenAi => "https://api.openai.com/v1",
            ServerKind::Claude => "https://api.anthropic.com/v1",
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            ServerKind::OpenAi => "gpt-4o-mini",
            ServerKind::Claude => "claude-sonnet-4-5",
        }
    }
}

/// 一个 AI 服务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub kind: ServerKind,
    /// 接口根地址，结尾不带斜杠。
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub system_prompt: String,
    pub temperature: f32,
    /// 只 Claude 强制要求（流式时必须给 max_tokens），但两家都接受，
    /// 所以统一放在这里，省得为一个字段分叉出两套请求构造。
    pub max_tokens: u32,
}

impl Server {
    pub fn new(kind: ServerKind) -> Self {
        Self {
            id: new_id(),
            name: kind.label().to_string(),
            kind,
            base_url: kind.default_url().to_string(),
            api_key: String::new(),
            model: kind.default_model().to_string(),
            system_prompt: default_system_prompt(),
            temperature: 0.7,
            max_tokens: 4096,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 可以配多个服务，切换着用。
    #[serde(default)]
    pub servers: Vec<Server>,
    /// 当前发消息走哪一个（`Server::id`）。
    #[serde(default)]
    pub active_server: Option<String>,
    #[serde(default)]
    pub dark_mode: bool,
    /// 三栏宽度，拖拽分隔条后写回。
    #[serde(default = "default_history_width")]
    pub history_width: f32,
    #[serde(default = "default_outline_width")]
    pub outline_width: f32,

    // ---- 旧版（0.1.0）的扁平字段，只为迁移保留 ----
    // 读上来是 Some 就说明这是老配置，load 时折成一个 Server 再置 None。
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

fn default_system_prompt() -> String {
    "你是一个简洁、准确的助手。用中文回答。".to_string()
}
fn default_history_width() -> f32 {
    240.
}
fn default_outline_width() -> f32 {
    260.
}

impl Default for Config {
    fn default() -> Self {
        let server = Server::new(ServerKind::OpenAi);
        let id = server.id.clone();
        Self {
            servers: vec![server],
            active_server: Some(id),
            dark_mode: false,
            history_width: default_history_width(),
            outline_width: default_outline_width(),
            base_url: None,
            api_key: None,
            model: None,
            system_prompt: None,
            temperature: None,
        }
    }
}

impl Config {
    /// 把 0.1.0 时代的扁平配置折成一个服务。
    ///
    /// 只在 `servers` 为空时动手 —— 已经迁移过的配置不该被这些残留字段
    /// 再改一次。折完把旧字段清空，下次存盘就落干净了。
    fn migrate(&mut self) {
        if !self.servers.is_empty() {
            return;
        }
        let legacy = self.base_url.is_some()
            || self.api_key.is_some()
            || self.model.is_some()
            || self.system_prompt.is_some()
            || self.temperature.is_some();
        if !legacy {
            return;
        }

        let mut server = Server::new(ServerKind::OpenAi);
        if let Some(url) = self.base_url.take() {
            if !url.trim().is_empty() {
                server.base_url = url;
            }
        }
        if let Some(key) = self.api_key.take() {
            server.api_key = key;
        }
        if let Some(model) = self.model.take() {
            if !model.trim().is_empty() {
                server.model = model;
            }
        }
        if let Some(prompt) = self.system_prompt.take() {
            server.system_prompt = prompt;
        }
        if let Some(temperature) = self.temperature.take() {
            server.temperature = temperature;
        }

        let id = server.id.clone();
        self.servers.push(server);
        self.active_server = Some(id);
    }

    /// 当前在用的服务。
    pub fn server(&self) -> Option<&Server> {
        self.active_server
            .as_deref()
            .and_then(|id| self.servers.iter().find(|s| s.id == id))
            .or_else(|| self.servers.first())
    }

    pub fn server_mut(&mut self) -> Option<&mut Server> {
        let id = self.active_server.clone();
        match id {
            Some(id) => self.servers.iter_mut().find(|s| s.id == id),
            None => self.servers.first_mut(),
        }
    }

    pub fn find(&self, id: &str) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.id == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConversationFile {
    #[serde(default)]
    conversations: Vec<Conversation>,
}

pub struct Store {
    pub config: Config,
    pub conversations: Vec<Conversation>,
    dir: PathBuf,
}

impl Store {
    pub fn load() -> Self {
        let dir = config_dir();
        let mut config: Config = read_json(&dir.join("config.json")).unwrap_or_default();
        let file: ConversationFile = read_json(&dir.join("conversations.json")).unwrap_or_default();

        config.migrate();
        // 一个服务都没有（配置被手工清空过）就补一个默认的，
        // 否则界面上连"没有可用服务"之外的东西都画不出来。
        if config.servers.is_empty() {
            let server = Server::new(ServerKind::OpenAi);
            config.active_server = Some(server.id.clone());
            config.servers.push(server);
        }
        if config.server().is_none() {
            config.active_server = config.servers.first().map(|s| s.id.clone());
        }

        let mut conversations = file.conversations;
        // 存盘时任何"正在接收"的消息都已经被拒之门外（见 `Message::streaming`
        // 上的 `#[serde(skip)]`），读回来一律是完成态。这里再清一次，是为了
        // 覆盖被外部手工改过、或者旧版本写下的文件。
        for c in &mut conversations {
            for m in &mut c.messages {
                m.streaming = false;
            }
        }
        conversations.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Self {
            config,
            conversations,
            dir,
        }
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        if let Err(e) = write_json(&self.dir.join("config.json"), &self.config) {
            log::warn!("保存 config.json 失败：{e}");
        }
        if let Err(e) = write_json(
            &self.dir.join("conversations.json"),
            &ConversationFile {
                conversations: self.conversations.clone(),
            },
        ) {
            log::warn!("保存 conversations.json 失败：{e}");
        }
    }

    pub fn get(&self, id: &str) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Conversation> {
        self.conversations.iter_mut().find(|c| c.id == id)
    }

    pub fn add(&mut self, conversation: Conversation) -> String {
        let id = conversation.id.clone();
        self.conversations.insert(0, conversation);
        id
    }

    pub fn remove(&mut self, id: &str) {
        self.conversations.retain(|c| c.id != id);
    }

    /// 把一条消息追加到会话，并更新标题与时间。
    pub fn push_message(&mut self, conversation_id: &str, message: Message) {
        if let Some(c) = self.get_mut(conversation_id) {
            c.messages.push(message);
            c.updated_at = crate::model::now_secs();
            c.touch_title();
        }
    }

    /// 追加流式内容到会话的最后一条消息。
    pub fn append_delta(&mut self, conversation_id: &str, delta: &str) {
        if let Some(c) = self.get_mut(conversation_id) {
            if let Some(last) = c.messages.last_mut() {
                last.content.push_str(delta);
            }
        }
    }

    pub fn finish_streaming(&mut self, conversation_id: &str) {
        if let Some(c) = self.get_mut(conversation_id) {
            if let Some(last) = c.messages.last_mut() {
                last.streaming = false;
            }
            c.updated_at = crate::model::now_secs();
        }
    }

    /// 会话里最后一条助手消息置为出错态。
    pub fn mark_error(&mut self, conversation_id: &str, error: String) {
        if let Some(c) = self.get_mut(conversation_id) {
            if let Some(last) = c.messages.last_mut() {
                last.streaming = false;
                last.error = Some(error);
            }
        }
    }

    /// 最近使用排序：置顶正在用的，其余按更新时间倒序。
    pub fn ordered(&self) -> Vec<&Conversation> {
        let mut v: Vec<&Conversation> = self.conversations.iter().collect();
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        v
    }
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("trialog")
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &std::path::Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("{} 解析失败，用默认值：{e}", path.display());
            None
        }
    }
}

fn write_json<T: Serialize>(path: &std::path::Path, value: &T) -> anyhow::Result<()> {
    // 先写临时文件再改名：写一半断电不会留下半个 JSON，下一次启动还能
    // 读到上一份完整的。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
