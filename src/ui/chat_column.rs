//! 右栏：对话。
//!
//! 消息列表 + 输入区。两条非显然的决定：
//!
//! 1. 助手的定稿用 `TextView::markdown` 渲染（标题、代码块、列表、行内样式
//!    它都管），但**正在流式接收的那条用纯文本逐行画**。理由有两个：每收到
//!    几个 token 就整篇重解析一遍 markdown 太费；而且半个 `**` 会让它渲染
//!    出成片的星号，抖得厉害。收完再切回 markdown，一次到位。
//!
//! 2. 跳转靠 `on_prepaint` 逐帧记录每条消息的位置。消息高度各不相同，
//!    `scroll_to_item` 那种"按索引 × 等高"的算法用不上。

use crate::model::{format_time, Role};
use crate::theme;
use crate::ui::Workspace;

use gpui::{
    div, prelude::*, px, rems, Bounds, ClipboardItem, Context, Div, FontWeight, Pixels,
    SharedString, Stateful, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    scroll::Scrollbar,
    text::TextView,
    v_flex, ActiveTheme as _, ElementExt as _, IconName, Sizable as _,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// 一条消息的渲染快照。
///
/// 先把需要的东西拷出来再建行：建行时要反复可变借用 `self`（拿 `cx`
/// 建回调、读 `highlight`），手上攥着 `&Conversation` 的引用会打架。
struct MessageView {
    id: String,
    role: Role,
    content: String,
    streaming: bool,
    error: Option<String>,
    time: String,
}

impl Workspace {
    pub(crate) fn render_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let snapshots: Vec<MessageView> = self
            .active()
            .map(|conversation| {
                conversation
                    .messages
                    .iter()
                    .map(|m| MessageView {
                        id: m.id.clone(),
                        role: m.role,
                        content: m.content.clone(),
                        streaming: m.streaming,
                        error: m.error.clone(),
                        time: format_time(m.created_at),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let title: SharedString = self
            .active()
            .map(|c| c.title.clone())
            .unwrap_or_else(|| "没有打开的对话".to_string())
            .into();
        // 顶栏显示"哪个服务 / 哪个模型"，一眼能看出这条消息会发去哪里。
        let model = self
            .store
            .server()
            .map(|s| format!("{} · {}", s.name, s.model))
            .unwrap_or_else(|| "未配置服务".to_string());

        // 上一帧的滚动偏移。配合下面记下的容器 y，才能把消息的窗口坐标
        // 换算成内容坐标。
        let offset_y = self.chat_scroll.offset().y.as_f32();
        let top_cell = self.scroll_view_top.clone();
        let offsets = self.jump_offsets.clone();

        let mut rows = v_flex().gap_3().px(px(16.)).py(px(12.));
        if snapshots.is_empty() {
            rows = rows.child(theme::empty(
                cx,
                "问点什么吧。Enter 发送，Shift+Enter 换行。",
            ));
        } else {
            for snapshot in snapshots {
                rows = rows.child(self.message_row(snapshot, offset_y, &top_cell, &offsets, cx));
            }
        }

        let scroll_area = div()
            .id("chat-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.chat_scroll)
            // 记下滚动容器这一帧在窗口里的 y。父元素先于子元素 prepaint，
            // 所以下面的消息行读到的一定是本帧的值。
            .on_prepaint({
                let top_cell = top_cell.clone();
                move |bounds: Bounds<Pixels>, _, _| top_cell.set(bounds.origin.y.as_f32())
            })
            .child(rows);

        theme::column(cx)
            .child(theme::header(
                cx,
                title,
                Some(
                    h_flex().gap_2().items_center().child(
                        div()
                            .text_size(rems(theme::META))
                            .text_color(theme::muted(cx))
                            .child(model),
                    ),
                ),
            ))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(scroll_area)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .left_0()
                            .child(Scrollbar::vertical(&self.chat_scroll).id("chat-sb")),
                    ),
            )
            .child(self.render_composer(cx))
    }

    fn render_composer(&mut self, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().border;
        let streaming = self.is_streaming();

        div()
            .flex_shrink_0()
            .px(px(12.))
            .py(px(8.))
            .border_t_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_end()
                    .child(div().flex_1().min_w_0().child(Input::new(&self.composer)))
                    .child(
                        Button::new("send")
                            .icon(if streaming {
                                IconName::Loader
                            } else {
                                IconName::ArrowUp
                            })
                            .label(if streaming { "停止" } else { "发送" })
                            .primary()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.is_streaming() {
                                    this.stop(cx);
                                } else {
                                    this.send(window, cx);
                                }
                            })),
                    ),
            )
    }

    fn message_row(
        &mut self,
        view: MessageView,
        offset_y: f32,
        top_cell: &Rc<Cell<f32>>,
        offsets: &Rc<RefCell<HashMap<String, f32>>>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_user = view.role == Role::User;
        let highlighted = self.highlight.as_deref() == Some(view.id.as_str());

        let body: Div = if let Some(error) = view.error.clone() {
            div()
                .text_size(rems(theme::TEXT))
                .text_color(cx.theme().danger)
                .child(error)
        } else if !is_user && !view.streaming && !view.content.is_empty() {
            // 定稿的助手消息走 markdown。
            div().child(TextView::markdown(
                format!("md-{}", view.id),
                view.content.clone(),
            ))
        } else {
            plain_lines(&view.content)
        };

        let copy_content = view.content.clone();

        div()
            .id(format!("msg-{}", view.id))
            .on_prepaint({
                let id = view.id.clone();
                let top_cell = top_cell.clone();
                let offsets = offsets.clone();
                move |bounds: Bounds<Pixels>, _, _| {
                    offsets
                        .borrow_mut()
                        .insert(id, bounds.origin.y.as_f32() - top_cell.get() + offset_y);
                }
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_size(rems(theme::META))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if is_user {
                                        cx.theme().foreground
                                    } else {
                                        theme::muted(cx)
                                    })
                                    .child(view.role.label()),
                            )
                            .child(
                                div()
                                    .text_size(rems(theme::META))
                                    .text_color(theme::muted(cx))
                                    .child(view.time.clone()),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new(format!("copy-{}", view.id))
                                    .icon(IconName::Copy)
                                    .tooltip("复制这条消息")
                                    .ghost()
                                    .xsmall()
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy_content.clone(),
                                        ));
                                    }),
                            ),
                    )
                    .child(
                        div()
                            // 自己的消息给个底色区分，助手的不加 —— 助手的
                            // 回复里常有代码块，套一层灰底反而把代码块的
                            // 背景顶掉。
                            .when(is_user, |this| {
                                this.bg(cx.theme().muted)
                                    .rounded(theme::CARD_RADIUS)
                                    .px(px(10.))
                                    .py(px(6.))
                            })
                            .when(highlighted, |this| {
                                this.border_2().border_color(cx.theme().primary)
                            })
                            .child(body),
                    ),
            )
    }
}

/// 逐行画纯文本。
///
/// 这个版本的 gpui 的 `WhiteSpace` 只有 Normal 和 Nowrap，没有 `Pre`，
/// 所以换行只能自己拆：一行一个 div，空行给个空格占位，否则高度会塌掉。
fn plain_lines(text: &str) -> Div {
    let mut out = v_flex();
    let mut empty = true;
    for line in text.lines() {
        empty = false;
        out = out.child(
            div()
                .text_size(rems(theme::TEXT))
                .whitespace_normal()
                .child(if line.is_empty() {
                    SharedString::from(" ")
                } else {
                    SharedString::from(line.to_string())
                }),
        );
    }
    if empty {
        out = out.child(div().text_size(rems(theme::TEXT)).child(" "));
    }
    out
}
