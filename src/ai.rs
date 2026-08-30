//! AI 接口客户端：OpenAI 兼容 与 Claude（Anthropic）两族协议。
//!
//! 用 ureq 而不是 reqwest：这里要的只是一条阻塞的 SSE 流，引入 tokio
//! 运行时纯属多余。请求跑在 gpui 的后台线程池上，主线程只管把收到的
//! 增量贴到界面上。
//!
//! 两族协议的差别全都收在 [`request`] 和 [`fold_data`] 里 —— URL 后缀、
//! 鉴权头、请求体怎么拼、SSE 的事件形状。往下读流的部分对两家是同一套。

use crate::store::{Server, ServerKind};
use serde::Deserialize;
use serde_json::json;
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
/// 只在后台线程调用，全程不碰 gpui。中途把 `cancel` 置 true 会在下一行
/// 数据到达时收尾。
pub fn stream_chat(
    server: &Server,
    history: Vec<(String, String)>,
    cancel: Arc<AtomicBool>,
    tx: Sender<StreamMsg>,
) {
    if let Err(e) = stream(server, &history, &cancel, &tx) {
        let _ = tx.send(StreamMsg::Error(e));
    }
}

fn stream(
    server: &Server,
    history: &[(String, String)],
    cancel: &AtomicBool,
    tx: &Sender<StreamMsg>,
) -> Result<(), String> {
    let request = build_request(server)?;

    let response = request
        .send_json(&request_body(server, history))
        .map_err(|e| match e {
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
        if n == 0 || cancel.load(Ordering::Relaxed) {
            break;
        }

        // 用 lossy 而不是 `lines()`：服务端偶尔会切出半个 UTF-8 序列，
        // `lines()` 那种情况下直接返回 Err，整条回复就没了。
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() {
            continue;
        }
        if payload == "[DONE]" {
            break;
        }

        match fold_data(payload, tx)? {
            Fold::Continue => {}
            Fold::Stop => break,
        }
    }

    let _ = tx.send(StreamMsg::Done);
    Ok(())
}

/// 一片 SSE data 的处理结果。
enum Fold {
    Continue,
    Stop,
}

/// 解析一片 `data:` 并把增量推出去。
///
/// 两家的事件形状差得挺远，但字段互不重叠（OpenAI 给 `choices`，Anthropic
/// 给顶层 `delta`），所以一个结构体就能同时吃下两边，省得维护两套解析器
/// 再靠分支挑——那种写法一旦哪天两家都加了 `type` 字段就会选错。
fn fold_data(payload: &str, tx: &Sender<StreamMsg>) -> Result<Fold, String> {
    // 心跳注释、认不出的字段，一律跳过 —— 一行脏数据不该掐断整段回复。
    let Ok(chunk) = serde_json::from_str::<Chunk>(payload) else {
        return Ok(Fold::Continue);
    };
    // Anthropic 出错时发 `event: error` + `data: {"type":"error","error":{...}}`；
    // OpenAI 系网关偶尔在 200 里塞 `{"error":{...}}`。形状一样，一起处理。
    if let Some(error) = chunk.error {
        let message = error
            .message
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "服务端返回了一个没有说明的错误".to_string());
        return Err(message);
    }

    if let Some(delta) = chunk.delta {
        // Anthropic：{"type":"content_block_delta","delta":{"type":"text_delta","text":"…"}}
        if let Some(text) = delta.text {
            if !text.is_empty() && tx.send(StreamMsg::Delta(text)).is_err() {
                return Ok(Fold::Stop);
            }
        }
        return Ok(Fold::Continue);
    }

    for choice in chunk.choices.unwrap_or_default() {
        if let Some(content) = choice.delta.and_then(|d| d.content) {
            if !content.is_empty() && tx.send(StreamMsg::Delta(content)).is_err() {
                return Ok(Fold::Stop);
            }
        }
    }

    Ok(Fold::Continue)
}

/// 拼请求：URL、鉴权头。
fn build_request(server: &Server) -> Result<ureq::Request, String> {
    let base = server.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("还没填接口地址。打开右上角的设置配上 Base URL。".to_string());
    }

    let path = match server.kind {
        ServerKind::OpenAi => "chat/completions",
        ServerKind::Claude => "messages",
    };
    let url = format!("{base}/{path}");

    let mut request = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream");

    let key = server.api_key.trim();
    match server.kind {
        ServerKind::OpenAi => {
            if !key.is_empty() {
                request = request.set("Authorization", &format!("Bearer {key}"));
            }
        }
        ServerKind::Claude => {
            // Anthropic 用 x-api-key，且必须带 anthropic-version，
            // 缺了后者是 400 而不是"少个 header"的提示。
            if !key.is_empty() {
                request = request.set("x-api-key", key);
            }
            request = request.set("anthropic-version", "2023-06-01");
        }
    }

    Ok(request)
}

