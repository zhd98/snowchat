//! 设置浮层。
//!
//! 不是对话框，就是一层盖在工作区上的卡片：配置项少（四个），用不上
//! `Root` 那套 Dialog/Sheet 的焦点管理，少一层间接也就少一处出错的地方。
//! 点卡片外面关掉。

use crate::theme;
use crate::ui::Workspace;

use gpui::{div, prelude::*, px, rems, Context, Div, Entity, Window};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme as _, Sizable as _,
};

impl Workspace {
    pub(crate) fn render_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().border;

        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .bg(cx.theme().overlay)
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down_out(cx.listener(|this, _, _window, cx| {
                this.settings_open = false;
                cx.notify();
            }))
            .child(
                v_flex()
                    .w(px(520.))
                    .max_h(px(620.))
                    .p_4()
                    .gap_3()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(border)
                    .bg(cx.theme().background)
                    .child(
                        div()
                            .text_size(rems(theme::TEXT + 1. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("设置"),
                    )
                    .child(Self::settings_field(
                        cx,
                        "接口地址（OpenAI 兼容）",
                        &self.cfg_base_url,
                    ))
                    .child(Self::settings_field(cx, "API Key", &self.cfg_api_key))
                    .child(Self::settings_field(cx, "模型", &self.cfg_model))
                    .child(Self::settings_field(
                        cx,
                        "系统提示词",
                        &self.cfg_system,
                    ))
                    .child(
                        div()
                            .text_size(rems(theme::META))
                            .text_color(theme::muted(cx))
                            .child(
                                "同一个接口地址对 DeepSeek、通义、月之暗面以及本地的 Ollama（http://localhost:11434/v1）都适用。",
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("settings-cancel")
                                    .label("取消")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.settings_open = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("settings-save")
                                    .label("保存")
                                    .primary()
                                    .small()
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.save_settings(cx)
                                    })),
                            ),
                    ),
            )
    }

    fn settings_field(
        cx: &Context<'_, Self>,
        label: &'static str,
        state: &Entity<InputState>,
    ) -> Div {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(rems(theme::META))
                    .text_color(theme::muted(cx))
                    .child(label),
            )
            .child(Input::new(state))
    }
}
