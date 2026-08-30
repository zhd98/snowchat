//! 设置浮层：多个 AI 服务。
//!
//! 不是对话框，就是一层盖在工作区上的卡片：配置项不算多，用不上
//! `Root` 那套 Dialog/Sheet 的焦点管理，少一层间接也就少一处出错的地方。
//! 点卡片外面关掉。
//!
//! 左边是服务列表，右边是当前编辑的那个服务的字段。模型名既能从服务端
//! 刷新出来点选，也能手填 —— 各家兼容实现列出来的模型名常有对不上的。

use crate::store::ServerKind;
use crate::theme;
use crate::ui::Workspace;

use gpui::{div, prelude::*, px, rems, Context, Div, Entity, Stateful, Window};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme as _, Disableable as _, IconName, Selectable as _, Sizable as _,
};

/// 左列一行服务需要的全部信息。
///
/// 拷成 owned 再建行：建行时要反复可变借用 `self`（拿 `cx` 建回调），
/// 手上攥着 `&Server` 会打架。
struct ServerRow {
    id: String,
    name: String,
    kind: ServerKind,
    active: bool,
}

impl Workspace {
    pub(crate) fn render_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let rows: Vec<ServerRow> = self
            .store
            .config
            .servers
            .iter()
            .map(|s| ServerRow {
                id: s.id.clone(),
                name: s.name.clone(),
                kind: s.kind,
                active: self.store.config.active_server.as_deref() == Some(s.id.as_str()),
            })
            .collect();
        let editing_id = self.editing_server.clone();
        let editing_kind = editing_id
            .as_deref()
            .and_then(|id| self.store.find(id))
            .map(|s| s.kind);
        let editing_active = editing_id.as_deref() == self.store.config.active_server.as_deref();
        let models = self.models.clone();
        let hint = self.model_hint.clone();
        let fetching = self.fetching_models;
        let can_remove = rows.len() > 1;

