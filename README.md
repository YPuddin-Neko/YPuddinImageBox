<div align="center">

<img src="design/icon/app-icon-macos.png" width="200" alt="YPuddinImageBox">

<h1>YPuddinImageBox</h1>

一个 Danbooru / Gelbooru / e621 / Rule34.xxx / Kemono / Yande.re / Pixiv 图片下载与本地图库管理工具，支持从 X 用户媒体页采集图片

[![License](https://img.shields.io/badge/License-GPLv3-007ec6)](LICENSE)
[![构建](https://github.com/YPuddin-Neko/YPuddinImageBox/actions/workflows/build.yml/badge.svg)](https://github.com/YPuddin-Neko/YPuddinImageBox/actions/workflows/build.yml)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS-555)
![Version](https://img.shields.io/github/package-json/v/YPuddin-Neko/YPuddinImageBox?label=Version&color=e8743b)
![Rust](https://img.shields.io/badge/Rust-Tauri%202-e8743b)
![React](https://img.shields.io/badge/React-19-61dafb)

[下载](#下载) · [功能介绍](#功能介绍) · [快速开始](#快速开始)

</div>

## 功能介绍

- **搜索**：按 tag 搜索 Danbooru、Gelbooru、e621、Rule34.xxx、Kemono、Yande.re 和 Pixiv（Pixiv 还能看画师的全部作品），分级可复选，可按上传时间、分数、收藏、热度、分辨率、文件大小排序；超出账号能搜的 tag 数时，多出来的条件在本地筛选。常用的搜索条件可以收藏，点一下就能重新搜。搜索框里每个 tag 是一个胶囊，点 × 删除、双击修改，输入时按空格收成胶囊；右侧详情里的 tag 点一下就加进搜索框（只填入，不搜索），图库组内的筛选框也一样。
- **X 媒体采集**：输入 X 用户名，在独立的 X 窗口里登录并打开 Media 页面，软件收集页面加载到的图片后交给下载队列；图库里单独归到 X 文件夹。视频暂不加入图库。
- **收藏**：侧栏「收藏」按平台列出你在 Danbooru、Gelbooru、Yande.re、e621、Pixiv、Kemono 上的收藏（Pixiv 分公开和非公开，Kemono 还能列出收藏的作者），以及 X 上自己的喜欢和书签；可以多选下载，也可以一键下载全部收藏。
- **看图**：鼠标移到卡片上，右下角的放大镜打开查看器；图片从卡片的位置放大展开，先显示缩略图和站点的缩小图，原图加载完后换上，缩放比例按原图算。已经下载过的图直接读本地文件，不再从站点下载（图库里也一样）。可以滚轮缩放（以指针为中心）、拖动、左右翻页。站点没开放原图、原图是视频或加载失败时，底部会说明，显示的是缩小图。
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
4. 下载好的图在「图库」里，进度在「下载」里。Gelbooru 和 Rule34.xxx 需要先在「设置 → 账号」里填写 User ID 和 API Key；e621 可以匿名使用，也可以填写账号提高访问权限。
5. 要下载 X 用户的图片，打开侧栏「X 媒体采集」，输入用户名并开始采集；在弹出的 X 窗口里登录后，页面加载到的图片会出现在软件里，勾选后加入下载队列。

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

### e621、Rule34.xxx

- e621 使用公开的 Danbooru 风格接口，不登录也能搜索；登录信息在「设置 → 账号」里填写用户名和 API Key。e621 的 safe / questionable / explicit 会映射到软件里的分级，species、lore 等站点分类保存在元数据 tag 中。
- Rule34.xxx 使用官方 DAPI 接口，需要在「设置 → 账号」里填写 User ID 和 API Key；没有凭据时站点接口会拒绝请求。站点设置页给的是一整串 `&api_key=…&user_id=…`，整串粘贴到任意一栏即可（Gelbooru 也一样）。图片 tag 按一般 tag 保存，排序支持最新、最早和分数。

### Kemono

- Kemono 使用官方 API v1。输入普通关键词时搜索全站帖子；按作者搜索时写 `creator:服务/作者 ID`，例如 `creator:patreon/123456`，也可以在后面加 `tag:关键词`。
- 帖子里的图片文件和图片附件会分别出现在结果里，编号是「帖子 id p第几张」，视频、压缩包和其他文件不会加入图片下载队列。Kemono 的作者标识会作为画师 tag 保存，图库里可以按画师分组。
- 预览用 img.kemono.cr 的缩略图（长边不超过 800），查看器里再加载原图。原图在 n1～n4 数据节点上，有的网络连不上，这时查看器显示缩略图并提示原图加载失败，下载也会失败。接口不给图片尺寸，瀑布流里先按 4:5 占位，下载后按文件补上真实尺寸。
- 不登录也能搜索和下载；在「设置 → 账号」里登录 Kemono 后，「收藏」里能看收藏的帖子和作者。

### 收藏

- 侧栏「收藏」（发现下面）左上角选平台，列出这个账号的收藏，按收藏先后排；分级筛选、多选下载、下载全部收藏都和发现页一样。
- 各平台要的账号：Danbooru 填用户名（填了 API Key 还能看私密收藏），Gelbooru 填 User ID 和 API Key，e621 填用户名和 API Key，Pixiv、Kemono 要登录，Yande.re 只填用户名（Yande.re 的收藏是公开的）。没填时页面里有去账号设置的按钮。
- Kemono 的「作者」列出收藏的作者，点开看这位作者的全部帖子。
- X 在收藏页里打开采集窗口：喜欢要填自己的用户名（X 从 2024 年起只能看自己的喜欢），书签不用填；窗口里登录 X 后，页面滚动时收集图片。
- Rule34.xxx 的收藏只有网页、没有接口，暂不支持。

### Pixiv

- 不登录也能搜全年龄作品、下载原图，但按 tag 搜最多翻 10 页；看 R-18 作品要登录。登录在「设置 → 账号」里点「登录 Pixiv」，在弹出的 Pixiv 页面里登录，密码只交给 Pixiv，软件从这个窗口里取出登录状态。登录页打不开时（例如用 Google 账号登录），也可以在浏览器里登录后，粘贴 Cookie 里的 PHPSESSID。
- 搜索框里除了 tag（`-tag` 表示排除，和网页上一样），还可以输入 `user:画师 ID` 看这位画师的全部作品（后面再加 tag 就在这些作品里筛选），或者直接粘贴画师主页、作品的链接。
- 一个作品一张卡片，多页的作品在右上角标出页数，下载时每一页都下载。文件名和原图一样是「作品 id_p页码」，放在「Pixiv / 画师名」文件夹里；有一页没下载成功就算这个作品失败，重试时只补缺的页。动图（ugoira）暂不下载。
- 排序只有最新上传和最早上传；列表里没有分数和收藏数。tag 大多是日文，和 Danbooru 的英文 tag 不通用，聚合搜索时同样的 tag 在 Pixiv 上可能搜不到。
- 为了不被站点限制，访问 Pixiv 接口每秒最多一次，翻页比其他站点慢一些。

### X 媒体采集

- 这项功能通过独立的 X 浏览器窗口读取用户的 Media 页面，不把 X 的登录 Cookie 交给普通 HTTP 下载接口。输入用户名后，窗口会自动滚动并收集页面加载到的图片，也可以手动继续滚动。
- 在 ImageBox 里勾选要下载的图片后加入队列，文件放在「X / 用户名」目录下。推文 hashtag 会作为一般 tag，用户名会作为画师。
- X 不登录看不到用户的媒体页，要先在采集窗口里登录（受保护的账号还要有关注权限）。可以用账号密码，也可以用 Google、Apple 登录；Google 有时不允许在内嵌窗口里登录，这时改用账号密码。X 页面结构或登录限制变化时，采集可能需要随版本更新。

### 超出 tag 上限

Danbooru 一次能搜的 tag 数有限（未登录 2 个，排序也占一个）。超出时前几个交给站点，其余在本地逐页筛选，发现页的筛选行会标出「本地筛选」；下载全部结果和订阅同样适用。

### 订阅与后台运行

- 在「发现」里搜索后点「订阅」，按设定的间隔（每小时到每天）检查新图并自动下载；订阅页可以暂停、改间隔、立即检查。订阅按上传先后找新图，不使用排序。
- 关闭窗口后默认在后台继续运行，从菜单栏（Windows 为托盘）图标重新打开或退出；「设置 → 通用」里可以改成关窗口即退出，也可以打开开机启动。

### 账号与代理

- 账号在「设置 → 账号」里填写，保存前先访问一次站点验证。Danbooru、e621 不填也能用，Gelbooru、Rule34.xxx 必须填写，Pixiv 不登录也能用（R-18 作品要登录，见上面的 Pixiv 一节）；Kemono 不登录也能搜，登录后能看收藏；Yande.re 不用账号，只为收藏页填一个用户名。Pixiv、Kemono 可以在软件的登录窗口里登录，也可以在浏览器里登录后粘贴 Cookie（PHPSESSID / session）。
- API Key 的保存方式由用户选：系统钥匙串（macOS 钥匙串 / Windows 凭据管理器，默认），或加密后存在 `settings.json` 里（XChaCha20-Poly1305，密钥由本机设备标识和随机盐派生，文件复制到别的电脑解不开）。切换时已保存的 Key 一起搬过去。Pixiv 的登录状态和 API Key 存在同一个地方。
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
