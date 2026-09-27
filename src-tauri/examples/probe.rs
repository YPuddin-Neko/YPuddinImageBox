//! 用真实网络检查 Rust 客户端：能否通过 Danbooru 的 Cloudflare、tag 额度怎么计算、预览图能否下载，
//! 以及各种代理设置下的连接结果和报错文字。
//! 运行：cargo run --example probe

use imagebox_lib::net::{test_connection, Net};
use imagebox_lib::settings::{ProxyMode, ProxySettings};
use imagebox_lib::sources::{build_query, danbooru, gelbooru, Page, Rating, Source};

#[tokio::main]
async fn main() {
    let net = Net::new(&ProxySettings::default()).expect("HTTP 客户端创建失败");
    let cases: [(&str, &[Rating]); 4] = [
        ("scenery", &[Rating::General]),
        ("1girl scenery", &[Rating::General, Rating::Sensitive]),
        ("1girl scenery sky", &[]),
        ("1girl scenery -comic", &[]),
    ];
    let mut first_thumb = None;
    for (tags, ratings) in cases {
        let query = build_query(Source::Danbooru, tags, ratings);
        match danbooru::search(&net, &query, &Page::Number(1), 5, None).await {
            Ok((posts, fetched)) => {
                let first = posts.first();
                println!(
                    "成功  {query:<36} 返回 {fetched} 条，可用 {} 条，首条 {}",
                    posts.len(),
                    first.map_or("-".to_string(), |p| format!("#{} {}x{} {}", p.id, p.width, p.height, p.file_ext))
                );
                if first_thumb.is_none() {
                    first_thumb = first.and_then(|p| p.thumb_url.clone());
                }
            }
            Err(err) => println!("失败  {query:<36} {err}"),
        }
    }
    if let Some(url) = first_thumb {
        let request = net.client().get(&url).header("Referer", Source::Danbooru.referer());
        match net.preview.send(request).await {
            Ok(resp) => {
                let status = resp.status();
                let kind = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
                let size = resp.bytes().await.map(|b| b.len()).unwrap_or(0);
                println!("预览图 {status} {kind} {size} 字节  {url}");
            }
            Err(err) => println!("预览图下载失败：{err}"),
        }
    }

    // 填错的账号：两个站点都应该提示账号或 Key 不对。
    let fake = danbooru::Credentials { username: "imagebox_probe_nobody".into(), api_key: "wrong".into() };
    match danbooru::verify(&net, &fake).await {
        Ok(profile) => println!("Danbooru 假账号居然通过了：{profile:?}"),
        Err(err) => println!("Danbooru 假账号：{err}"),
    }
    let fake = gelbooru::Credentials { user_id: "1".into(), api_key: "wrong".into() };
    match gelbooru::verify(&net, &fake).await {
        Ok(()) => println!("Gelbooru 假账号居然通过了"),
        Err(err) => println!("Gelbooru 假账号：{err}"),
    }

    let proxies = [
        ("跟随系统", ProxySettings::default()),
        ("不使用代理", ProxySettings { mode: ProxyMode::None, url: String::new() }),
        // 本机 9 号端口通常没有服务，用来看连不上代理时的提示。
        ("手动（无效端口）", ProxySettings { mode: ProxyMode::Manual, url: "127.0.0.1:9".into() }),
        ("手动（格式错误）", ProxySettings { mode: ProxyMode::Manual, url: "ftp://127.0.0.1:21".into() }),
    ];
    for (label, proxy) in proxies {
        match test_connection(&proxy).await {
            Ok(elapsed) => println!("代理 {label}：连接正常，{} ms", elapsed.as_millis()),
            Err(err) => println!("代理 {label}：{err}"),
        }
    }
}