        let mut list = v_flex().gap_1().p_2();
        for row in rows {
            list = list.child(self.server_row(row, can_remove, cx));
        }

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
                h_flex()
                    .w(px(880.))
                    .max_h(px(660.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .overflow_hidden()
                    // ---- 左列：服务列表 ----
                    .child(
                        v_flex()
                            .w(px(240.))
                            .flex_shrink_0()
                            .h_full()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(
                                theme::header(cx, "AI 服务", None),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .overflow_y_scroll()
                                    .child(list),
                            )
                            .child(
                                v_flex()
                                    .flex_shrink_0()
                                    .gap_1()
                                    .p_2()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        Button::new("add-openai")
                                            .label("+ OpenAI 兼容")
                                            .ghost()
                                            .xsmall()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_server(ServerKind::OpenAi, window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("add-claude")
                                            .label("+ Claude")
                                            .ghost()
                                            .xsmall()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_server(ServerKind::Claude, window, cx)
                                            })),
                                    ),
                            ),
                    )
                    // ---- 右列：当前服务的字段 ----
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                theme::header(
                                    cx,
                                    "设置",
                                    Some(
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child(
                                                Button::new("kind-openai")
                                                    .label(ServerKind::OpenAi.label())
                                                    .ghost()
                                                    .xsmall()
                                                    .selected(editing_kind == Some(ServerKind::OpenAi))
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        this.set_server_kind(
                                                            ServerKind::OpenAi,
                                                            window,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                            .child(
                                                Button::new("kind-claude")
                                                    .label(ServerKind::Claude.label())
                                                    .ghost()
                                                    .xsmall()
                                                    .selected(editing_kind == Some(ServerKind::Claude))
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        this.set_server_kind(
                                                            ServerKind::Claude,
                                                            window,
                                                            cx,
                                                        )
                                                    })),
                                            ),
                                    ),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .overflow_y_scroll()
                                    .child(
                                        v_flex()
                                            .gap_3()
                                            .p_4()
                                            .child(Self::field(
                                                cx,
                                                "名称",
                                                &self.cfg_name,
                                            ))
                                            .child(Self::field(
                                                cx,
                                                "接口地址",
                                                &self.cfg_base_url,
                                            ))
                                            .child(Self::field(
                                                cx,
                                                "API Key",
                                                &self.cfg_api_key,
                                            ))
                                            // 模型：输入框 + 刷新，下面一排可点的模型名
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_size(rems(theme::META))
                                                            .text_color(theme::muted(cx))
                                                            .child("模型"),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .items_start()
                                                            .child(
                                                                div()
                                                                    .flex_1()
                                                                    .min_w_0()
                                                                    .child(Input::new(
                                                                        &self.cfg_model,
                                                                    )),
                                                            )
                                                            .child(
                                                                Button::new("refresh-models")
                                                                    .icon(IconName::Loader)
                                                                    .label(if fetching {
                                                                        "拉取中"
                                                                    } else {
                                                                        "刷新"
                                                                    })
                                                                    .ghost()
                                                                    .xsmall()
                                                                    .on_click(cx.listener(
                                                                        |this, _, _window, cx| {
                                                                            this.refresh_models(cx)
                                                                        },
                                                                    )),
                                                            ),
                                                    )
                                                    .when_some(hint.clone(), |this, hint| {
                                                        this.child(
                                                            div()
                                                                .text_size(rems(theme::META))
                                                                .text_color(theme::muted(cx))
                                                                .child(hint),
                                                        )
                                                    })
                                                    .when(!models.is_empty(), |this| {
                                                        this.child(
                                                            div()
                                                                .h(px(104.))
                                                                .w_full()
                                                                .overflow_y_scroll()
                                                                .child(
                                                                    h_flex()
                                                                        .flex_wrap()
                                                                        .gap_1()
                                                                        .children(
                                                                            models
                                                                                .iter()
                                                                                .cloned()
                                                                                .map(|model| {
                                                                                    self.model_chip(
                                                                                        model, cx,
                                                                                    )
                                                                                }),
                                                                        ),
                                                                ),
                                                        )
                                                    }),
                                            )
                                            .child(Self::field(
                                                cx,
                                                "系统提示词",
                                                &self.cfg_system,
                                            ))
                                            .child(
                                                h_flex()
                                                    .gap_3()
                                                    .items_start()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .child(Self::field(
                                                                cx,
                                                                "温度 0–1（Claude 上限为 1）",
                                                                &self.cfg_temperature,
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .child(Self::field(
                                                                cx,
                                                                "最大 token",
                                                                &self.cfg_max_tokens,
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(rems(theme::META))
                                                    .text_color(theme::muted(cx))
                                                    .child(
                                                        "OpenAI 兼容一栏对 DeepSeek、通义、月之暗面、本地 Ollama 都适用，改地址与模型即可。",
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_shrink_0()
                                    .justify_end()
                                    .gap_2()
                                    .p_3()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        Button::new("settings-use")
                                            .label(if editing_active {
                                                "当前在用"
                                            } else {
                                                "设为在用"
                                            })
                                            .ghost()
                                            .small()
                                            .disabled(editing_active)
                                            .on_click({
                                                let id = editing_id.clone();
                                                cx.listener(move |this, _, _window, cx| {
                                                    if let Some(id) = id.clone() {
                                                        this.use_server(id, cx)
                                                    }
                                                })
                                            }),
                                    )
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
                    ),
            )
    }

    /// 左列的一行服务。
    fn server_row(
        &mut self,
        row: ServerRow,
        can_remove: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let editing = self.editing_server.as_deref() == Some(row.id.as_str());

        h_flex()
            .id(format!("server-row-{}", row.id))
            .px(px(8.))
            .py(px(6.))
            .gap_2()
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .when(editing, |this| this.bg(theme::selected_bg(cx)))
            .hover(|style| style.bg(cx.theme().list_hover))
            .on_click({
                let id = row.id.clone();
                cx.listener(move |this, _, window, cx| this.edit_server(id.clone(), window, cx))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(rems(theme::TEXT))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(row.name.clone()),
                            )
                            .when(row.active, |this| {
                                this.child(
                                    div()
                                        .text_size(rems(theme::META))
                                        .text_color(cx.theme().primary)
                                        .child("在用"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(rems(theme::META))
                            .text_color(theme::muted(cx))
                            .child(row.kind.label()),
                    ),
            )
            .when(can_remove, |this| {
                this.child(
                    Button::new(format!("server-delete-{}", row.id))
                        .icon(IconName::Delete)
                        .tooltip("删除这个服务")
                        .ghost()
                        .xsmall()
                        .on_click({
                            let id = row.id.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.remove_server(id.clone(), window, cx)
                            })
                        }),
                )
            })
    }

    /// 模型名做成小标签，点一下填进输入框。
    fn model_chip(&mut self, model: String, cx: &mut Context<Self>) -> Button {
        Button::new(format!("model-{}", model))
            .label(model.clone())
            .ghost()
            .xsmall()
            .on_click(
                cx.listener(move |this, _, window, cx| this.pick_model(model.clone(), window, cx)),
            )
    }

    fn field(cx: &Context<'_, Self>, label: &'static str, state: &Entity<InputState>) -> Div {
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
