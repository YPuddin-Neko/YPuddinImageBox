# 开发备忘

安装、启动和构建见 [README](../README.md#开发)，界面设计稿和图标原图在 `design/`。

## 手动测试

命令都在 `src-tauri` 目录下运行。

```bash
cargo run --example probe                     # 用真实网络检查 Danbooru 的访问和 tag 额度
cargo run --example download_probe            # 用真实网络跑一遍下载队列，存到临时目录；后面加 yandere 或 pixiv 换站点
cargo run --release --example library_bench   # 造 10 万张的图库，测写入和查询耗时
```

要联网、会读写系统钥匙串或废纸篓、或者是测速度的测试，平时的 `cargo test` 会跳过，要加 `--ignored` 单独运行：

```bash
cargo test combined_search_on_real_sites -- --ignored   # 不带账号在所有平台上跑一遍聚合搜索
cargo test yandere -- --ignored                         # Yande.re 搜索和收藏
cargo test pixiv -- --ignored                           # 不登录搜 Pixiv
cargo test kemono -- --ignored                          # Kemono 搜索
cargo test secrets -- --ignored                         # 真实读写一次系统钥匙串
cargo test trash -- --ignored                           # 往废纸篓里放一个临时文件再清掉
cargo test --release --lib thumbs::tests::speed -- --ignored --nocapture   # 缩略图生成速度
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

## 钥匙串

开发版每次重新编译后，macOS 可能询问是否允许读取钥匙串里的 API Key，选「始终允许」即可。安装包只做了临时签名，装上新版本后也会再问一次。

## 日志

日志写在「软件数据」位置的 `logs/imagebox.log`，超过 2 MB 换新文件，保留上一份。「设置 → 通用」里能看到完整路径，也能打开所在的文件夹。
