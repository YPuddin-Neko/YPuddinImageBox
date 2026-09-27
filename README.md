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
cd src-tauri && cargo run --example download_probe # 用真实网络跑一遍下载队列（存到临时目录）
```

## 主题

颜色只写在 `src/theme/themes.json`，运行 `pnpm themes` 生成 `src/styles/themes.css`（`pnpm build` 会自动执行）。

## 图标

原图放在 `design/icon/`：`app-icon.png` 铺满画布，用于 Windows；`app-icon-macos.png` 按 macOS 图标规范四周留白并带投影。换图后重新生成：

```bash
pnpm tauri icon design/icon/app-icon.png -o /tmp/ibx-icons
pnpm tauri icon design/icon/app-icon-macos.png -o /tmp/ibx-icons-mac
```

把 `/tmp/ibx-icons` 中与 `src-tauri/icons/` 同名的文件复制过去（不需要 `android/`、`ios/`），`icon.icns` 改用 `/tmp/ibx-icons-mac` 里的。界面左上角的 `src/assets/app-icon.png` 是 128px 的缩小版。

## 账号与代理

- 账号在「设置 → 账号」里填写。保存前先访问一次站点验证；API Key 存在系统钥匙串（macOS 钥匙串 / Windows 凭据管理器），`settings.json` 里只记用户名。Danbooru 不填也能用，Gelbooru 必须填写。
- 代理在「设置 → 网络」里选：跟随系统、不使用代理或手动填写（http / https / socks5），保存后立即生效。
- 开发版每次重新编译后，macOS 可能询问是否允许读取钥匙串里的 API Key，选「始终允许」即可；签名后的正式版不会反复询问。
- `cd src-tauri && cargo test secrets -- --ignored` 会真实读写一次钥匙串，平时的 `cargo test` 不碰钥匙串。
