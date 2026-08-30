//! 落盘：设置 + 全部会话。
//!
//! 位置统一在 `<配置目录>/trialog/`（Windows 上是
//! `%APPDATA%\trialog`），两个文件：`config.json` 和 `conversations.json`。
//!
//! 读失败一律降级成默认值而不是报错退出：一份坏掉的配置文件不该让
//! 应用打不开。写失败只记日志 —— 用户此刻正聊着天，弹窗没有意义。

use crate::model::{Conversation, Message};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// OpenAI 兼容接口的根地址，结尾不带斜杠。
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub dark_mode: bool,
    /// 三栏宽度，拖拽分隔条后写回。
    #[serde(default = "default_history_width")]
    pub history_width: f32,
    #[serde(default = "default_outline_width")]
    pub outline_width: f32,
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}
fn default_model() -> String {
    "gpt-4o-mini".to_string()
}
fn default_system_prompt() -> String {
    "你是一个简洁、准确的助手。用中文回答。".to_string()
}
fn default_temperature() -> f32 {
    0.7
}
fn default_history_width() -> f32 {
    240.
}
fn default_outline_width() -> f32 {
    260.
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: String::new(),
            model: default_model(),
            system_prompt: default_system_prompt(),
            temperature: default_temperature(),
            dark_mode: false,
            history_width: default_history_width(),
            outline_width: default_outline_width(),
        }
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
        let config: Config = read_json(&dir.join("config.json")).unwrap_or_default();
        let file: ConversationFile = read_json(&dir.join("conversations.json")).unwrap_or_default();

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
