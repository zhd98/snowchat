//! Trialog —— 三栏式 AI 聊天桌面应用。
//!
//! 窗口自上而下只有两层：顶栏 + 一条 `h_resizable` 三栏。左栏聊天历史、
//! 中栏大纲、右栏对话。界面控件全部来自 gpui-component（与 tty7 同一分支），
//! 本项目只负责把数据接上去。
//!
//! 发布构建在 Windows 上挂 windows 子系统，双击不弹控制台。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod ai;
mod assets;
mod markdown;
mod model;
mod store;
mod theme;
mod ui;

use gpui::*;
use gpui_component::Root;
use ui::Workspace;

fn main() {
    // 图标、字体等静态资源走 gpui-component 自带的资产包。
    let app = gpui_platform::application().with_assets(assets::Assets);

    app.run(move |cx| {
        // 必须在使用任何 gpui-component 控件之前调用。
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Trialog".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(760.), px(500.))),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                // 窗口的第一层必须是 Root，Sheet / Dialog / 通知都挂在它上面。
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open the Trialog window");
        })
        .detach();
    });
}
