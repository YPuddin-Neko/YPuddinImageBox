//! 各 booru 站点的适配器：把不同站点的帖子统一成 [`Post`]。

pub mod combined;
pub mod danbooru;
pub mod e621;
pub mod filter;
pub mod fanbox;
pub mod gelbooru;
pub mod kemono;
pub mod moebooru;
pub mod pixiv;
pub mod rule34;
pub mod timestamp;
pub mod x;

use std::sync::{PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::AppError;
use crate::i18n::tr;
use crate::net::Net;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Danbooru,
    Gelbooru,
    E621,
    Rule34,
    Kemono,
    Yandere,
    Pixiv,
    Fanbox,
    X,
    Custom,
}

impl Source {
    pub const ALL: [Source; 10] = [
        Source::Danbooru,
        Source::Gelbooru,
        Source::E621,
        Source::Rule34,
        Source::Kemono,
        Source::Yandere,
        Source::Pixiv,
        Source::Fanbox,
        Source::X,
        Source::Custom,
    ];
    pub const REMOTE: [Source; 8] = [
        Source::Danbooru,
        Source::Gelbooru,
        Source::E621,
        Source::Rule34,
        Source::Kemono,
        Source::Yandere,
        Source::Pixiv,
        Source::Fanbox,
    ];

    /// 数据库、文件夹名和图片路由里用的小写名称。
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Danbooru => "danbooru",
            Source::Gelbooru => "gelbooru",
            Source::E621 => "e621",
            Source::Rule34 => "rule34",
            Source::Kemono => "kemono",
            Source::Yandere => "yandere",
            Source::Pixiv => "pixiv",
            Source::Fanbox => "fanbox",
            Source::X => "x",
            Source::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|source| source.as_str() == value)
    }

    pub fn site_name(self) -> &'static str {
        match self {
            Source::Danbooru => "Danbooru",
            Source::Gelbooru => "Gelbooru",
            Source::E621 => "e621",
            Source::Rule34 => "Rule34.xxx",
            Source::Kemono => "Kemono",
            Source::Yandere => "Yande.re",
            Source::Pixiv => "Pixiv",
            Source::Fanbox => "FANBOX",
            Source::X => "X",
            Source::Custom => "自定义导入",
        }
    }

    /// 允许加载图片的域名（含子域名）。预览代理只放行这些域名。
    pub fn allowed_host_suffixes(self) -> &'static [&'static str] {
        match self {
            Source::Danbooru => &["donmai.us"],
            Source::Gelbooru => &["gelbooru.com"],
            Source::E621 => &["e621.net"],
            Source::Rule34 => &["rule34.xxx"],
            Source::Kemono => &["kemono.cr"],
            Source::Yandere => &["yande.re"],
            // 网页和接口在 pixiv.net，图片在 i.pximg.net。
            Source::Pixiv => &["pixiv.net", "pximg.net"],
            Source::Fanbox => &["downloads.fanbox.cc"],
            Source::X => &["x.com", "twitter.com", "twimg.com"],
            Source::Custom => &[],
        }
    }

    pub fn referer(self) -> &'static str {
        match self {
            Source::Danbooru => "https://danbooru.donmai.us/",
            Source::Gelbooru => "https://gelbooru.com/",
            Source::E621 => "https://e621.net/",
            Source::Rule34 => "https://rule34.xxx/",
            Source::Kemono => "https://kemono.cr/",
            Source::Yandere => "https://yande.re/",
            Source::Pixiv => pixiv::REFERER_URL,
            Source::Fanbox => "https://www.fanbox.cc/",
            Source::X => "https://x.com/",
            Source::Custom => "file://",
        }
    }

    /// 保存的账号有没有 Key。Yande.re 只存用户名（看收藏用）。
    pub fn has_key(self) -> bool {
        self != Source::Yandere
    }

    pub fn for_host(host: &str) -> Option<Source> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        Source::ALL.into_iter().find(|source| {
            source
                .allowed_host_suffixes()
                .iter()
                .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
        })
    }

    /// 下载任务每页取多少条：取站点允许的最大值，减少翻页请求。
    pub fn max_page_size(self) -> u32 {
        match self {
            Source::Danbooru => 200,
            Source::Gelbooru | Source::Yandere => 100,
            Source::E621 => 320,
            Source::Rule34 => 100,
            Source::Kemono => 50,
            Source::Pixiv => pixiv::PAGE_SIZE,
            Source::Fanbox => fanbox::PAGE_SIZE,
            Source::X => 40,
            Source::Custom => 40,
        }
    }
}

