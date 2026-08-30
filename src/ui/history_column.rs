//! 左栏：聊天历史。
//!
//! 按"今天 / 昨天 / 最近七天 / 更早"分桶，桶的划分跟 tty7 首页那套一致。
//! 每行是标题 + 最后一条消息预览。

use crate::model::format_time;
use crate::theme;
use crate::ui::Workspace;

use gpui::{div, prelude::*, px, rems, Context, Div, FontWeight, Stateful};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    scroll::Scrollbar,
    v_flex, ActiveTheme as _, IconName, Sizable as _,
};

impl Workspace {
    pub(crate) fn render_history(&mut self, cx: &mut Context<Self>) -> Div {
        let groups = self.history_groups();

        let mut rows = v_flex().gap_1().py(px(4.));
        if groups.is_empty() {
            rows = rows.child(theme::empty(
                cx,
                if self.filter.is_empty() {
                    "还没有对话，点上面的 + 开一个"
                } else {
                    "没有匹配的对话"
                },
            ));
        } else {
            for (label, ids) in groups {
                rows = rows.child(theme::section_heading(cx, label));
                for id in ids {
                    let selected = self.active_id.as_deref() == Some(id.as_str());
                    rows = rows.child(self.history_row(id, selected, cx));
                }
            }
        }

        let scroll_area = div()
            .id("history-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.history_scroll)
            .child(rows);

        theme::column(cx)
            .child(self.history_header(cx))
            .child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .flex_shrink_0()
                    .child(Input::new(&self.search)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(scroll_area)
                    // 滚动条浮在内容上，跟 tty7 的 `with_vertical_scrollbar`
                    // 同一个做法：外层绝对定位的盒子没有 hitbox，不会挡住
                    // 下面那一排会话的点击。
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .left_0()
                            .child(Scrollbar::vertical(&self.history_scroll).id("history-sb")),
                    ),
            )
    }

    fn history_header(&mut self, cx: &mut Context<Self>) -> Div {
        theme::header(
            cx,
            "聊天历史",
            Some(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("history-new")
                            .icon(IconName::Plus)
                            .tooltip("新建对话")
                            .ghost()
                            .xsmall()
                            .on_click(
                                cx.listener(|this, _, _window, cx| this.new_conversation(cx)),
                            ),
                    )
                    .child(self.count_badge(cx)),
            ),
        )
    }

    fn count_badge(&self, cx: &Context<'_, Self>) -> Div {
        div()
            .text_size(rems(theme::META))
            .text_color(theme::muted(cx))
            .child(format!("{}", self.store.conversations.len()))
    }

    fn history_row(
        &mut self,
        id: String,
        selected: bool,
        cx: &mut Context<'_, Self>,
    ) -> Stateful<Div> {
        let (title, preview, time) = match self.store.get(&id) {
            Some(conversation) => (
                conversation.title.clone(),
                conversation.preview(),
                format_time(conversation.updated_at),
            ),
            None => return div().id("history-row-missing"),
        };

        h_flex()
            .id(format!("history-row-{id}"))
            .mx(px(theme::ROW_INSET))
            .px(px(8.))
            .py(px(6.))
            .gap_2()
            .items_start()
            .rounded(px(6.))
            .when(selected, |this| this.bg(theme::selected_bg(cx)))
            .hover(|style| style.bg(cx.theme().list_hover))
            // 点击区只覆盖文字这一块，删除按钮是它的**兄弟**而不是子节点 ——
            // 于是两个点击目标互不重叠，用不着靠 stop_propagation 去拦冒泡。
            .child(
                v_flex()
                    .id(format!("history-select-{id}"))
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .cursor_pointer()
                    .on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, _window, cx| {
                            this.select_conversation(id.clone(), cx)
                        })
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(rems(theme::TEXT))
                                    .font_weight(FontWeight::MEDIUM)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(rems(theme::META))
                                    .text_color(theme::muted(cx))
                                    .child(time),
                            ),
                    )
                    .child(
                        div()
                            .text_size(rems(theme::META))
                            .text_color(theme::muted(cx))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(preview),
                    ),
            )
            .child(
                // 只有选中或悬停时露出，避免每会话都挂一个垃圾桶图标。
                div()
                    .invisible()
                    .when(selected, |this| this.visible())
                    .child(
                        Button::new(format!("history-delete-{id}"))
                            .icon(IconName::Delete)
                            .tooltip("删除这个对话")
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.delete_conversation(id.clone(), cx)
                            })),
                    ),
            )
    }
}
