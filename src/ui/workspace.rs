//! 工作区：一个窗口里唯一的主视图。
//!
//! 结构只有两层 —— 顶栏，加一条 `h_resizable` 三栏。三栏本身分别在
//! `history_column` / `outline_column` / `chat_column` 里画，这里只负责
//! 状态、动作和把它们装配起来。

use crate::ai::{self, StreamMsg};
use crate::markdown;
use crate::model::{Conversation, Message, OutlineNode, Role};
use crate::store::{Config, Store};
use crate::theme;

use gpui::{
    div, point, prelude::*, px, Context, Entity, Pixels, ScrollHandle, SharedString, Task, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, InputState},
    resizable::{h_resizable, resizable_panel, ResizableState},
    theme::{Theme, ThemeMode},
    v_flex, ActiveTheme as _, IconName, Selectable as _, Sizable as _,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// 流式回复的运行时状态。
struct Streaming {
    conversation_id: String,
    cancel: Arc<AtomicBool>,
    #[allow(dead_code)]
    task: Task<()>,
}

pub struct Workspace {
    /// 会话与设置，落盘的唯一出口。
    pub store: Store,
    pub active_id: Option<String>,
    /// 历史栏的搜索词。
    pub filter: String,

    // ---- 布局 ----
    pub resizable: Entity<ResizableState>,
    pub history_visible: bool,
    pub outline_visible: bool,

    // ---- 控件 ----
    pub composer: Entity<InputState>,
    pub search: Entity<InputState>,
    pub cfg_base_url: Entity<InputState>,
    pub cfg_api_key: Entity<InputState>,
    pub cfg_model: Entity<InputState>,
    pub cfg_system: Entity<InputState>,

    pub settings_open: bool,

    streaming: Option<Streaming>,
    /// 顶栏右下角的一句话状态（错误、正在接收……）。
    pub status: Option<SharedString>,

    // ---- 滚动与跳转 ----
    pub chat_scroll: ScrollHandle,
    pub history_scroll: ScrollHandle,
    pub outline_scroll: ScrollHandle,
    /// 每条消息在会话内容坐标系里的 y，由 `on_prepaint` 逐帧记录。
    pub jump_offsets: Rc<RefCell<HashMap<String, f32>>>,
    /// 滚动容器这一帧在窗口里的 y。
    pub scroll_view_top: Rc<Cell<f32>>,
    /// 待跳转的消息 id 与剩余尝试次数。
    pub pending_jump: Option<(String, u8)>,
    /// 刚跳转到的消息，短暂高亮。
    pub highlight: Option<String>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = Store::load();
        let dark = store.config.dark_mode;
        let history_width = store.config.history_width;
        let outline_width = store.config.outline_width;

        let resizable = cx.new(|_| ResizableState::default());

        let composer = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(1, 8)
                // 普通 Enter 交给 PressEnter 去发消息，Shift+Enter 才换行。
                .submit_on_enter(true)
                .placeholder("发消息…（Enter 发送，Shift+Enter 换行）")
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索历史…"));

