//! 中栏：大纲。
//!
//! 由当前会话**推导**出来，不单独存一份 —— 存了就会有和正文对不上的一天。
//! 点任意一行，右栏滚到那条消息并短暂高亮。

use crate::model::{OutlineKind, OutlineNode};
use crate::theme;
use crate::ui::Workspace;

use gpui::{div, prelude::*, px, rems, Context, Div, FontWeight, Stateful};
use gpui_component::{h_flex, scroll::Scrollbar, v_flex, ActiveTheme as _, Icon, IconName};

impl Workspace {
    pub(crate) fn render_outline(&mut self, cx: &mut Context<Self>) -> Div {
        let nodes = self.outline();
        let count = nodes.len();

        let mut rows = v_flex().gap_1().py(px(4.));
        if nodes.is_empty() {
            rows = rows.child(theme::empty(cx, "还没有内容。发一条消息，大纲就出来了。"));
        } else {
            for node in nodes {
                rows = rows.child(self.outline_row(node, cx));
            }
        }

        let scroll_area = div()
            .id("outline-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.outline_scroll)
            .child(rows);

        theme::column(cx)
            .child(theme::header(
                cx,
                "大纲",
                Some(
                    div()
                        .text_size(rems(theme::META))
                        .text_color(theme::muted(cx))
                        .child(format!("{count} 条")),
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
                            .child(Scrollbar::vertical(&self.outline_scroll).id("outline-sb")),
                    ),
            )
    }

    fn outline_row(&mut self, node: OutlineNode, cx: &mut Context<'_, Self>) -> Stateful<Div> {
        let highlighted = self.highlight.as_deref() == Some(node.message_id.as_str());
        let is_turn = node.kind == OutlineKind::UserTurn;
        // 一轮提问顶格，标题按其 markdown 层级缩进，一眼能看出从属关系。
        let indent = px(12. + node.depth as f32 * 12.);
        let message_id = node.message_id.clone();

        h_flex()
            .id(format!("outline-row-{}", node.message_id))
            .mx(px(theme::ROW_INSET))
            .pl(indent)
            .pr(px(8.))
            .py(px(4.))
            .gap_1()
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .when(highlighted, |this| this.bg(theme::selected_bg(cx)))
            .hover(|style| style.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, _window, cx| this.jump_to(message_id.clone(), cx)))
            .child(
                Icon::new(if is_turn {
                    IconName::User
                } else {
                    IconName::ChevronRight
                })
                .size_3()
                .text_color(theme::muted(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(rems(if is_turn { theme::TEXT } else { theme::META }))
                    .when(is_turn, |this| this.font_weight(FontWeight::MEDIUM))
                    .when(!is_turn, |this| this.text_color(theme::muted(cx)))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(node.label.clone()),
            )
            .when(node.streaming, |this| {
                this.child(
                    Icon::new(IconName::Loader)
                        .size_3()
                        .text_color(cx.theme().info),
                )
            })
    }
}
