# Trialog

三栏式 AI 聊天桌面应用。GPU 渲染，纯 Rust，界面基于 Zed 的 [gpui](https://github.com/zed-industries/zed) 与 [gpui-component](https://github.com/l0ng-ai/gpui-component)。

```
┌──────────────┬──────────────┬─────────────────────────────┐
│  聊天历史    │   大纲        │   对话                       │
│              │              │                             │
│  今天        │  ▸ 讲讲 GP…  │  我：讲讲 GPUI 的渲染模型     │
│   ▸ GPUI 聊… │    ▸ 渲染管线 │                             │
│   ▸ Rust 问… │    ▸ 布局     │  助手：GPUI 把整个界面当作…   │
│  昨天        │  ▸ 再问一个  │                             │
│   ▸ 编译问题 │              │  ─────────────────────────   │
│              │              │  [输入区            ] [发送] │
└──────────────┴──────────────┴─────────────────────────────┘
       ↕ 可拖拽            ↕ 可拖拽
```

三栏分别是：**聊天历史 → 大纲 → 对话**。两道分隔条都能拖，宽度会记住。

## 与 tty7 的关系

界面**不是**自己写的。控件、布局系统、主题、图标全部取自
[tty7](https://github.com/l0ng-ai/tty7) 使用的那套 gpui 技术栈，并且沿用了它的做法：

| 借来的东西 | 出处 |
| --- | --- |
| 三栏可拖拽布局（`h_resizable` / `resizable_panel` / `ResizableState`） | `gpui-component::resizable` |
| 主题与配色（`cx.theme()`、`Theme::change`） | `gpui-component::theme` |
| 输入区（多行、自动长高、Enter 提交 / Shift+Enter 换行） | `gpui-component::input` |
| 消息正文的 Markdown 渲染 | `gpui_component::text::TextView::markdown` |
| 滚动条浮层（`with_vertical_scrollbar` 的写法） | tty7 `src/ui/scrollbar.rs` |
| 资源入口与 `stock/` 前缀转义 | tty7 `src/ui/assets.rs` |
| 字号用 rem 不用 px、栏头高度统一、分组标题在次要字号之下 | tty7 `src/ui/theme.rs`、`src/ui/right_panel.rs` |
| 历史列表按"今天/昨天/最近七天/更早"分桶 | tty7 `src/ui/home.rs` |

gpui / gpui-platform 的 git rev、gpui-component 的分支，都与 tty7 的 `Cargo.toml` 逐一对齐。
唯一有意的不同：**没有**开 `tree-sitter-languages` —— 聊天界面用不到语法高亮，
而它会把三十来个 C 语法拖进构建，Windows CI 上要多花十几分钟。

## 文件结构

| 文件 | 职责 |
| --- | --- |
| `Cargo.toml` | 依赖与构建配置。gpui 栈的 pin 与 tty7 一致 |
| `.github/workflows/build.yml` | Windows 上构建 `trialog.exe`，打包上传；打 tag 时发 Release |
| `src/main.rs` | 进程入口：初始化 gpui-component、开窗口、挂 `Root` |
| `src/assets.rs` | 静态资源入口（透传 gpui-component 资产包） |
| `src/theme.rs` | 间距/圆角/字号阶梯，三栏共用的栏头、分组标题、空状态 |
| `src/model.rs` | 领域模型：`Conversation` / `Message` / `Role` / `OutlineNode`，时间格式化 |
| `src/store.rs` | 落盘：`config.json` + `conversations.json`，原子写 |
| `src/ai.rs` | OpenAI 兼容接口的 SSE 流式客户端（ureq，跑在后台线程） |
| `src/markdown.rs` | 从会话内容抽大纲（识别标题、跳过代码块里的 `#`） |
| `src/ui/workspace.rs` | 主视图：状态、动作（发送/停止/增删会话/跳转/设置）、三栏装配 |
| `src/ui/history_column.rs` | 左栏：搜索 + 分桶的会话列表 |
| `src/ui/outline_column.rs` | 中栏：大纲树，点击跳转 |
| `src/ui/chat_column.rs` | 右栏：消息列表 + 输入区 |
| `src/ui/settings.rs` | 设置浮层 |

## 用起来

### 1. 配接口

第一次打开点右上角的齿轮，或者直接用默认值。四个字段：

| 字段 | 说明 |
| --- | --- |
| 接口地址 | OpenAI 兼容根地址，结尾不带斜杠。默认 `https://api.openai.com/v1` |
| API Key | `Bearer` 令牌。本地 Ollama 之类不需要的可以留空 |
| 模型 | 默认 `gpt-4o-mini` |
| 系统提示词 | 每轮请求都会带在前面 |

通一条地址对多家都成立，改地址和模型即可：

- DeepSeek：`https://api.deepseek.com/v1` + `deepseek-chat`
- 本地 Ollama：`http://localhost:11434/v1` + `qwen2.5:7b`（Key 留空）

### 2. 快捷键

| 键 | 作用 |
| --- | --- |
| `Enter` | 发送 |
| `Shift+Enter` | 换行 |
| 点大纲任意一行 | 对话区滚到那条消息并高亮 |

## 构建

本地（需要 Rust 1.85+，因为 gpui 依赖异步闭包）：

```bash
cargo build --release        # 产物在 target/release/trialog.exe
```

Windows 以外的平台要装系统依赖：Linux 需要 `pkg-config cmake clang libxkbcommon-dev libfontconfig1-dev libwayland-dev libx11-dev libxcb1-dev`（gpui 的 x11/wayland 后端要用）。

### 用 GitHub Actions 出 exe

仓库推上去就会跑 `.github/workflows/build.yml`：

1. `windows-latest` 上装 stable 工具链（镜像自带 DirectWrite / D3D 的 SDK，不需要额外系统依赖）
2. `cargo build --release --target x86_64-pc-windows-msvc`
3. 压成 `trialog-windows-x86_64.zip`，作为 artifact 上传
4. 如果这次推送的是 `v*` 标签，顺手发一个 Release 并挂上 zip

产物在 Actions 页面的 **Artifacts** 里，名字是 `trialog-windows-x86_64`。
冷构建二十分钟量级（gpui 依赖树大），之后有 `Swatinem/rust-cache` 缓存会快很多。

想直接拿到 Release：

```bash
git tag v0.1.0
git push origin v0.1.0
```

## 数据存在哪

`<配置目录>/trialog/`：

- Windows：`%APPDATA%\trialog\`
- macOS：`~/Library/Application Support/trialog/`
- Linux：`~/.config/trialog/`

两个文件：`config.json`（设置）、`conversations.json`（全部会话）。写盘先写 `.tmp` 再改名，
写一半断电不会留下半个 JSON。文件读坏了会降级成默认值并记日志，不会打不开应用。

## 已知的取舍

- **大纲是算出来的，不单独存。** 每次渲染从当前会话推导。存一份就会有和正文对不上的一天。
- **流式接收中的消息用纯文本画，收完才切 Markdown。** 每几个 token 就整篇重解析一遍太费，
  而且半个 `**` 会渲染成成片的星号。
- **时间按 UTC 显示。** 不引 chrono / tzdb 的前提下拿本地时区要碰 libc 的 `localtime_r`，
  会话列表里差几个小时不影响判断。
- **分隔条宽度只在鼠标松开时写盘**，拖动过程中不写。

## License

Apache-2.0