/// 几个站点存进数据库时写成一个字符串：按固定顺序、逗号分隔，例如 `danbooru,gelbooru`。
pub fn join_sources(sources: &[Source]) -> String {
    Source::REMOTE.iter().filter(|source| sources.contains(source)).map(|source| source.as_str()).collect::<Vec<_>>().join(",")
}

/// 读回 [`join_sources`] 写的字符串，按固定顺序；认不出的站点跳过。
pub fn split_sources(value: &str) -> Vec<Source> {
    let parsed: Vec<Source> = value.split(',').filter_map(Source::parse).collect();
    Source::REMOTE.into_iter().filter(|source| parsed.contains(source)).collect()
}

/// 地址属于哪个已接入站点。只认 http(s)、默认端口、不带账号信息，域名按点边界匹配。
pub fn source_for_url(url: &Url) -> Option<Source> {
    if !matches!(url.scheme(), "https" | "http") || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    if url.port().is_some() {
        return None;
    }
    Source::for_host(url.host_str()?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    General,
    Sensitive,
    Questionable,
    Explicit,
}

impl Rating {
    pub const ALL: [Rating; 4] = [Rating::General, Rating::Sensitive, Rating::Questionable, Rating::Explicit];

    /// Danbooru 用单字母（g/s/q/e），Gelbooru 用全称；旧数据里的 safe 按一般处理。
    pub fn parse(value: &str) -> Option<Rating> {
        match value.trim().to_ascii_lowercase().as_str() {
            "g" | "general" | "safe" => Some(Rating::General),
            "s" | "sensitive" => Some(Rating::Sensitive),
            "q" | "questionable" => Some(Rating::Questionable),
            "e" | "explicit" => Some(Rating::Explicit),
            _ => None,
        }
    }

    fn danbooru_code(self) -> &'static str {
        match self {
            Rating::General => "g",
            Rating::Sensitive => "s",
            Rating::Questionable => "q",
            Rating::Explicit => "e",
        }
    }

    /// 全称，也是数据库里存的值。
    pub fn as_str(self) -> &'static str {
        match self {
            Rating::General => "general",
            Rating::Sensitive => "sensitive",
            Rating::Questionable => "questionable",
            Rating::Explicit => "explicit",
        }
    }
}

/// 搜索结果的排序。默认按上传先后（新到旧），其余换成站点的排序条件加进查询；
/// 带了排序条件就不能按 id 翻页，改用页码（见 [`Page::next`]）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Newest,
    Oldest,
    Score,
    Favorites,
    /// Danbooru 的 order:rank：按分数和新近程度综合排序，只包含最近两天左右上传的帖子。
    Popular,
    Resolution,
    Filesize,
}

impl Sort {
    const ALL: [Sort; 7] =
        [Sort::Newest, Sort::Oldest, Sort::Score, Sort::Favorites, Sort::Popular, Sort::Resolution, Sort::Filesize];

