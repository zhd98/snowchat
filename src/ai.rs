//! OpenAI 兼容接口的流式客户端（SSE）。
//!
//! 用 ureq 而不是 reqwest：这里要的只是一条阻塞的 SSE 流，引入 tokio
//! 运行时纯属多余。请求跑在 gpui 的后台线程池上，主线程只管把收到的
//! 增量贴到界面上。
//!
//! `base_url` 留成可配的，所以同一份代码对 OpenAI、DeepSeek、通义、
//! 以及本地 Ollama（`http://localhost:11434/v1`）都成立。

use crate::store::Config;
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// 后台线程推给主线程的三件事。
pub enum StreamMsg {
    /// 又收到一段文本。
    Delta(String),
    /// 正常读完。
    Done,
    /// 出错，带上给人看的原因。
    Error(String),
}

/// 阻塞地跑完一次对话，增量通过 `tx` 送出去。
///
/// 这个函数只在后台线程调用，全程不碰 gpui。中途把 `cancel` 置 true
/// 会在下一行数据到达时收尾。
pub fn stream_chat(
    config: &Config,
    history: Vec<(String, String)>,
    cancel: Arc<AtomicBool>,
    tx: Sender<StreamMsg>,
) {
    if let Err(e) = run(config, &history, &cancel, &tx) {
        let _ = tx.send(StreamMsg::Error(e));
    }
}

fn run(
    config: &Config,
    history: &[(String, String)],
    cancel: &AtomicBool,
    tx: &Sender<StreamMsg>,
) -> Result<(), String> {
    let base = config.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("还没填接口地址。打开右上角的设置配上 Base URL。".to_string());
    }
    let url = format!("{base}/chat/completions");

    // 前情提要先按会话顺序塞进去，系统提示在最前。
    let mut messages = Vec::with_capacity(history.len() + 1);
    if !config.system_prompt.trim().is_empty() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": config.system_prompt,
        }));
    }
    for (role, content) in history {
        messages.push(serde_json::json!({ "role": role, "content": content }));
    }

    let body = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "stream": true,
        "temperature": config.temperature,
    });

    let mut request = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream");
    if !config.api_key.trim().is_empty() {
        request = request.set(
            "Authorization",
            &format!("Bearer {}", config.api_key.trim()),
        );
    }

    let response = request.send_json(&body).map_err(|e| match e {
        ureq::Error::Status(code, resp) => {
            // ureq 默认把 4xx/5xx 当错误返回，正文里通常有服务端给的
            // 具体原因（"model not found" 之类），比光报一个状态码有用得多。
            let detail = resp.into_string().unwrap_or_default();
            let detail = detail.trim();
            if detail.is_empty() {
                format!("HTTP {code}")
            } else if detail.chars().count() > 400 {
                format!(
                    "HTTP {code}：{}…",
                    detail.chars().take(400).collect::<String>()
                )
            } else {
                format!("HTTP {code}：{detail}")
            }
        }
        other => format!("请求失败：{other}"),
    })?;

    let mut reader = BufReader::new(response.into_reader());
    let mut buf: Vec<u8> = Vec::with_capacity(512);

    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| format!("读取响应失败：{e}"))?;
        if n == 0 {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        // 用 lossy 而不是 `lines()`：服务端偶尔会切出半个 UTF-8 序列，
        // `lines()` 那种情况下直接返回 Err，整条回复就没了。
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload == "[DONE]" {
            break;
        }

        // 心跳注释、无法识别的字段，一律跳过 —— 一行脏数据不该掐断整段回复。
        let Ok(chunk) = serde_json::from_str::<Chunk>(payload) else {
            continue;
        };
        for choice in chunk.choices {
            if let Some(content) = choice.delta.and_then(|d| d.content) {
                if !content.is_empty() && tx.send(StreamMsg::Delta(content)).is_err() {
                    // 收的人没了（窗口关了），没必要再往下读。
                    return Ok(());
                }
            }
        }
    }

    let _ = tx.send(StreamMsg::Done);
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    /// 收尾的那一片只有 `finish_reason` 没有 `delta`，所以是 Option。
    #[serde(default)]
    delta: Option<Delta>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}
