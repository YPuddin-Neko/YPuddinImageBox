<div align="center">

<img src="design/icon/app-icon-macos.png" width="200" alt="YPuddinImageBox">

<h1>YPuddinImageBox</h1>

Danbooru、Pixiv、X 等平台的图片搜索、批量下载与本地图库管理工具

[![License](https://img.shields.io/badge/License-GPLv3-007ec6)](LICENSE)
[![构建](https://github.com/YPuddin-Neko/YPuddinImageBox/actions/workflows/build.yml/badge.svg)](https://github.com/YPuddin-Neko/YPuddinImageBox/actions/workflows/build.yml)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS-555)
![Rust](https://img.shields.io/badge/Rust-Tauri%202-e8743b)
![React](https://img.shields.io/badge/React-19-61dafb)

[功能](#功能) · [支持的平台](#支持的平台) · [开发](#开发)

</div>

> 仍在开发中，还没有正式版本。

## 功能

- **搜索**：按 tag、分级和排序搜索，常用的搜索条件可以保存下来。
- **聚合搜索**：几个平台一起搜，结果按同一种排序合成一列并标出来源，同一张图只显示一次。
- **下载**：多选下载原图，或按条件下载全部结果。后台队列可暂停、继续和重试，下载时校验 md5，已有的图自动跳过。
- **订阅**：定期检查搜索条件下的新图并自动下载，有新图时发系统通知；关闭窗口后在菜单栏（Windows 为托盘）继续运行。
- **收藏**：列出各平台账号的收藏并下载，包括 Pixiv 的书签、Kemono 收藏的作者，以及 X 的喜欢和书签。
- **X 媒体采集**：通过内置的 X 页面收集用户媒体页里的图片。
- **图库**：按来源分文件夹，再按画师、作品、角色或 tag 分组，可筛选和排序；也能导入本地图片。
- **看图**：查看原图，已下载的图直接读本地文件；可缩放、拖动和翻页。
- **账号、网络与存储**：API Key 存在系统钥匙串，或加密后存在设置文件里；代理可跟随系统或手动填写 http / https / socks5；图片、数据库和缓存的位置都可以改。
- **外观与语言**：8 套配色（6 深 2 浅，可跟随系统切换），简体中文和英文界面。

## 支持的平台

| 平台 | 账号 | 限制 |
|---|---|---|
| Danbooru | 可选 | 未登录和普通账号一次只能搜 2 个 tag，多出的条件在本地筛选 |
| Gelbooru | 需要 User ID 和 API Key | tag 不带分类 |
| Yande.re | 不需要，看收藏要填用户名 | tag 不带分类，分级只有三级 |
| e621 | 可选 | — |
| Rule34.xxx | 需要 User ID 和 API Key | tag 不带分类，不支持收藏 |
| Pixiv | 可选 | 未登录只能看全年龄作品，按 tag 最多翻 10 页；动图暂不下载 |
| Kemono | 可选，看收藏要登录 | 只下载图片，视频和压缩包等附件不下载 |
| X | 需要登录 | 视频暂不下载 |

各平台的 tag 写法不同（例如风景在 Danbooru 是 `scenery`，在 Yande.re 是 `landscape`；Pixiv 多用日文 tag），聚合搜索时同样的 tag 在有的平台上可能搜不到图。

## 开发

基于 Tauri 2 + React 19 + TypeScript + Rust，本地数据用 SQLite。

环境：Node.js 20 以上、pnpm、Rust stable；Windows 另需 Microsoft C++ Build Tools 和 WebView2（Windows 10 1803 以上已自带）。

```bash
pnpm install
pnpm tauri dev              # 启动开发版
pnpm build                  # 生成主题 CSS、类型检查并构建前端
cd src-tauri && cargo test  # Rust 测试
```

技术方案和进度见 [docs/architecture.md](docs/architecture.md)；联网测试、性能测试和主题、图标的生成方法见 [docs/development.md](docs/development.md)。

## 开源协议

本项目以 [GNU General Public License v3.0](LICENSE) 开源。
