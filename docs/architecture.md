# ImageBox 技术方案

> 状态：M0 进行中（2026-09-27）
> 目标平台：Windows 10（1803 及以上）/ 11，macOS 12 及以上（Apple 芯片与 Intel）
> 界面：夜幕画廊，8 套内置配色（颜色源文件 `src/theme/themes.json`）

## 1. 结论

**Tauri 2 + React / TypeScript（界面）+ Rust（下载、图库、缩略图）**，本地数据用 SQLite。

## 2. 选型对比

| | Tauri 2（推荐） | Electron | Python 打包（PyInstaller + pywebview） |
|---|---|---|---|
| 安装包体积 | 约 10–20 MB | 约 100 MB 以上 | 约 60–120 MB |
| 常驻内存 | 低，适合挂在托盘里跑订阅 | 高 | 中 |
| 界面渲染 | Windows 用 WebView2（Chromium 内核），macOS 用系统 WKWebView（Safari 内核），两端都要测 | 两端同一个 Chromium，最一致 | 同 Tauri |
| 下载内核 | 用 Rust 重写训练器的 booru 模块 | 用 TypeScript 重写 | 直接复用训练器的 Python 代码 |
| 打包、签名、自动更新 | 官方打包器 + updater 插件 | electron-builder，成熟 | 最麻烦，Windows 上杀毒软件误报常见 |
| 主要代价 | 需要写 Rust | 体积和内存 | 分发体验差，要带整套 Python 运行时 |

选 Tauri 的理由：

1. 夜幕画廊本来就是网页技术画的（CSS 变量主题、毛玻璃、瀑布流），Tauri 可以原样实现，主题 CSS 直接复用。
2. 软件要常驻托盘跑订阅，还要管理几万张图的索引和缩略图，Rust 在内存占用和并发下载上更合适。
3. 训练器的下载内核规模不大（搜索、字段归一、令牌桶、退避、重试），用 Rust 重写成本可控。
4. 前端沿用训练器那套 React + TypeScript + Vite，写法和组件经验都能复用。

备选：如果更看重「全部用 TypeScript、两端渲染完全一致」，可以用 Electron，代价是安装包和内存大得多。不建议走 Python 打包路线。

## 3. 架构

```
┌─────────────── 前端（React + TypeScript） ───────────────┐
│ 发现 / 图库 / 订阅 / 队列 / 设置   主题：themes.css 变量   │
└──────────────┬──────────────────────────▲───────────────┘
      命令（invoke）                  事件（进度、日志、限速状态）
┌──────────────▼──────────────────────────┴───────────────┐
│                     Rust 核心（Tauri）                    │
│ sources  Danbooru / Gelbooru 适配器（统一接口：搜索、计数、帖子解析） │
│ net      HTTP 客户端（描述性 UA、代理）、双令牌桶、429/503 退避   │
│ jobs     下载队列（持久化、暂停/继续/取消、重启后续跑）、订阅调度 │
│ library  SQLite 索引、md5 去重、缩略图缓存、导出训练集          │
│ protocol 自定义图片协议：本地缩略图与远程预览统一经 Rust 加载   │
│ secrets  API Key 存系统钥匙串                                  │
└──────────────────────────────────────────────────────────┘
```

几个关键设计：

- **远程图片不让界面直接请求**。瀑布流里的远程预览图也经过 Rust 下载和缓存，这样代理、UA、限速和域名白名单只有一处；训练器的画廊代理就是这么做的。
- **队列持久化**。任务和进度写进 SQLite，软件重启后能接着下载。
- **API Key 进系统钥匙串**：Windows 凭据管理器、macOS 钥匙串，不写明文配置文件。
- **超出 tag 上限自动本地过滤**。Danbooru 免费账号一次只能搜 2 个 tag（Gold 6 个、Platinum 12 个），多出来的条件在拿到结果后本地筛。实测 `rating:`、`date:`、`score:` 不占额度，排除项（`-tag`）和 `order:` 占额度；`order:` 无法在本地等价实现，超额时只能提示。

## 4. 存储位置

四类目录都能在「设置 → 存储」里改：

| 位置 | 放什么 | 默认位置 | 修改后 |
|---|---|---|---|
| 图片 | 下载的原图 | 系统「图片」文件夹下的 ImageBox | 立即生效；可移动已有图片，或留在原处 |
| 数据库 | 图库索引、下载任务、订阅 | 软件数据目录下的 database（随软件数据一起移动） | 重启后生效；可移动，或改用新位置里已有的数据库 |
| 软件数据 | 设置、日志 | 系统应用数据目录下的 data | 重启后生效；可移动，或新位置从空开始 |
| 缓存 | 缩略图、预览图 | 系统缓存目录下的 image-cache | 立即生效；可移动，或清空旧缓存 |

规则：
- 位置设置本身写在系统配置目录的 `storage.json`，位置固定，软件靠它找到其余目录。
- 数据库、软件数据运行中一直被占用，修改时先记为待迁移，下次启动时在打开任何文件之前搬完；失败则保持原位置并在界面上提示。
- 新位置不能和其他任何一类位置重叠（默认数据库在软件数据目录里除外）；选择移动时新位置必须是空文件夹。
- 同一磁盘直接重命名；跨磁盘先复制并核对大小，再删除旧文件。
- 系统 WebView 自己的缓存不在管理范围内，默认缓存目录用了单独的子文件夹，移动或清空时不会碰到它。

## 5. 技术清单