    /// 数据库里存的名称，和界面上用的一致。
    pub fn as_str(self) -> &'static str {
        match self {
            Sort::Newest => "newest",
            Sort::Oldest => "oldest",
            Sort::Score => "score",
            Sort::Favorites => "favorites",
            Sort::Popular => "popular",
            Sort::Resolution => "resolution",
            Sort::Filesize => "filesize",
        }
    }

    pub fn parse(value: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|sort| sort.as_str() == value)
    }

    /// 加进查询的排序条件，默认顺序为 `None`。只有 Danbooru 能按收藏、热度和文件大小排序，分辨率还有 Yande.re 能排。
    pub fn term(self, source: Source) -> Result<Option<&'static str>, AppError> {
        let term = match (source, self) {
            (_, Sort::Newest) => return Ok(None),
            (Source::Danbooru, Sort::Oldest) => "order:id",
            (Source::Danbooru, Sort::Score) => "order:score",
            (Source::Danbooru, Sort::Favorites) => "order:favcount",
            (Source::Danbooru, Sort::Popular) => "order:rank",
            (Source::Danbooru, Sort::Resolution) => "order:mpixels",
            (Source::Danbooru, Sort::Filesize) => "order:filesize",
            (Source::E621, Sort::Oldest) => "order:id",
            (Source::E621, Sort::Score) => "order:score",
            (Source::E621, Sort::Favorites) => "order:favcount",
            (Source::E621, Sort::Popular) => "order:rank",
            (Source::E621, Sort::Resolution) => "order:mpixels",
            (Source::E621, Sort::Filesize) => "order:filesize",
            (Source::Gelbooru, Sort::Oldest) => "sort:id:asc",
            (Source::Gelbooru, Sort::Score) => "sort:score:desc",
            (Source::Rule34, Sort::Oldest) => "sort:id:asc",
            (Source::Rule34, Sort::Score) => "sort:score:desc",
            (Source::Yandere, Sort::Oldest) => "order:id",
            (Source::Yandere, Sort::Score) => "order:score",
            (Source::Yandere, Sort::Resolution) => "order:mpixels",
            // Pixiv 的适配器自己认这个条件（换成 order=date），不是站点的语法。
            (Source::Pixiv, Sort::Oldest) => "order:date",
            (Source::Kemono | Source::Fanbox, _) => {
                let site = source.site_name();
                return Err(AppError::InvalidInput(tr!("{site} 只支持按最新上传排序", "{site} only supports sorting by newest")));
            }
            (Source::X, _) => {
                let site = source.site_name();
                return Err(AppError::InvalidInput(tr!("{site} 通过媒体采集窗口使用", "Use {site} through the media capture window")));
            }
            (Source::Custom, _) => {
                return Err(AppError::InvalidInput(tr!("自定义导入不能用于站点搜索", "Custom imports can't be used for site searches")));
            }
            (Source::Gelbooru | Source::Rule34 | Source::Yandere | Source::Pixiv, _) => {
                let site = source.site_name();
                return Err(AppError::InvalidInput(tr!("{site} 不支持这种排序", "{site} doesn't support this sort order")));
            }
        };
        Ok(Some(term))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostTags {
    pub artist: Vec<String>,
    pub copyright: Vec<String>,
    pub character: Vec<String>,
    pub general: Vec<String>,
    pub meta: Vec<String>,
}

