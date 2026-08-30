//! 窗口 chrome：间距、圆角、字号阶梯，以及三栏共用的头部/分区行。
//!
//! 这一整套是照 tty7 的 `ui/theme.rs` + `ui/right_panel.rs` 搬过来的做法，
//! 两条规矩也一起搬了：
//!
//! 1. 字号一律用 rem，不用 px。写死 12px 意味着 `ui_font_size` 一改，
//!    侧栏不动、正文动了，两边立刻不是同一款界面。
//! 2. 冒出一个新尺寸之前，先看看台阶上有没有现成的。手写一个 `px(13.)`
//!    多半是笔误，或者说明这个数字本来就不该存在。

use gpui::{div, prelude::*, px, rems, App, Div, FontWeight, Hsla, Pixels, SharedString};
use gpui_component::{h_flex, v_flex, ActiveTheme as _};

/// 卡片/气泡圆角。
pub const CARD_RADIUS: Pixels = px(8.);

/// 行的左右内缩。列表减去它、行再加回来，于是 hover 底色比文字两侧各宽
/// 4px，而文字本身仍落在同一个缩进上。
pub const ROW_INSET: f32 = 4.;

/// 栏头高度。三栏都是这个高度，顶栏也是，横向扫过去才是一条线。
pub const HEADER_H: Pixels = px(40.);

// ---- 字号阶梯（rem，基准是主题的 font_size，通常 16px）----------------
pub const TEXT: f32 = 14. / 16.; // 正文
pub const META: f32 = 12. / 16.; // 时间、预览等次要信息
pub const HEADING: f32 = 11. / 16.; // 分组标题，刻意在 META 之下

/// 一栏的底：占满、可被压缩、着窗口底色。
pub fn column(cx: &App) -> Div {
    v_flex().size_full().min_w_0().bg(cx.theme().background)
}

/// 栏头：标题靠左，右侧留给调用方塞按钮。
///
/// `trailing` 用 Option 而不是"永远传一个"，是因为没有按钮时留一个空
/// `h_flex` 会把标题推到中心线左边一点点，三栏的标题就对不齐了。
pub fn header(cx: &App, title: impl Into<SharedString>, trailing: Option<Div>) -> Div {
    h_flex()
        .h(HEADER_H)
        .flex_shrink_0()
        .px(px(12.))
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_size(rems(HEADING))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().muted_foreground)
                .child(title.into()),
        )
        .when_some(trailing, |this, trailing| this.child(trailing))
}

/// 列表里的分组标题（"今天"、"更早"……）。
pub fn section_heading(cx: &App, label: impl Into<SharedString>) -> Div {
    div()
        .px(px(12. + ROW_INSET))
        .py(px(4.))
        .text_size(rems(HEADING))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(label.into())
}

/// 空状态占位。三栏共用，省得每栏各自发明一种"什么都没有"。
pub fn empty(cx: &App, text: impl Into<SharedString>) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .text_size(rems(META))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// 次要文字色，省得每处都写一遍 `cx.theme().muted_foreground`。
pub fn muted(cx: &App) -> Hsla {
    cx.theme().muted_foreground
}

/// 选中态底色。三栏的"当前项"用同一个颜色，一眼能看出谁被选中。
pub fn selected_bg(cx: &App) -> Hsla {
    cx.theme().list_active
}
