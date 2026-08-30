//! 领域模型：会话、消息、大纲节点。
//!
//! 这一层不碰 gpui，也不碰文件系统 —— 存盘是 `store` 的事，画出来是 `ui` 的事。

use serde::{Deserialize, Serialize};

/// 单调 id。不上 uuid 依赖：一个进程内计数器加时间戳就够，且天然有序。
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}", now_secs(), n)
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 时间落款。今天显示 `14:03`，更早显示 `03-07`。
///
/// 用 UTC 而不是本地时区：不引入 chrono / tzdb 的前提下，本地时区要靠
/// libc 的 `localtime_r`，那是平台相关的活。会话列表里差几个小时不影响
/// 判断"这是不是刚才那条"，而错误的时区偏移反而会让人一头雾水。
pub fn format_time(secs: u64) -> String {
    const DAY: u64 = 86_400;
    let days = (secs / DAY) as i64;
    let rem = secs % DAY;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;

    let now_days = (now_secs() / DAY) as i64;
    if days == now_days {
        format!("{hour:02}:{minute:02}")
    } else {
        let (year, month, day_of_month) = civil_from_days(days);
        if days + 365 > now_days {
            format!("{month:02}-{day_of_month:02}")
        } else {
            format!("{year:04}-{month:02}-{day_of_month:02}")
        }
    }
}

/// Howard Hinnant 的 civil_from_days：把距 1970-01-01 的天数换算成
/// (年, 月, 日)。纯整数运算，不依赖平台。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    /// 气泡上的落款。
    pub fn label(&self) -> &'static str {
        match self {
            Role::System => "系统",
            Role::User => "我",
            Role::Assistant => "助手",
        }
    }

    /// OpenAI 兼容接口里的角色名。
    pub fn as_api_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub content: String,
    pub created_at: u64,
    /// 出错时的提示，正常为 None。
    #[serde(default)]
    pub error: Option<String>,
    /// 正在流式接收。`skip` 因为它只是运行时状态：存进盘再读回来，
    /// 一个"正在接收"的消息没有任何意义。
    #[serde(skip)]
    pub streaming: bool,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            role,
            content: content.into(),
            created_at: now_secs(),
            error: None,
            streaming: false,
        }
    }

    /// 大纲栏的条目文字：取第一行，砍掉 markdown 标记。
    pub fn outline_label(&self) -> String {
        let first = self
            .content
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("(空消息)");
        let trimmed = first.trim();
        // 去掉标题井号、引用符号、列表符号，大纲里这些是噪音。
        let cleaned = trimmed
            .trim_start_matches('#')
            .trim_start_matches('>')
            .trim_start_matches(|c| c == '-' || c == '*' || c == '+' || c == ' ');
        let cleaned = if cleaned.is_empty() { trimmed } else { cleaned };
        if cleaned.chars().count() > 40 {
            format!("{}…", cleaned.chars().take(40).collect::<String>())
        } else {
            cleaned.to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn new() -> Self {
        let now = now_secs();
        Self {
            id: new_id(),
            title: "新对话".to_string(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        }
    }

    /// 会话标题：用户没改过就用第一条提问的前 24 个字。
    pub fn touch_title(&mut self) {
        if self.title != "新对话" {
            return;
        }
        if let Some(first) = self.messages.iter().find(|m| m.role == Role::User) {
            let label: String = first.outline_label();
            if !label.is_empty() && label != "(空消息)" {
                self.title = label;
            }
        }
    }

    pub fn preview(&self) -> String {
        self.messages
            .iter()
            .rev()
            .find(|m| !m.content.trim().is_empty())
            .map(|m| m.outline_label())
            .unwrap_or_else(|| "还没有消息".to_string())
    }
}

/// 大纲栏的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    /// 点击后要跳转到的消息。
    pub message_id: String,
    pub label: String,
    /// 缩进层级：0 是一轮对话，1..=6 是这一轮里助手回复的 markdown 标题。
    pub depth: usize,
    pub kind: OutlineKind,
    /// 当前是否正在接收 —— 大纲里给个转圈提示，让人知道话还没说完。
    pub streaming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    /// 用户提问（一轮的开头）。
    UserTurn,
    /// 助手回复里的标题。
    Heading,
    /// 助手回复本身，当它不含任何标题时出现。
    AssistantTurn,
}