/// 拼请求体。
fn request_body(server: &Server, history: &[(String, String)]) -> serde_json::Value {
    match server.kind {
        ServerKind::OpenAi => {
            let mut messages = Vec::with_capacity(history.len() + 1);
            if !server.system_prompt.trim().is_empty() {
                messages.push(json!({ "role": "system", "content": server.system_prompt }));
            }
            for (role, content) in history {
                messages.push(json!({ "role": role, "content": content }));
            }
            json!({
                "model": server.model,
                "messages": messages,
                "stream": true,
                "temperature": server.temperature,
            })
        }
        ServerKind::Claude => {
            // Anthropic 的 system 是独立字段，且流式时必须给 max_tokens。
            let messages: Vec<serde_json::Value> = history
                .iter()
                .map(|(role, content)| {
                    // "system" 不是合法角色，历史里不该出现；真出现了降级成 user，
                    // 总好过整个请求 400。
                    let role = if role == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    };
                    json!({ "role": role, "content": content })
                })
                .collect();
            let mut body = json!({
                "model": server.model,
                "messages": messages,
                "stream": true,
                "max_tokens": server.max_tokens,
            });
            if !server.system_prompt.trim().is_empty() {
                body["system"] = json!(server.system_prompt);
            }
            // Anthropic 的 temperature 只允许 0..=1，超了会 400。
            let temperature = server.temperature.clamp(0., 1.);
            body["temperature"] = json!(temperature);
            body
        }
    }
}

/// 拉模型列表。阻塞，跑在后台线程。
///
/// 两家的 `/models` 返回形状一致（`{"data":[{"id": "..."}]}`），Anthropic 多
/// 一个 `display_name`，这里只取 id。
pub fn fetch_models(server: &Server) -> Result<Vec<String>, String> {
    let base = server.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("还没填接口地址".to_string());
    }
    let url = format!("{base}/models");

    let mut request = ureq::get(&url).set("Accept", "application/json");
    let key = server.api_key.trim();
    match server.kind {
        ServerKind::OpenAi => {
            if !key.is_empty() {
                request = request.set("Authorization", &format!("Bearer {key}"));
            }
        }
        ServerKind::Claude => {
            if !key.is_empty() {
                request = request.set("x-api-key", key);
            }
            request = request.set("anthropic-version", "2023-06-01");
        }
    }

    let response = request.call().map_err(|e| match e {
        ureq::Error::Status(code, resp) => {
            let detail = resp.into_string().unwrap_or_default();
            let detail = detail.trim();
            if detail.is_empty() {
                format!("HTTP {code}")
            } else if detail.chars().count() > 200 {
                format!(
                    "HTTP {code}：{}…",
                    detail.chars().take(200).collect::<String>()
                )
            } else {
                format!("HTTP {code}：{detail}")
            }
        }
        other => format!("请求失败：{other}"),
    })?;

    // 模型列表可能很长，但也就是几十 KB，读成字符串再解析比手写流解析省心。
    let text = response
        .into_string()
        .map_err(|e| format!("读取模型列表失败：{e}"))?;
    let list: ModelList =
        serde_json::from_str(&text).map_err(|e| format!("模型列表解析失败：{e}"))?;

    let mut models: Vec<String> = list
        .data
        .into_iter()
        .filter_map(|entry| {
            entry
                .id
                .or(entry.name)
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
        })
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

// ---- SSE 载荷 -----------------------------------------------------------
// 一个结构体吃下两家：`choices` 是 OpenAI 的，顶层 `delta` 是 Anthropic 的，
// 两者不会同时出现。

#[derive(Debug, Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Option<Vec<Choice>>,
    #[serde(default)]
    delta: Option<Delta>,
    /// Anthropic 给 `{"type":"error","error":{...}}`；OpenAI 系网关偶尔在
    /// 200 里塞 `{"error":{...}}`。形状一样，一个字段够了。
    #[serde(default)]
    error: Option<ErrorBody>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Option<Delta>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    /// OpenAI
    #[serde(default)]
    content: Option<String>,
    /// Anthropic 的 text_delta
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelList {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    #[serde(default)]
    id: Option<String>,
    /// 少数兼容实现只给 `name`。
    #[serde(default)]
    name: Option<String>,
}