        let cfg_base_url = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(store.config.base_url.clone())
                .placeholder("https://api.openai.com/v1")
        });
        let cfg_api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(store.config.api_key.clone())
                .placeholder("sk-…（留空表示服务端不需要密钥）")
        });
        let cfg_model = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(store.config.model.clone())
                .placeholder("gpt-4o-mini")
        });
        let cfg_system = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(3)
                .default_value(store.config.system_prompt.clone())
                .placeholder("系统提示词")
        });

        // 主题在窗口渲染之前定好，免得先白后黑闪一下。
        Theme::change(
            if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            None,
            cx,
        );

        // 恢复上次拖出来的栏宽。`resize_panel` 会把多出来的空间分给兄弟栏。
        resizable.update(cx, |state, cx| {
            state.resize_panel(0, px(history_width), window, cx);
            state.resize_panel(1, px(outline_width), window, cx);
        });

        let mut this = Self {
            store,
            active_id: None,
            filter: String::new(),
            resizable,
            history_visible: true,
            outline_visible: true,
            composer,
            search,
            cfg_base_url,
            cfg_api_key,
            cfg_model,
            cfg_system,
            settings_open: false,
            streaming: None,
            status: None,
            chat_scroll: ScrollHandle::new(),
            history_scroll: ScrollHandle::new(),
            outline_scroll: ScrollHandle::new(),
            jump_offsets: Rc::new(RefCell::new(HashMap::new())),
            scroll_view_top: Rc::new(Cell::new(0.)),
            pending_jump: None,
            highlight: None,
        };

        // 默认选中最近一条会话，省得每次开窗口都是空白。
        this.active_id = this.store.ordered().first().map(|c| c.id.clone());

        cx.subscribe_in(
            &this.composer,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary, shift } = event {
                    if !secondary && !shift {
                        this.send(window, cx);
                    }
                }
            },
        )
        .detach();

        cx.subscribe_in(
            &this.search,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.filter = this.search.read(cx).value().trim().to_string();
                    cx.notify();
                }
            },
        )
        .detach();

        this
    }

    // ---- 查询 ----

    pub fn active(&self) -> Option<&Conversation> {
        self.active_id.as_deref().and_then(|id| self.store.get(id))
    }

    pub fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }

    pub fn outline(&self) -> Vec<OutlineNode> {
        self.active()
            .map(markdown::build_outline)
            .unwrap_or_default()
    }

    /// 历史栏要显示的会话（先过滤，再按时间分桶）。
    ///
    /// 返回的是 `(分组标题, 会话 id 列表)`，id 是 owned 的 —— 后面建行时
    /// 还要反复借用 `self` 拿 `cx` 建回调，把引用留在 Vec 里会打架。
    pub(crate) fn history_groups(&self) -> Vec<(&'static str, Vec<String>)> {
        let filter = self.filter.to_lowercase();
        let ordered = self.store.ordered();
        let matches: Vec<&Conversation> = ordered
            .into_iter()
            .filter(|c| {
                if filter.is_empty() {
                    return true;
                }
                c.title.to_lowercase().contains(&filter)
                    || c.messages
                        .iter()
                        .any(|m| m.content.to_lowercase().contains(&filter))
            })
            .collect();

        let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
        let day = 86_400;
        let now = crate::model::now_secs();
        let today_start = now - (now % day);

        for conversation in matches {
            let label = if conversation.updated_at >= today_start {
                "今天"
            } else if conversation.updated_at + day >= today_start {
                "昨天"
            } else if conversation.updated_at + 7 * day >= today_start {
                "最近七天"
            } else {
                "更早"
            };
            match groups.last_mut() {
                Some((last, ids)) if *last == label => ids.push(conversation.id.clone()),
                _ => groups.push((label, vec![conversation.id.clone()])),
            }
        }
        groups
    }

    // ---- 动作 ----

    pub(crate) fn new_conversation(&mut self, cx: &mut Context<Self>) {
        let conversation = Conversation::new();
        let id = self.store.add(conversation);
        self.active_id = Some(id);
        self.chat_scroll.set_offset(point(px(0.), px(0.)));
        self.pending_jump = None;
        self.highlight = None;
        self.status = None;
        self.store.save();
        cx.notify();
    }

    pub(crate) fn select_conversation(&mut self, id: String, cx: &mut Context<Self>) {
        if self.active_id.as_deref() == Some(id.as_str()) {
            return;
        }
        self.active_id = Some(id);
        self.highlight = None;
        self.pending_jump = None;
        self.status = None;
        self.chat_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(crate) fn delete_conversation(&mut self, id: String, cx: &mut Context<Self>) {
        self.store.remove(&id);
        if self.active_id.as_deref() == Some(id.as_str()) {
            self.active_id = self.store.ordered().first().map(|c| c.id.clone());
        }
        self.store.save();
        cx.notify();
    }

    pub(crate) fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.history_visible = !self.history_visible;
        cx.notify();
    }

    pub(crate) fn toggle_outline(&mut self, cx: &mut Context<Self>) {
        self.outline_visible = !self.outline_visible;
        cx.notify();
    }

    pub(crate) fn toggle_dark(&mut self, cx: &mut Context<Self>) {
        let next = !self.store.config.dark_mode;
        self.store.config.dark_mode = next;
        self.store.save();
        Theme::change(
            if next {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            None,
            cx,
        );
        cx.refresh_windows();
    }

    pub(crate) fn jump_to(&mut self, message_id: String, cx: &mut Context<Self>) {
        // 布局要到下一帧才有新的 offset，所以留两次机会。
        self.pending_jump = Some((message_id, 2));
        self.highlight = None;
        cx.notify();
    }

    /// 把挂着的跳转真正做掉。在 render 开头调用，此时 `jump_offsets`
    /// 里躺的是上一帧量出来的位置。
    fn apply_pending_jump(&mut self) {
        let Some((id, tries)) = self.pending_jump.take() else {
            return;
        };
        match self.jump_offsets.borrow().get(&id).copied() {
            Some(y) => {
                self.chat_scroll
                    .set_offset(point(px(0.), px((y - 12.).max(0.))));
                self.highlight = Some(id);
            }
            None if tries > 1 => self.pending_jump = Some((id, tries - 1)),
            None => {}
        }
    }

    // ---- 发送 ----

    pub(crate) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.streaming.is_some() {
            return;
        }
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }

        let conversation_id = match self.active_id.clone() {
            Some(id) => id,
            None => {
                let conversation = Conversation::new();
                self.store.add(conversation)
            }
        };

        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));

        self.store
            .push_message(&conversation_id, Message::new(Role::User, text));
        let mut assistant = Message::new(Role::Assistant, String::new());
        assistant.streaming = true;
        self.store.push_message(&conversation_id, assistant);
        self.active_id = Some(conversation_id.clone());
        self.chat_scroll.scroll_to_bottom();
        self.store.save();

        // 请求体：只带已经落定、且非空的普通消息。正在流式接收的那条
        // 助手消息内容为空，会被过滤掉。
        let history: Vec<(String, String)> = self
            .store
            .get(&conversation_id)
            .map(|c| {
                c.messages
                    .iter()
                    .filter(|m| m.role != Role::System && !m.content.trim().is_empty())
                    .map(|m| (m.role.as_api_str().to_string(), m.content.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let (tx, rx) = mpsc::channel::<StreamMsg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let config: Config = self.store.config.clone();

        let task = cx.spawn(async move |this, cx| {
            // 阻塞的 HTTP 读放后台线程池，主线程只负责收增量。
            let worker = cx.background_executor().spawn(async move {
                ai::stream_chat(&config, history, worker_cancel, tx);
            });

            loop {
                let mut finished = false;
                // 一帧之内攒下的增量一次性贴完，避免每个 token 都刷一次屏。
                while let Ok(message) = rx.try_recv() {
                    match message {
                        StreamMsg::Delta(delta) => {
                            let _ = this.update(cx, |this, cx| this.on_delta(&delta, cx));
                        }
                        StreamMsg::Done => {
                            finished = true;
                            let _ = this.update(cx, |this, cx| this.on_finish(cx));
                        }
                        StreamMsg::Error(error) => {
                            finished = true;
                            let _ = this.update(cx, |this, cx| this.on_error(&error, cx));
                        }
                    }
                }
                if finished {
                    break;
                }
                // ~60fps 轮询。gpui 的前台 executor 就跑在主线程上，这里
                // 让出去才能让界面的点击、滚动照常响应。
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
            drop(worker);
        });

        self.streaming = Some(Streaming {
            conversation_id,
            cancel,
            task,
        });
        self.status = Some("正在接收…".into());
        cx.notify();
    }

    fn on_delta(&mut self, delta: &str, cx: &mut Context<Self>) {
        let Some(id) = self.streaming.as_ref().map(|s| s.conversation_id.clone()) else {
            return;
        };
        self.store.append_delta(&id, delta);
        // 跟着往下滚，否则新内容一直在视口外面。
        self.chat_scroll.scroll_to_bottom();
        cx.notify();
    }

    fn on_finish(&mut self, cx: &mut Context<Self>) {
        let Some(streaming) = self.streaming.take() else {
            return;
        };
        self.store.finish_streaming(&streaming.conversation_id);
        self.store.save();
        self.status = None;
        cx.notify();
    }

    fn on_error(&mut self, error: &str, cx: &mut Context<Self>) {
        let Some(streaming) = self.streaming.take() else {
            return;
        };
        self.store
            .mark_error(&streaming.conversation_id, error.to_string());
        self.store.save();
        self.status = Some(format!("出错了：{error}").into());
        cx.notify();
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(streaming) = self.streaming.take() else {
            return;
        };
        streaming.cancel.store(true, Ordering::Relaxed);
        self.store.finish_streaming(&streaming.conversation_id);
        self.store.save();
        self.status = None;
        cx.notify();
    }

    // ---- 设置 ----

    pub(crate) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_open {
            // 每次打开都从 config 重新灌一遍，免得改了没保存、关掉再开
            // 还显示着上次的草稿。
            let config = self.store.config.clone();
            self.sync_settings_inputs(&config, window, cx);
        }
        self.settings_open = !self.settings_open;
        cx.notify();
    }

    fn sync_settings_inputs(
        &mut self,
        config: &Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cfg_base_url.update(cx, |state, cx| {
            state.set_value(config.base_url.clone(), window, cx)
        });
        self.cfg_api_key.update(cx, |state, cx| {
            state.set_value(config.api_key.clone(), window, cx)
        });
        self.cfg_model.update(cx, |state, cx| {
            state.set_value(config.model.clone(), window, cx)
        });
        self.cfg_system.update(cx, |state, cx| {
            state.set_value(config.system_prompt.clone(), window, cx)
        });
    }

    pub(crate) fn save_settings(&mut self, cx: &mut Context<Self>) {
        let base_url = self.cfg_base_url.read(cx).value().trim().to_string();
        let api_key = self.cfg_api_key.read(cx).value().trim().to_string();
        let model = self.cfg_model.read(cx).value().trim().to_string();
        let system_prompt = self.cfg_system.read(cx).value().to_string();

        if !base_url.is_empty() {
            self.store.config.base_url = base_url;
        }
        if !model.is_empty() {
            self.store.config.model = model;
        }
        self.store.config.api_key = api_key;
        self.store.config.system_prompt = system_prompt;
        self.store.save();
        self.settings_open = false;
        cx.notify();
    }
}

impl gpui::Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        self.apply_pending_jump();

        let this = cx.entity();
        let active_title: SharedString = self
            .active()
            .map(|c| c.title.clone())
            .unwrap_or_else(|| "没有打开的对话".to_string())
            .into();
        let streaming = self.is_streaming();

        let history_width = px(self.store.config.history_width);
        let outline_width = px(self.store.config.outline_width);

        let content = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_top_bar(&active_title, streaming, window, cx))
            .child(
                div().flex_1().min_h_0().w_full().child(
                    h_resizable("trialog-workspace")
                        .with_state(&self.resizable)
                        .on_resize(move |state, _window, cx| {
                            let sizes: Vec<Pixels> = state.read(cx).sizes().clone();
                            let _ = this.update(cx, |this, _cx| {
                                if let Some(width) = sizes.first() {
                                    this.store.config.history_width = width.as_f32();
                                }
                                if let Some(width) = sizes.get(1) {
                                    this.store.config.outline_width = width.as_f32();
                                }
                                this.store.save();
                            });
                        })
                        .child(
                            resizable_panel()
                                .visible(self.history_visible)
                                .size(history_width)
                                // 侧栏不吃剩余空间，否则主栏一缩它就跟着长。
                                .flex_none()
                                .child(self.render_history(cx)),
                        )
                        .child(
                            resizable_panel()
                                .visible(self.outline_visible)
                                .size(outline_width)
                                .flex_none()
                                .child(self.render_outline(cx)),
                        )
                        .child(resizable_panel().child(self.render_chat(window, cx))),
                ),
            );

        let root = div().relative().size_full().child(content);
        if self.settings_open {
            root.child(self.render_settings(window, cx))
        } else {
            root
        }
    }
}

