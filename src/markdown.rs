//! 从会话内容里抽大纲。
//!
//! 渲染助手消息用的是 gpui-component 的 `TextView::markdown`，这一层只做
//! 大纲栏需要的那点解析：认出 markdown 标题，并且**跳过代码块里的井号**。
//! Python 的 `# 注释` 混进大纲是最容易犯的错，所以围栏状态必须跟踪。

use crate::model::{Conversation, OutlineKind, OutlineNode, Role};

/// 抽出所有 ATX 标题，返回 `(层级 1..=6, 文字)`。
pub fn headings(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_fence = false;

    for line in text.lines() {
        let line = line.trim_end();
        let trimmed = line.trim_start();

        // ``` 或 ~~~ 都算围栏；开关状态而不是"找配对"，因为流式输出
        // 到一半时结尾的围栏还没到。
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes == 0 || hashes > 6 {
            continue;
        }
        // '#' 是 ASCII，字符数等于字节数，切字节索引是安全的。
        let mut rest = trimmed[hashes..].trim();
        // 闭合式标题（## 标题 ##）去掉尾巴上的井号。
        rest = rest.trim_end_matches('#').trim();
        if rest.is_empty() {
            continue;
        }
        out.push((hashes, rest.to_string()));
    }

    out
}

/// 一条会话 → 大纲栏的行。
///
/// 一轮对话（用户提问）是 0 级；它下面挂助手回复里的标题，层级直接用
/// markdown 的 1..=6。助手回复一个标题都没有时，回退成一行"助手回复"，
/// 否则这一轮在大纲里就只剩提问，点不到答案。
pub fn build_outline(conversation: &Conversation) -> Vec<OutlineNode> {
    let mut nodes = Vec::new();

    for message in &conversation.messages {
        match message.role {
            Role::System => continue,
            Role::User => {
                if message.content.trim().is_empty() {
                    continue;
                }
                nodes.push(OutlineNode {
                    message_id: message.id.clone(),
                    label: message.outline_label(),
                    depth: 0,
                    kind: OutlineKind::UserTurn,
                    streaming: message.streaming,
                });
            }
            Role::Assistant => {
                if message.content.trim().is_empty() {
                    continue;
                }
                let found = headings(&message.content);
                if found.is_empty() {
                    nodes.push(OutlineNode {
                        message_id: message.id.clone(),
                        label: message.outline_label(),
                        depth: 1,
                        kind: OutlineKind::AssistantTurn,
                        streaming: message.streaming,
                    });
                } else {
                    for (level, text) in found {
                        nodes.push(OutlineNode {
                            message_id: message.id.clone(),
                            label: text,
                            depth: level,
                            kind: OutlineKind::Heading,
                            streaming: message.streaming,
                        });
                    }
                }
            }
        }
    }

    nodes
}

#[cfg(test)]
mod tests {
    use super::headings;

    #[test]
    fn 代码块里的井号不算标题() {
        let md = "## 真正的标题\n\n```python\n# 这是一条注释\nprint(1)\n```\n\n### 另一个标题";
        let got = headings(md);
        assert_eq!(
            got,
            vec![(2, "真正的标题".to_string()), (3, "另一个标题".to_string())]
        );
    }

    #[test]
    fn 波浪号围栏同样被跳过() {
        assert!(headings("~~~sh\n# not a heading\n~~~\n").is_empty());
    }

    #[test]
    fn 闭合式标题去掉尾部井号() {
        assert_eq!(headings("## 标题 ##"), vec![(2, "标题".to_string())]);
    }

    #[test]
    fn 七个井号不是标题() {
        assert!(headings("####### 太长").is_empty());
    }
}