**前端**
- React + TypeScript + Vite，包管理用 pnpm
- TanStack Query 处理命令调用的数据，Zustand 管界面状态
- 虚拟化瀑布流：按列计算位置，只渲染可视区域
- i18next（中文 / 英文）
- 样式：Tailwind CSS（和训练器一致），颜色全部映射到主题变量
- 字体：Manrope 随安装包内置，中文用系统字体（苹方 / 微软雅黑）
- 动效：Motion for React 负责组件进出场和视图切换；悬停、按下等微交互和图片加载扫光（Shimmer）用 CSS；换主题优先用 View Transitions API 整窗淡入，不支持时退回颜色渐变。时长 120–250ms，遵守系统「减弱动态效果」

**Rust**
- tokio（异步运行时）、reqwest（HTTP，rustls，带 SOCKS 代理支持）
- governor 或自写令牌桶（限速）
- sqlx（SQLite 与数据库迁移）
- image + fast_image_resize（缩略图）、md-5（去重）
- keyring（系统钥匙串）、tracing（日志）

**Tauri 插件**
- updater（自动更新）、autostart（开机启动）、single-instance（只开一个实例）
- window-state（记住窗口位置）、notification（订阅有新作时通知）、dialog、opener
- 托盘用 Tauri 内置的托盘功能

## 6. 数据模型（初稿）

- **posts**：来源、post_id（来源 + post_id 唯一）、md5（建索引）、原图地址、格式、宽高、分级、分数、收藏数、发布时间、文件大小、本地路径、下载时间
- **tags / post_tags**：tag 名称与分类（画师 / 作品 / 角色 / 一般 / 元）
- **subscriptions**：来源、查询条件、上次最大 post_id、检查间隔、是否启用、上次检查时间
- **jobs**：任务类型、参数、状态、进度，用于重启后续跑
- **saved_queries**：保存的查询

## 7. Windows 与 macOS 的差异

- **窗口标题栏**：macOS 用 Overlay 标题栏，红黄绿按钮嵌进左侧栏顶部；Windows 隐藏系统标题栏，右上角自绘最小化、最大化、关闭，顶部留拖拽区。
- **半透明**：Windows 11 可选云母效果，macOS 可选毛玻璃，默认关闭。
- **快捷键**：⌘ 和 Ctrl 自动对应。
- **默认目录与路径**：按平台区分（下载目录、缓存目录、配置目录）。
- **测试**：你在 Mac 上开发，Windows 版在 CI 上构建，另外需要一台 Windows 机器或虚拟机实测界面。

## 8. 打包与发布

- **Windows**：NSIS 安装包（.exe），内置 WebView2 引导安装。没有代码签名证书时，首次运行会弹 SmartScreen 提示，可以之后再买证书。
- **macOS**：.dmg，通用二进制（Apple 芯片 + Intel）。发给别人用需要 Apple 开发者账号（每年 99 美元）做签名和公证；只自己用可以先跳过。
- **自动更新**：tauri-plugin-updater，更新包单独签名，托管在 GitHub Releases。
- **CI**：GitHub Actions 分别在 Windows 和 macOS 机器上构建。

## 9. 里程碑

- **M0 技术验证**：两端跑通空壳；Rust 客户端带 UA 和代理访问 Danbooru / Gelbooru，确认能过 Cloudflare；自定义协议加载远程预览；主题切换。
- **M1 可用版本**：搜索与估算、下载队列（限速、退避、重试、取消）、SQLite 图库与 md5 去重、图库浏览、设置（账号、代理、存储、外观）。
- **M2**：订阅与增量下载、托盘常驻与新作通知、超出 tag 上限的本地过滤、导出训练集（图片 + .txt）。
- **M3**：安装包、签名、自动更新；接入 Yande.re、Konachan 等来源。

## 10. 要尽早验证的风险

- **Cloudflare**：训练器的经验是按 UA 放行，但 Rust 客户端的 TLS 指纹和 Python requests 不同，M0 先用真实账号验证。
- **两端渲染差异**：WKWebView 和 WebView2 在毛玻璃、字体、滚动上有差异，要在两个平台都测。
- **图库规模**：到十万张时的缩略图生成速度和瀑布流性能，M1 用大批量数据压测。

## 11. 进度

**M0 已完成**（macOS 上验证）：
- Tauri 2 工程：React 19 + TypeScript + Vite，窗口用 Overlay 标题栏，生产环境 CSP 只放行自身资源和 `ibx:` 图片协议。
- Rust：Danbooru / Gelbooru 适配器、分通道限速与 429/503 退避、`ibx://` 图片协议（域名白名单、按内容判断格式、磁盘缓存 14 天）。
- 前端：夜幕画廊外框、发现页（搜索、分级筛选、瀑布流、滚动加载、详情面板）、设置 → 外观（8 套主题、跟随系统）、动效与 Shimmer。
- 实测：Rust 客户端（rustls）带描述性 UA 可以正常访问 Danbooru，浏览器 UA 会被 Cloudflare 拦下；Gelbooru 不带 API Key 返回 401。

**M0 待做**：Windows 上实测（WebView2 渲染、自绘标题栏按钮）；账号暂从环境变量读取，M1 改为设置页 + 系统钥匙串。

**M1 进行中**：
- 已完成：存储位置设置（四类目录、迁移、重启后生效、目录重叠和非空检查）。
- 接下来：SQLite 图库与 md5 去重、下载队列、图库浏览、账号与代理设置。

## 参考

- Tauri 发布记录：https://v2.tauri.app/release/
- Tauri 环境要求：https://v2.tauri.app/start/prerequisites/