/// 顶栏：左边是当前会话标题，右边是四个开关。
impl Workspace {
    fn render_top_bar(
        &mut self,
        title: &SharedString,
        streaming: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let border = cx.theme().border;
        let muted = theme::muted(cx);

        h_flex()
            .h(theme::HEADER_H)
            .flex_shrink_0()
            .px(px(12.))
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(gpui::rems(theme::TEXT))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(title.clone()),
            )
            .when_some(self.status.clone(), |this, status| {
                this.child(
                    div()
                        .text_size(gpui::rems(theme::META))
                        .text_color(muted)
                        .text_ellipsis()
                        .max_w(px(320.))
                        .child(status),
                )
            })
            .child(
                Button::new("new-chat")
                    .icon(IconName::Plus)
                    .tooltip("新建对话")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, _window, cx| this.new_conversation(cx))),
            )
            .child(
                Button::new("toggle-history")
                    .icon(IconName::PanelLeft)
                    .tooltip("显示/隐藏 聊天历史")
                    .ghost()
                    .xsmall()
                    .selected(self.history_visible)
                    .on_click(cx.listener(|this, _, _window, cx| this.toggle_history(cx))),
            )
            .child(
                Button::new("toggle-outline")
                    .icon(IconName::PanelRight)
                    .tooltip("显示/隐藏 大纲")
                    .ghost()
                    .xsmall()
                    .selected(self.outline_visible)
                    .on_click(cx.listener(|this, _, _window, cx| this.toggle_outline(cx))),
            )
            .child(
                Button::new("toggle-theme")
                    .icon(if self.store.config.dark_mode {
                        IconName::Sun
                    } else {
                        IconName::Moon
                    })
                    .tooltip("明/暗")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, _window, cx| this.toggle_dark(cx))),
            )
            .child(
                Button::new("open-settings")
                    .icon(IconName::Settings)
                    .tooltip("设置")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_settings(window, cx);
                        window.refresh();
                    })),
            )
            .when(streaming, |this| {
                this.child(
                    Button::new("stop")
                        .icon(IconName::Close)
                        .label("停止")
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _window, cx| this.stop(cx))),
                )
            })
    }
}
