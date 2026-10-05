//! 大图库的性能：在临时目录里造一个 N 张图（默认 10 万）的图库，测写入速度和图库页常用查询的耗时。
//! 每张图 1 个画师、1 个作品、0～2 个角色、15～30 个一般 tag，tag 的使用频率按长尾分布。
//! 运行：cargo run --release --example library_bench [-- 张数]
//! 设了 IBX_BENCH_DB=目录 时数据库留在那里，下次运行直接用，不用重新写入。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use imagebox_lib::library::{GroupKind, GroupQuery, GroupSort, Library, LibraryQuery, LibrarySort};
use imagebox_lib::sources::{Post, PostTags, Rating, Source};

/// 固定种子的 xorshift，每次造出来的数据一样，前后两次测量可以直接比较。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// 长尾分布：小编号出现得多，模拟热门 tag。
    fn skewed(&mut self, n: u64) -> u64 {
        let u = (self.next() % 1_000_000) as f64 / 1_000_000.0;
        ((u * u * u) * n as f64) as u64
    }
}

fn post(rng: &mut Rng, id: u64) -> Post {
    let mut tags = PostTags {
        artist: vec![format!("artist_{}", rng.skewed(5_000))],
        copyright: vec![format!("series_{}", rng.skewed(500))],
        ..PostTags::default()
    };
    for _ in 0..rng.below(3) {
        tags.character.push(format!("character_{}", rng.skewed(3_000)));
    }
    let count = 15 + rng.below(16);
    while tags.general.len() < count as usize {
        let tag = format!("tag_{}", rng.skewed(20_000));
        if !tags.general.contains(&tag) {
            tags.general.push(tag);
        }
    }
    let rating = match rng.below(100) {
        0..=49 => Rating::General,
        50..=79 => Rating::Sensitive,
        80..=91 => Rating::Questionable,
        _ => Rating::Explicit,
    };
    let (width, height) = [(1200, 1600), (2000, 3000), (3840, 2160), (1080, 1920), (2480, 3508)][rng.below(5) as usize];
    Post {
        source: if rng.below(10) < 8 { Source::Danbooru } else { Source::Gelbooru },
        id,
        md5: Some(format!("{:032x}", rng.next() as u128 * 7919 + id as u128)),
        width,
        height,
        rating: Some(rating),
        score: rng.skewed(800) as i64,
        fav_count: Some(rng.skewed(1_200) as i64),
        file_ext: "jpg".into(),
        file_name: None,
        title: None,
        file_size: Some(200_000 + rng.below(15_000_000)),
        file_url: Some(format!("https://cdn.donmai.us/original/{id}.jpg")),
        sample_url: None,
        thumb_url: None,
        created_at: Some(format!(
            "20{:02}-{:02}-{:02}T{:02}:{:02}:00.000-04:00",
            15 + rng.below(12),
            1 + rng.below(12),
            1 + rng.below(28),
            rng.below(24),
            rng.below(60)
        )),
        post_url: format!("https://danbooru.donmai.us/posts/{id}"),
        tags,
        pages: None,
    }
}

/// 跑 5 次取中位数，排除第一次冷缓存的影响；同时返回最后一次的结果。
async fn median<T, F: Future<Output = T>>(mut run: impl FnMut() -> F) -> (Duration, T) {
    let mut times = Vec::new();
    let mut result = None;
    for _ in 0..5 {
        let start = Instant::now();
        result = Some(run().await);
        times.push(start.elapsed());
    }
    times.sort();
    (times[2], result.expect("跑过 5 次"))
}