impl PostTags {
    /// 按分类列出，分类名和数据库里的一致。
    pub fn by_category(&self) -> [(&'static str, &[String]); 5] {
        [
            ("artist", &self.artist),
            ("copyright", &self.copyright),
            ("character", &self.character),
            ("general", &self.general),
            ("meta", &self.meta),
        ]
    }

    pub fn push(&mut self, category: &str, name: String) {
        match category {
            "artist" => self.artist.push(name),
            "copyright" => self.copyright.push(name),
            "character" => self.character.push(name),
            "meta" => self.meta.push(name),
            _ => self.general.push(name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub source: Source,
    #[serde(with = "crate::post_id")]
    pub id: u64,
    pub md5: Option<String>,
    pub width: u32,
    pub height: u32,
    pub rating: Option<Rating>,
    pub score: i64,
    pub fav_count: Option<i64>,
    pub file_ext: String,
    pub file_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 原图。站点不对当前账号开放原图时为空，见 [`Post::gold_only`]。
    pub file_url: Option<String>,
    /// 详情面板用的中等尺寸图。
    pub sample_url: Option<String>,
    /// 瀑布流缩略图。
    pub thumb_url: Option<String>,
    pub created_at: Option<String>,
    pub post_url: String,
    pub tags: PostTags,
    /// 一个作品里有几页（Pixiv 的多页作品），只有一页或别的站点时为空。下载时每页各存一张。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
}

/// Danbooru 的受限 tag：带这些 tag 的帖子只对 Gold 及以上等级开放原图，普通账号和未登录都拿不到原图地址。
const GOLD_ONLY_TAGS: [&str; 3] = ["loli", "shota", "toddlercon"];

impl Post {
    /// 界面上显示的编号。Pixiv 显示作品 id，第二页起再写页码。
    pub fn label(&self) -> String {
        match self.source {
            Source::Pixiv => match pixiv::split_id(self.id) {
                (illust, 0) => format!("#{illust}"),
                (illust, page) => format!("#{illust} p{}", page + 1),
            },
            Source::Kemono => match kemono::split_id(self.id) {
                (post, 0) => format!("#{post}"),
                (post, index) => format!("#{post} p{}", index + 1),
            },
            Source::Fanbox if fanbox::is_cover(self) => {
                let post = fanbox::post_id(self);
                crate::i18n::tr!("#{post} 封面", "#{post} Cover")
            }
            Source::Fanbox => {
                let post = fanbox::post_id(self);
                let index = fanbox::display_index(self);
                if index <= 1 { format!("#{post}") } else { format!("#{post} p{index}") }
            },
            Source::X => format!("#{}", x::label_id(self.id)),
            _ => format!("#{}", self.id),
        }
    }

    /// 没有原图地址是不是因为账号等级不够。其余情况（画师被封禁、图片下架）连 Gold 也拿不到。
    pub fn gold_only(&self) -> bool {
        self.source == Source::Danbooru && self.tags.general.iter().any(|tag| GOLD_ONLY_TAGS.contains(&tag.as_str()))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchParams {
    pub source: Source,
    #[serde(default)]
    pub pixiv_input: bool,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub ratings: Vec<Rating>,
    /// 订阅按上传先后找新图，不看排序。
    #[serde(default)]
    pub sort: Sort,
    /// 上一次返回的 `next`；为空表示第一页。
    #[serde(default)]
    pub cursor: Option<String>,
}

impl SearchParams {
    pub fn normalized_tags(&self) -> String {
        if self.source == Source::Pixiv && self.pixiv_input { pixiv::normalize_input(&self.tags) } else { self.tags.clone() }
    }

    /// 用户输入的 tag 加上所选排序的条件。选了排序时以选项为准，输入框里手写的 order: / sort: 不再发给站点。
    pub fn tags_with_sort(&self) -> Result<String, AppError> {
        let tags = self.normalized_tags();
        let Some(term) = self.sort.term(self.source)? else { return Ok(tags) };
        let mut words: Vec<&str> = tags.split_whitespace().filter(|tag| !is_sort_tag(tag)).collect();
        words.push(term);
        Ok(words.join(" "))
    }

    /// 统计张数用的 tag。排序不影响张数，一般不带（Danbooru 带 order:score 这类条件时不给数字）；
    /// 近期热门同时限定了时间范围，要带上。
    pub fn tags_for_count(&self) -> Result<String, AppError> {
        if self.sort == Sort::Popular {
            self.tags_with_sort()
        } else {
            Ok(self.normalized_tags())
        }
    }
}

/// 明确指定排序的条件（order:、sort:）。
fn is_sort_tag(tag: &str) -> bool {
    let tag = tag.to_ascii_lowercase();
    tag.starts_with("order:") || tag.starts_with("sort:")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub posts: Vec<Post>,
    /// 下一页的位置，没有更多时为空。
    pub next: Option<String>,
    /// 实际发给站点的查询串，界面上展示给用户核对。
    pub query: String,
    /// 超出 tag 上限、在本地筛选的 tag（空格分隔，-tag 表示排除），没有时为空字符串。
    pub local_filter: String,
    /// 这一页里已在图库中的帖子 id。
    #[serde(with = "crate::post_id::vec")]
    pub owned: Vec<u64>,
    pub creators: Vec<pixiv::PixivCreator>,
    pub creator_error: Option<AppError>,
    pub artwork_error: Option<AppError>,
}

/// 翻页参数。Danbooru 按默认顺序（新到旧）时用「id 小于某值」翻页：
/// 翻页期间有新图上传也不会重复或漏掉，也不受 1000 页的上限限制。
/// 订阅找新图时反过来用「id 大于某值」，从上次处理到的地方往新的方向走。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Number(u32),
    Before(u64),
    After(u64),
}

impl Page {
    pub fn to_param(&self) -> String {
        match self {
            Page::Number(n) => n.to_string(),
            Page::Before(id) => format!("b{id}"),
            Page::After(id) => format!("a{id}"),
        }
    }

    pub fn parse(value: &str) -> Option<Page> {
        if let Some(id) = value.strip_prefix('b') {
            return id.parse().ok().map(Page::Before);
        }
        if let Some(id) = value.strip_prefix('a') {
            return id.parse().ok().map(Page::After);
        }
        value.parse().ok().filter(|n| *n >= 1).map(Page::Number)
    }

    /// 取完这一页后的下一页。`bounds` 是这一页里帖子 id 的（最小值，最大值）。
    pub fn next(&self, source: Source, query: &str, bounds: Option<(u64, u64)>) -> Page {
        let by_id = matches!(source, Source::Danbooru | Source::E621 | Source::Yandere) && !has_custom_order(query);
        match (self, bounds) {
            (Page::After(_), Some((_, max))) => Page::After(max),
            (Page::Number(_) | Page::Before(_), Some((min, _))) if by_id => Page::Before(min),
            (Page::Number(n), _) => Page::Number(n + 1),
            // 没拿到帖子：调用方此时已判定翻完，原样返回。
            (page, _) => page.clone(),
        }
    }
}

/// 查询里指定了排序（order:score、随机、Gelbooru 的 sort: 等）时不能按 id 翻页，也不能订阅。
/// 收藏页的 `favorites:`（e621、Kemono）和 `bookmarks:`（Pixiv）按收藏时间排，同样只能按页码翻。
pub fn has_custom_order(query: &str) -> bool {
    query.split_whitespace().any(|tag| {
        let tag = tag.to_ascii_lowercase();
        ["order:", "ordfav:", "ordpool:", "random:", "sort:", "favorites:", "bookmarks:"]
            .iter()
            .any(|prefix| tag.starts_with(prefix))
    })
}

/// 各站点的账号。Danbooru 可以不填；Gelbooru 的接口必须带账号。
#[derive(Debug, Clone, Default)]
pub struct Accounts {
    pub danbooru: Option<danbooru::Credentials>,
    pub gelbooru: Option<gelbooru::Credentials>,
    pub e621: Option<e621::Credentials>,
    pub rule34: Option<rule34::Credentials>,
    /// 不登录也能用，登录后才能看 R-18 作品。
    pub pixiv: Option<pixiv::Credentials>,
    /// 不登录也能搜；登录后才能看自己的收藏。
    pub kemono: Option<kemono::Credentials>,
    pub fanbox: Option<fanbox::Credentials>,
}

impl Accounts {
    fn gelbooru(&self) -> Result<&gelbooru::Credentials, AppError> {
        self.gelbooru.as_ref().ok_or(AppError::CredentialsMissing("Gelbooru"))
    }

    fn rule34(&self) -> Result<&rule34::Credentials, AppError> {
        self.rule34.as_ref().ok_or(AppError::CredentialsMissing("Rule34.xxx"))
    }

    pub fn api_key(&self, source: Source) -> Option<&str> {
        match source {
            Source::Danbooru => self.danbooru.as_ref().map(|c| c.api_key.as_str()),
            Source::Gelbooru => self.gelbooru.as_ref().map(|c| c.api_key.as_str()),
            Source::E621 => self.e621.as_ref().map(|c| c.api_key.as_str()),
            Source::Rule34 => self.rule34.as_ref().map(|c| c.api_key.as_str()),
            Source::Kemono => self.kemono.as_ref().map(|c| c.session.as_str()),
            Source::Yandere => None,
            Source::Pixiv => self.pixiv.as_ref().map(|c| c.session.as_str()),
            Source::Fanbox => self.fanbox.as_ref().map(|c| c.session.as_str()),
            Source::X => None,
            Source::Custom => None,
        }
    }

    /// 设置或清除某个站点的账号。
    pub fn set(&mut self, source: Source, account: Option<(String, String)>) {
        match source {
            Source::Danbooru => {
                self.danbooru = account.map(|(username, api_key)| danbooru::Credentials { username, api_key })
            }
            Source::Gelbooru => {
                self.gelbooru = account.map(|(user_id, api_key)| gelbooru::Credentials { user_id, api_key })
            }
            Source::E621 => {
                self.e621 = account.map(|(username, api_key)| e621::Credentials { username, api_key })
            }
            Source::Rule34 => {
                self.rule34 = account.map(|(user_id, api_key)| rule34::Credentials { user_id, api_key })
            }
            // Kemono 存的「Key」是登录后的 session Cookie。
            Source::Kemono => self.kemono = account.map(|(_, session)| kemono::Credentials { session }),
            // Yande.re 只存用户名（看收藏用），没有 Key。
            Source::Yandere => {}
            // Pixiv 存的「Key」是登录后的 PHPSESSID，账号的用户 id 从里面取。
            Source::Pixiv => self.pixiv = account.and_then(|(_, session)| pixiv::Credentials::from_session(&session)),
            Source::Fanbox => self.fanbox = account.and_then(|(_, session)| fanbox::Credentials::from_session(&session)),
            Source::X => {}
            Source::Custom => {}
        }
    }
}

/// 运行中可以修改的账号：设置页保存后，之后发出的请求立即使用新账号。
#[derive(Debug, Default)]
pub struct AccountStore(RwLock<Accounts>);

impl AccountStore {
    pub fn new(accounts: Accounts) -> Self {
        Self(RwLock::new(accounts))
    }

    /// 当前账号的副本，请求期间不持锁。
    pub fn get(&self) -> Accounts {
        self.0.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Accounts)) {
        change(&mut self.0.write().unwrap_or_else(PoisonError::into_inner));
    }
}

/// 取一页帖子，返回（整理后的帖子，站点这一页实际返回的条数）。
pub async fn fetch(
    net: &Net,
    accounts: &Accounts,
    source: Source,
    query: &str,
    page: &Page,
    limit: u32,
) -> Result<(Vec<Post>, usize), AppError> {
    match source {
        Source::Danbooru => danbooru::search(net, query, page, limit, accounts.danbooru.as_ref()).await,
        Source::Gelbooru => {
            let creds = accounts.gelbooru()?;
            match page {
                Page::Number(n) => gelbooru::search(net, query, *n, limit, creds).await,
                // Gelbooru 没有按 id 翻页的参数，用 id:> 和按 id 升序排序做到同样的效果。
                Page::After(id) => {
                    let query = format!("{query} id:>{id} sort:id:asc");
                    gelbooru::search(net, query.trim(), 1, limit, creds).await
                }
                Page::Before(_) => gelbooru::search(net, query, 1, limit, creds).await,
            }
        }
        Source::E621 => e621::search(net, query, page, limit, accounts.e621.as_ref()).await,
        Source::Rule34 => {
            let creds = accounts.rule34()?;
            match page {
                Page::Number(n) => rule34::search(net, query, *n, limit, creds).await,
                Page::After(id) => {
                    let query = format!("{query} id:>{id} sort:id:asc");
                    rule34::search(net, query.trim(), 1, limit, creds).await
                }
                Page::Before(_) => rule34::search(net, query, 1, limit, creds).await,
            }
        }
        Source::Kemono => kemono::search(net, query, page, limit, accounts.kemono.as_ref()).await,
        // Yande.re 用 id 条件翻页：默认从新到旧，id:< 接着往旧的方向翻；找新图时 id:> 加按 id 升序。
        Source::Yandere => match page {
            Page::Number(n) => moebooru::search(net, query, *n, limit).await,
            Page::Before(id) => moebooru::search(net, format!("{query} id:<{id}").trim(), 1, limit).await,
            Page::After(id) => moebooru::search(net, format!("{query} id:>{id} order:id").trim(), 1, limit).await,
        },
        Source::Pixiv => pixiv::search(net, accounts.pixiv.as_ref(), query, page).await,
        Source::Fanbox => fanbox::search(net, accounts.fanbox.as_ref(), query, page).await,
        Source::X => Err(AppError::InvalidInput(tr!("X 通过媒体采集窗口使用", "Use X through the media capture window"))),
        Source::Custom => Err(AppError::InvalidInput(tr!("自定义导入不能用于站点搜索", "Custom imports can't be used for site searches"))),
    }
}

/// 查询条件一共能搜到多少张（站点给的估计值）；站点不给数字时为 `None`。
pub async fn count(net: &Net, accounts: &Accounts, source: Source, query: &str) -> Result<Option<u64>, AppError> {
    match source {
        Source::Danbooru => danbooru::count(net, query, accounts.danbooru.as_ref()).await,
        Source::Gelbooru => gelbooru::count(net, query, accounts.gelbooru()?).await,
        Source::E621 => e621::count(net, query, accounts.e621.as_ref()).await,
        Source::Rule34 => rule34::count(net, query, accounts.rule34()?).await,
        Source::Kemono => kemono::count(net, query, accounts.kemono.as_ref()).await,
        Source::Yandere => moebooru::count(net, query).await,
        Source::Pixiv => pixiv::count(net, accounts.pixiv.as_ref(), query).await,
        Source::Fanbox => fanbox::count(net, accounts.fanbox.as_ref(), query).await,
        Source::X => Err(AppError::InvalidInput(tr!("X 通过媒体采集窗口使用", "Use X through the media capture window"))),
        Source::Custom => Err(AppError::InvalidInput(tr!("自定义导入不能用于站点搜索", "Custom imports can't be used for site searches"))),
    }
}

/// 用户输入的 tag 加上分级条件，翻译成站点的查询语法。
/// 四个分级全选或都不选时不加分级条件。
pub fn build_query(source: Source, tags: &str, ratings: &[Rating]) -> String {
    let mut parts: Vec<String> = tags.split_whitespace().map(str::to_string).collect();
    let selected: Vec<Rating> = Rating::ALL.into_iter().filter(|r| ratings.contains(r)).collect();
    if !selected.is_empty() && selected.len() < Rating::ALL.len() {
        match source {
            Source::Danbooru => {
                let codes: Vec<&str> = selected.iter().map(|r| r.danbooru_code()).collect();
                parts.push(format!("rating:{}", codes.join(",")));
            }
            Source::Gelbooru if selected.len() == 1 => {
                parts.push(format!("rating:{}", selected[0].as_str()));
            }
            Source::Gelbooru => {
                let alternatives: Vec<String> =
                    selected.iter().map(|r| format!("rating:{}", r.as_str())).collect();
                parts.push(format!("{{{}}}", alternatives.join(" ~ ")));
            }
            Source::E621 => parts.extend(e621::rating_terms(&selected)),
            Source::Rule34 => {
                if selected.len() == 1 {
                    parts.push(format!("rating:{}", selected[0].as_str()));
                } else {
                    let alternatives: Vec<String> = selected.iter().map(|r| format!("rating:{}", r.as_str())).collect();
                    parts.push(format!("{{{}}}", alternatives.join(" ~ ")));
                }
            }
            Source::Kemono => {}
            Source::Yandere => parts.extend(moebooru::rating_term(&selected)),
            // Pixiv 的适配器自己认这个条件：换成搜索的 mode，再在本地按分级筛。
            Source::Pixiv | Source::Fanbox => {
                let names: Vec<&str> = selected.iter().map(|r| r.as_str()).collect();
                parts.push(format!("rating:{}", names.join(",")));
            }
            Source::X => {}
            Source::Custom => {}
        }
    }
    parts.join(" ")
}

/// 空格分隔的 tag 串拆成列表，去空、去重、保序。
pub(crate) fn split_tags(value: Option<&str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    value
        .unwrap_or_default()
        .split_whitespace()
        .filter(|tag| seen.insert(tag.to_string()))
        .map(str::to_string)
        .collect()
}

/// 把空字符串当成缺失。
pub(crate) fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_join_in_fixed_order() {
        assert_eq!(join_sources(&[Source::Gelbooru, Source::Danbooru, Source::Gelbooru]), "danbooru,gelbooru");
        assert_eq!(join_sources(&[Source::Gelbooru]), "gelbooru");
        assert_eq!(split_sources("gelbooru,konachan,danbooru"), [Source::Danbooru, Source::Gelbooru]);
        assert_eq!(split_sources("danbooru"), [Source::Danbooru]);
        assert!(split_sources("all").is_empty());
    }

    #[test]
    fn query_adds_rating_per_site_syntax() {
        let ratings = [Rating::General, Rating::Sensitive];
        assert_eq!(build_query(Source::Danbooru, " 1girl  scenery ", &ratings), "1girl scenery rating:g,s");
        assert_eq!(build_query(Source::E621, "anthro", &[Rating::General]), "anthro rating:s");
        assert_eq!(build_query(Source::Rule34, "character", &[Rating::Explicit]), "character rating:explicit");
        assert_eq!(build_query(Source::Kemono, "creator:patreon/123", &[Rating::General]), "creator:patreon/123");
        assert_eq!(build_query(Source::Fanbox, "creator:alice", &[Rating::General]), "creator:alice rating:general");
        assert_eq!(
            build_query(Source::Gelbooru, "scenery", &ratings),
            "scenery {rating:general ~ rating:sensitive}"
        );
        assert_eq!(build_query(Source::Gelbooru, "scenery", &[Rating::General]), "scenery rating:general");
        assert_eq!(build_query(Source::Danbooru, "scenery", &Rating::ALL), "scenery");
        assert_eq!(build_query(Source::Danbooru, "", &[]), "");
    }

    #[test]
    fn page_cursor_round_trips_and_advances() {
        assert_eq!(Page::parse("3"), Some(Page::Number(3)));
        assert_eq!(Page::parse("b12345"), Some(Page::Before(12345)));
        assert_eq!(Page::parse("0"), None);
        assert_eq!(Page::parse("bx"), None);
        assert_eq!(Page::Before(9).to_param(), "b9");

        assert_eq!(Page::parse("a77"), Some(Page::After(77)));
        assert_eq!(Page::After(77).to_param(), "a77");

        let first = Page::Number(1);
        assert_eq!(first.next(Source::Danbooru, "scenery rating:g", Some((500, 900))), Page::Before(500));
        assert_eq!(first.next(Source::E621, "scenery rating:s", Some((500, 900))), Page::Before(500));
        assert_eq!(first.next(Source::Danbooru, "scenery order:score", Some((500, 900))), Page::Number(2));
        assert_eq!(first.next(Source::Gelbooru, "scenery", Some((500, 900))), Page::Number(2));
        // 订阅往新的方向走：下一页从这一页最大的 id 之后开始。
        assert_eq!(Page::After(100).next(Source::Danbooru, "scenery", Some((101, 180))), Page::After(180));
        assert_eq!(Page::After(100).next(Source::Gelbooru, "scenery", Some((101, 180))), Page::After(180));
        assert_eq!(Page::After(100).next(Source::Danbooru, "scenery", None), Page::After(100));
        assert!(has_custom_order("sky sort:score"));
        assert!(!has_custom_order("sky rating:g"));
    }

    #[test]
    fn sort_adds_site_term_and_overrides_typed_order() {
        let params = |source, tags: &str, sort| SearchParams { source, pixiv_input: false, tags: tags.into(), ratings: vec![], sort, cursor: None };
        assert_eq!(params(Source::Danbooru, "sky", Sort::Newest).tags_with_sort().unwrap(), "sky");
        // 默认顺序时照样用手写的排序；选了排序就以选项为准。
        assert_eq!(params(Source::Danbooru, "sky order:score", Sort::Newest).tags_with_sort().unwrap(), "sky order:score");
        assert_eq!(params(Source::Danbooru, "sky Order:Score", Sort::Favorites).tags_with_sort().unwrap(), "sky order:favcount");
        assert_eq!(params(Source::Gelbooru, "sky sort:score", Sort::Oldest).tags_with_sort().unwrap(), "sky sort:id:asc");
        assert!(params(Source::Gelbooru, "sky", Sort::Favorites).tags_with_sort().is_err());
        // 选了排序后改按页码翻页。
        let tags = params(Source::Danbooru, "sky", Sort::Score).tags_with_sort().unwrap();
        assert!(has_custom_order(&build_query(Source::Danbooru, &tags, &[])));
        // 计数不带排序，近期热门除外（它同时限定了时间范围）。
        assert_eq!(params(Source::Danbooru, "sky", Sort::Score).tags_for_count().unwrap(), "sky");
        assert_eq!(params(Source::Danbooru, "sky", Sort::Popular).tags_for_count().unwrap(), "sky order:rank");
    }

    #[test]
    fn pixiv_numeric_inputs_are_explicit_before_building_new_query_plans() {
        let params = SearchParams {
            source: Source::Pixiv, pixiv_input: true, tags: "22675109".into(), ratings: vec![Rating::General], sort: Sort::Newest, cursor: None,
        };
        assert_eq!(params.tags_with_sort().unwrap(), "id:22675109");
        assert_eq!(params.tags_for_count().unwrap(), "id:22675109");
        let subscription = filter::plan_query(params.source, &params.tags, &params.ratings, None).unwrap();
        assert_eq!(subscription.server_query, "22675109 rating:general");
        let saved = SearchParams { pixiv_input: false, ..params.clone() };
        assert_eq!(saved.tags_with_sort().unwrap(), "22675109");
        assert_eq!(saved.tags_for_count().unwrap(), "22675109");
        let oldest = SearchParams { sort: Sort::Oldest, ..params.clone() };
        assert_eq!(oldest.tags_with_sort().unwrap(), "id:22675109 order:date");
        assert_eq!(oldest.tags_for_count().unwrap(), "id:22675109");
        let other = SearchParams { source: Source::Danbooru, ..params };
        assert_eq!(other.normalized_tags(), "22675109");
        assert_eq!(other.tags_with_sort().unwrap(), "22675109");
        assert_eq!(other.tags_for_count().unwrap(), "22675109");
    }

    #[test]
    fn url_source_requires_known_host() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert_eq!(source_for_url(&url("https://cdn.donmai.us/original/a.png")), Some(Source::Danbooru));
        assert_eq!(source_for_url(&url("https://static1.e621.net/data/a.jpg")), Some(Source::E621));
        assert_eq!(source_for_url(&url("https://us.rule34.xxx/images/a.jpg")), Some(Source::Rule34));
        assert_eq!(source_for_url(&url("https://kemono.cr/data/a.jpg")), Some(Source::Kemono));
        assert_eq!(source_for_url(&url("https://downloads.fanbox.cc/images/a.jpg")), Some(Source::Fanbox));
        assert_eq!(source_for_url(&url("https://downloads.fanbox.cc.evil.test/a.jpg")), None);
        assert_eq!(source_for_url(&url("https://user:pw@cdn.donmai.us/a.png")), None);
        assert_eq!(source_for_url(&url("https://cdn.donmai.us:8443/a.png")), None);
        assert_eq!(source_for_url(&url("file:///etc/passwd")), None);
    }

    #[test]
    fn host_matching_respects_dot_boundary() {
        assert_eq!(Source::for_host("cdn.donmai.us"), Some(Source::Danbooru));
        assert_eq!(Source::for_host("img4.gelbooru.com."), Some(Source::Gelbooru));
        assert_eq!(Source::for_host("n4.kemono.cr"), Some(Source::Kemono));
        assert_eq!(Source::for_host("evildonmai.us"), None);
        assert_eq!(Source::for_host("donmai.us.example.com"), None);
    }
}
