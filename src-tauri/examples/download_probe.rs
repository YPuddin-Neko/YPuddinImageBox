//! 用真实网络跑一遍下载队列：选中下载 3 张、按条件下载 5 张、订阅检查一次（起点设在
//! 第 3 新的帖子，应该正好找到比它新的几张），存到临时目录后检查文件、缩略图和图库记录，
//! 最后删除临时目录。
//! 运行：cargo run --example download_probe

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use imagebox_lib::downloader::{Downloader, Event, EventSink};
use imagebox_lib::library::{JobStatus, Library, LibraryQuery, NewSubscription};
use imagebox_lib::net::Net;
use imagebox_lib::settings::ProxySettings;
use imagebox_lib::sources::{self, AccountStore, Page, Rating, Source};
use imagebox_lib::storage::{Defaults, Storage};

// 小图，免得探测时下载太多流量。
const QUERY: &str = "scenery filesize:..300kb rating:g";

#[tokio::main]
async fn main() {
    let dir = tempfile::tempdir().expect("临时目录");
    let defaults = Defaults {
        images: dir.path().join("images"),
        data: dir.path().join("data"),
        cache: dir.path().join("cache"),
    };
    let storage = Storage::load(dir.path().join("storage.json"), defaults);
    let library = Library::open(&dir.path().join("database")).await.expect("打开数据库");
    let net = Arc::new(Net::new(&ProxySettings::default()).expect("HTTP 客户端"));
    let accounts = Arc::new(AccountStore::default());
    let events: EventSink = Arc::new(|event| {
        if let Event::Job(job) = event {
            println!(
                "  任务 {} {:?}：存 {} / 跳过 {} / 失败 {} / 共 {:?}",
                job.id, job.status, job.saved, job.skipped, job.failed, job.total
            );
        }
    });
    let downloader = Downloader::new(
        library.clone(),
        Arc::clone(&net),
        Arc::clone(&accounts),
        Arc::new(RwLock::new(storage)),
        Arc::new(tokio::sync::RwLock::new(())),
        events,
    );
    tokio::spawn(Arc::clone(&downloader).run());

    let (posts, _) = sources::fetch(&net, &accounts.get(), Source::Danbooru, QUERY, &Page::Number(1), 3)
        .await
        .expect("搜索");
    println!("选中下载 {} 张", posts.len());
    let first = downloader.enqueue_posts(posts.clone()).await.expect("加入队列");
    let second = downloader.enqueue_query(Source::Danbooru, "按条件", QUERY, Some(5)).await.expect("加入队列");

    // 订阅：起点设在第 3 新的帖子，检查时应该找到比它新的那几张。
    let start = posts.iter().map(|post| post.id).min().expect("至少一张") as i64;
    let sub = library
        .create_subscription(NewSubscription {
            source: Source::Danbooru,
            tags: "scenery filesize:..300kb",
            ratings: &[Rating::General],
            query: QUERY,
            interval_minutes: 60,
            last_seen_id: start,
        })
        .await
        .expect("建订阅");
    let third = downloader.check_subscription(sub.id).await.expect("检查订阅").expect("应该新建检查任务");
    assert!(downloader.check_subscription(sub.id).await.unwrap().is_none(), "检查中不应重复建任务");

    for job in [first.id, second.id, third.id] {
        let start = Instant::now();
        loop {
            let info = library.job(job).await.unwrap().unwrap();
            if matches!(info.status, JobStatus::Done | JobStatus::Failed) {
                println!("任务 {job} 结束：{:?} {:?}", info.status, info.error);
                for note in library.item_notes(job).await.unwrap() {
                    println!("    #{} {}：{:?}", note.post_id, note.status, note.note);
                }
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(180), "任务 {job} 超时");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    let sub = library.subscription(sub.id).await.unwrap().unwrap();
    println!(
        "订阅：起点 #{start}，检查后处理到 #{}，找到 {} 张新图，出错：{:?}",
        sub.last_seen_id, sub.last_new, sub.last_error
    );
    assert!(sub.last_seen_id > start && sub.last_new >= 2, "订阅应该找到比起点新的图");

    let page = library.list(&LibraryQuery::default()).await.unwrap();
    println!("图库共 {} 张", page.total);
    for post in &page.posts {
        let file = std::fs::metadata(&post.path).map(|m| m.len()).unwrap_or(0);
        let thumb = dir.path().join("cache/thumbs/danbooru").join(post.post.id.to_string());
        let thumb = std::fs::metadata(&thumb).map(|m| m.len()).unwrap_or(0);
        println!(
            "  #{} {} 字节，缩略图 {} 字节，tag {} 个，{}",
            post.post.id,
            file,
            thumb,
            post.post.tags.general.len() + post.post.tags.artist.len(),
            post.path.trim_start_matches(&dir.path().to_string_lossy().into_owned())
        );
        assert!(file > 0 && thumb > 0);
    }
}
