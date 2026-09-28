<div align="center">

<img src="design/icon/app-icon-macos.png" width="200" alt="YPuddinImageBox">

<h1>YPuddinImageBox</h1>

一个 Danbooru / Gelbooru / Yande.re 图片下载与本地图库管理工具

[![License](https://img.shields.io/badge/License-GPLv3-007ec6)](LICENSE)
[![构建](https://github.com/YPuddin-Neko/YPuddinImageBox/actions/workflows/build.yml/badge.svg)](https://github.com/YPuddin-Neko/YPuddinImageBox/actions/workflows/build.yml)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS-555)
![Version](https://img.shields.io/github/package-json/v/YPuddin-Neko/YPuddinImageBox?label=Version&color=e8743b)
![Rust](https://img.shields.io/badge/Rust-Tauri%202-e8743b)
![React](https://img.shields.io/badge/React-19-61dafb)

[下载](#下载) · [功能介绍](#功能介绍) · [快速开始](#快速开始)

</div>

## 功能介绍

- **搜索**：按 tag 搜索 Danbooru、Gelbooru 和 Yande.re，分级可复选，可按上传时间、分数、收藏、热度、分辨率、文件大小排序；超出账号能搜的 tag 数时，多出来的条件在本地筛选。常用的搜索条件可以收藏，点一下就能重新搜。
- **聚合搜索**：来源可以勾选几个平台一起搜，结果按所选排序合成一列，每张图右上角标出来自哪个站点，筛选行可以按平台筛选；两个站点都有的同一张图只显示一次。
- **下载**：点选或多选下载原图，也可以按条件下载全部结果（可按当前排序只取前 N 张）。后台队列可暂停、继续、重试，下载时校验 md5，已有的图自动跳过。
- **图库**：首页按来源分文件夹（每个站点一个，另有自定义导入），点进去按画师、作品、角色或一般 tag 分组，每个文件夹和分组的卡片像一手扑克牌那样扇开最近下载的几张；组里按 tag、来源、分级筛选，按下载时间、发布时间、分数等排序；文件被移走时标出来并可以重新下载，删除的图进废纸篓（回收站）。
- **订阅**：搜索条件可以订阅，按设定的间隔检查新图并自动下载，有新图时发系统通知；关掉窗口后在菜单栏（Windows 为托盘）继续运行，也可以开机启动。
- **账号与网络**：API Key 存在系统钥匙串，或加密后存在设置文件里；代理支持跟随系统、直连或手动填写 http / https / socks5。
- **外观与存储**：8 套配色（6 深 2 浅），可跟随系统切换；图片、数据库、软件数据和缓存的位置都可以改。
- **语言**：简体中文和英文，默认跟随系统，也可以在「设置 → 通用」里切换，立即生效。
- **快捷键**：⌘ / Ctrl + F 跳到搜索框，方向键切换图片，空格勾选，⌘ / Ctrl + D 下载，完整列表在「设置 → 通用」里。

## 下载

每次推送到 GitHub 都会自动构建（[Actions → 构建](https://github.com/YPuddin-Neko/YPuddinImageBox/actions/workflows/build.yml)），打开最新一次成功的运行，在页面底部的 Artifacts 里下载：

- `YPuddinImageBox-macos-universal`：macOS 12 以上，Apple 芯片和 Intel 通用的 .dmg
- `YPuddinImageBox-windows-x64`：Windows 10 / 11 安装程序（.exe）

安装包没有做开发者签名：

- **macOS**：第一次打开会提示无法验证开发者。到「系统设置 → 隐私与安全性」底部点「仍要打开」，之后就能正常打开。
- **Windows**：SmartScreen 提示时点「更多信息 → 仍要运行」。

## 快速开始

1. 打开软件，在「发现」里输入 tag 搜索，例如 `scenery sky`。Danbooru 不登录也能用。
2. 点图片在右侧看详情，点「下载原图」；按住 ⌘（Windows 为 Ctrl）点图片可以多选，一起下载。
3. 想要某个条件下的全部图片，点「下载全部结果」；想以后自动收新图，点「订阅」。
4. 下载好的图在「图库」里，进度在「下载」里。Gelbooru 需要先在「设置 → 账号」里填写 User ID 和 API Key。

## 使用说明

### 聚合搜索

- 在搜索框左侧的来源里勾选两个以上平台（或点「全部平台」），同样的 tag、分级和排序会同时搜这几个站点。结果按所选排序合在一起，往下翻多少页都是这个顺序。排序只有所选站点都支持的几种（Danbooru 和 Gelbooru 一起搜时是最新上传、最早上传和分数最高）；按分数排时直接比各站点自己的分数。
- 筛选行的「平台」只在聚合搜索时能选，可以在勾选的平台里只看其中几个；只搜一个站点时显示为灰色。
- 同一张图（md5 相同）两个站点都有时只显示一次。从一个站点下载过的图，在另一个站点的结果里也标「已下载」。
- 「下载全部结果」和「订阅」给每个平台各建一个任务或订阅；多选的图来自两个站点时，也按站点分成两个下载任务。
- 一个站点出错（例如 Gelbooru 没填账号）时，另一个站点的结果照常显示，出错的站点单独提示。

### Yande.re

- 不用登录，一次能搜的 tag 数不限。它的 tag 写法和 Danbooru 不同，例如风景是 `landscape`（Danbooru 是 `scenery`），所以聚合搜索时同样的 tag 在它上面可能搜不到图。
- 分级只有三级：安全（相当于一般和敏感）、存疑、成人；只勾「一般」时也会搜到敏感的图。排序有最新上传、最早上传、分数最高和分辨率最高。
- 帖子里的 tag 不带分类，图库里和 Gelbooru 一样只能按一般 tag 分组。

### 超出 tag 上限

Danbooru 一次能搜的 tag 数有限（未登录 2 个，排序也占一个）。超出时前几个交给站点，其余在本地逐页筛选，发现页的筛选行会标出「本地筛选」；下载全部结果和订阅同样适用。

### 订阅与后台运行

- 在「发现」里搜索后点「订阅」，按设定的间隔（每小时到每天）检查新图并自动下载；订阅页可以暂停、改间隔、立即检查。订阅按上传先后找新图，不使用排序。
- 关闭窗口后默认在后台继续运行，从菜单栏（Windows 为托盘）图标重新打开或退出；「设置 → 通用」里可以改成关窗口即退出，也可以打开开机启动。

### 账号与代理

- 账号在「设置 → 账号」里填写，保存前先访问一次站点验证。Danbooru 不填也能用，Gelbooru 必须填写，Yande.re 不用账号。
- API Key 的保存方式由用户选：系统钥匙串（macOS 钥匙串 / Windows 凭据管理器，默认），或加密后存在 `settings.json` 里（XChaCha20-Poly1305，密钥由本机设备标识和随机盐派生，文件复制到别的电脑解不开）。切换时已保存的 Key 一起搬过去。
- 代理在「设置 → 网络」里选：跟随系统、不使用代理或手动填写（http / https / socks5），保存后立即生效。

### 界面语言

「设置 → 通用 → 语言」里选跟随系统、简体中文或 English，切换后立即生效，macOS 顶部的应用菜单、菜单栏图标（Windows 为托盘）的菜单、系统通知和错误提示一起换。跟随系统时，系统首选语言是中文就用中文，其他语言都用英文。已经记下的下载任务标题和跳过原因保持当时的语言。

### 日志

下载任务、订阅检查、网络退避和出错信息记在「软件数据」位置的 `logs/imagebox.log`，超过 2 MB 换新文件。「设置 → 通用」里可以直接打开所在的文件夹。

## 开发

技术方案与进度见 [docs/architecture.md](docs/architecture.md)，界面设计稿在 `design/`。

### 开发环境

- Node.js 20 以上、pnpm
- Rust stable
- Windows 另需 Microsoft C++ Build Tools 和 WebView2（Windows 10 1803 以上已自带 WebView2）

### 常用命令

```bash
pnpm install
pnpm tauri dev                            # 启动开发版
pnpm build                                # 生成主题 CSS、类型检查并构建前端
cd src-tauri && cargo test                # Rust 单元测试
cd src-tauri && cargo run --example probe # 用真实网络检查 Danbooru 访问与 tag 额度
cd src-tauri && cargo run --example download_probe # 用真实网络跑一遍下载队列（存到临时目录）
cd src-tauri && cargo run --release --example library_bench # 造 10 万张的图库，测写入和图库查询耗时
cd src-tauri && cargo test --release --lib thumbs::tests::speed -- --ignored --nocapture # 缩略图生成速度
```

### 主题

颜色只写在 `src/theme/themes.json`，运行 `pnpm themes` 生成 `src/styles/themes.css`（`pnpm build` 会自动执行）。

### 图标

原图放在 `design/icon/`：`app-icon.png` 铺满画布，用于 Windows；`app-icon-macos.png` 按 macOS 图标规范四周留白并带投影。换图后重新生成：

```bash
pnpm tauri icon design/icon/app-icon.png -o /tmp/ibx-icons
pnpm tauri icon design/icon/app-icon-macos.png -o /tmp/ibx-icons-mac
```

把 `/tmp/ibx-icons` 中与 `src-tauri/icons/` 同名的文件复制过去（不需要 `android/`、`ios/`），`icon.icns` 改用 `/tmp/ibx-icons-mac` 里的。界面左上角的 `src/assets/app-icon.png` 是 128px 的缩小版。

### 钥匙串与废纸篓

- 开发版每次重新编译后，macOS 可能询问是否允许读取钥匙串里的 API Key，选「始终允许」即可。安装包只做了临时签名，装上新版本后也会再问一次。
- `cd src-tauri && cargo test secrets -- --ignored` 会真实读写一次钥匙串，`cargo test trash -- --ignored` 会往废纸篓里放一个临时文件再清掉；平时的 `cargo test` 两样都不碰。

## 开源协议

本项目以 [GNU General Public License v3.0](LICENSE) 开源。
