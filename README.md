# ImageBox

按 tag、画师等条件从 Danbooru / Gelbooru 下载图片、管理本地图库的桌面软件，支持 Windows 和 macOS。

- 技术方案与进度：[docs/architecture.md](docs/architecture.md)
- 界面设计稿：`design/`（风格提案、夜幕画廊配色）

## 开发环境

- Node.js 20 以上、pnpm
- Rust stable
- Windows 另需 Microsoft C++ Build Tools 和 WebView2（Windows 10 1803 以上已自带 WebView2）

## 常用命令

```bash
pnpm install
pnpm tauri dev                            # 启动开发版
pnpm build                                # 生成主题 CSS、类型检查并构建前端
cd src-tauri && cargo test                # Rust 单元测试
cd src-tauri && cargo run --example probe # 用真实网络检查 Danbooru 访问与 tag 额度
```

## 主题

颜色只写在 `src/theme/themes.json`，运行 `pnpm themes` 生成 `src/styles/themes.css`（`pnpm build` 会自动执行）。

## 账号（临时方式）

账号设置页完成前，账号从环境变量读取：

```bash
export IMAGEBOX_DANBOORU_USERNAME=...
export IMAGEBOX_DANBOORU_API_KEY=...
export IMAGEBOX_GELBOORU_USER_ID=...
export IMAGEBOX_GELBOORU_API_KEY=...
```

Danbooru 不填也能匿名浏览；Gelbooru 必须填写。
