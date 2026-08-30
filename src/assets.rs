//! 静态资源入口。
//!
//! 直接透传给 gpui-component 的资产包，另外保留 tty7 的 `stock/` 前缀转义：
//! 写 `stock/icons/x.svg` 能绕开我们自己的覆盖拿到上游原图。这个前缀本身
//! 目前没人用，留着是因为一旦哪天要覆盖某个图标，没有它就没法再拿回原图。

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub struct Assets;

const STOCK_PREFIX: &str = "stock/";

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(downstream) = path.strip_prefix(STOCK_PREFIX) {
            return gpui_component_assets::Assets.load(downstream);
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}