async fn measure(lib: &Library, label: &str, query: LibraryQuery) {
    let query = &query;
    let (time, page) = median(|| async move { lib.list(query).await.expect("查询") }).await;
    println!("{label:<28} {:>8.1} ms   共 {} 张", ms(time), page.total);
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn db_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map(|entries| entries.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let count: u64 = std::env::args().nth(1).and_then(|arg| arg.parse().ok()).unwrap_or(100_000);
    let temp = tempfile::tempdir().expect("临时目录");
    let dir = std::env::var_os("IBX_BENCH_DB").map(PathBuf::from).unwrap_or_else(|| temp.path().to_path_buf());
    let opened = Instant::now();
    let lib = Library::open(&dir).await.expect("打开数据库");
    println!("打开数据库 {:.1} ms", ms(opened.elapsed()));
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let existing = lib.list(&LibraryQuery { limit: 1, ..LibraryQuery::default() }).await.expect("查询").total as u64;

    let start = Instant::now();
    for i in existing..count {
        let post = post(&mut rng, 1_000_000 + i);
        let path = format!("/images/{}/{}.jpg", post.source.as_str(), post.id);
        lib.save_post(&post, Path::new(&path), i as i64).await.expect("写入");
        if (i + 1) % 20_000 == 0 {
            println!("已写入 {} 张，{:.1} 秒", i + 1, start.elapsed().as_secs_f64());
        }
    }
    let elapsed = start.elapsed();
    let written = count.saturating_sub(existing).max(1);
    println!(
        "写入 {} 张用了 {:.1} 秒，每张 {:.2} ms；数据库 {:.1} MB",
        count.saturating_sub(existing),
        elapsed.as_secs_f64(),
        ms(elapsed) / written as f64,
        db_size(&dir) as f64 / 1_048_576.0
    );
    // 软件每次启动打开图库时都会更新统计信息，这里照样做一次，测到的是真实使用时的情况。
    let start = Instant::now();
    lib.optimize().await.expect("更新统计信息");
    let (version, analyzed) = lib.stats_info().await.expect("查询");
    println!("SQLite {version}，更新统计信息 {:.1} ms，已有统计信息：{analyzed}", ms(start.elapsed()));
    println!();

    let page = |sort| LibraryQuery { sort, limit: 60, ..LibraryQuery::default() };
    for (label, sort) in [
        ("默认（最近下载）", LibrarySort::Downloaded),
        ("最早下载", LibrarySort::DownloadedAsc),
        ("最新上传", LibrarySort::Newest),
        ("分数最高", LibrarySort::Score),
        ("收藏最多", LibrarySort::Favorites),
        ("分辨率最高", LibrarySort::Resolution),
        ("文件最大", LibrarySort::Filesize),
    ] {
        measure(&lib, label, page(sort)).await;
    }
    let deep = count as u32 / 2;
    measure(&lib, "翻到一半（最近下载）", LibraryQuery { offset: deep, ..page(LibrarySort::Downloaded) }).await;
    measure(&lib, "翻到一半（分数最高）", LibraryQuery { offset: deep, ..page(LibrarySort::Score) }).await;

    let tags = |tags: &str| LibraryQuery { tags: tags.into(), ..page(LibrarySort::Downloaded) };
    measure(&lib, "热门 tag（tag_0）", tags("tag_0")).await;
    measure(&lib, "冷门 tag（tag_19000）", tags("tag_19000")).await;
    measure(&lib, "两个热门 tag", tags("tag_0 tag_1")).await;
    measure(&lib, "排除热门 tag", tags("-tag_0")).await;
    measure(&lib, "画师（artist_7）", tags("artist_7")).await;
    measure(&lib, "tag + 分数排序", LibraryQuery { tags: "tag_2".into(), ..page(LibrarySort::Score) }).await;
    let ratings = vec![Rating::General, Rating::Sensitive];
    measure(&lib, "分级 + 来源", LibraryQuery { ratings: ratings.clone(), source: Some(Source::Danbooru), ..page(LibrarySort::Downloaded) }).await;
    measure(&lib, "分级 + tag + 发布时间", LibraryQuery { ratings, tags: "tag_3".into(), ..page(LibrarySort::Newest) }).await;

    // 图库首页的来源文件夹，以及点进去以后按画师、作品、角色、一般 tag 分组。
    let library = &lib;
    let (time, folders) = median(|| async move { library.folders().await.expect("查询") }).await;
    let summary: Vec<String> = folders.iter().map(|folder| format!("{} {} 张", folder.source.as_str(), folder.count)).collect();
    println!("{:<28} {:>8.1} ms   {}", "来源文件夹（含封面）", ms(time), summary.join("，"));
    for (kind, name) in [
        (GroupKind::Artist, "画师"),
        (GroupKind::Copyright, "作品"),
        (GroupKind::Character, "角色"),
        (GroupKind::General, "一般 tag"),
    ] {
        for (label, sort) in [("最近下载", GroupSort::Recent), ("图片最多", GroupSort::Count), ("名称", GroupSort::Name)] {
            let query = &GroupQuery { source: Source::Danbooru, kind, sort, offset: 0, limit: 60 };
            let (time, page) = median(|| async move { library.groups(query).await.expect("查询") }).await;
            println!("{:<28} {:>8.1} ms   {} 组", format!("分组：{name}（{label}）"), ms(time), page.total);
        }
    }

    // 帖子 id 都在图库里，md5 换成图库里没有的：两种查法都要走一遍索引。
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let probes: Vec<Post> = (0..200).map(|i| post(&mut rng, 1_000_000 + i * (count / 200))).collect();
    let start = Instant::now();
    let owned = lib.owned(&probes).await.expect("查询");
    println!("{:<28} {:>8.1} ms   找到 {} 张", "搜索结果里哪些已下载（200 个）", ms(start.elapsed()), owned.len());
}
