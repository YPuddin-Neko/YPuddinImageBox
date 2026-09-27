//! 用真实网络检查 Rust 客户端：能否通过 Danbooru 的 Cloudflare、tag 额度怎么计算、预览图能否下载。
//! 运行：cargo run --example probe

use imagebox_lib::net::Net;
use imagebox_lib::sources::{build_query, danbooru, Rating, Source};

#[tokio::main]
async fn main() {
    let net = Net::new().expect("HTTP 客户端创建失败");
    let cases: [(&str, &[Rating]); 4] = [
        ("scenery", &[Rating::General]),
        ("1girl scenery", &[Rating::General, Rating::Sensitive]),
        ("1girl scenery sky", &[]),
        ("1girl scenery -comic", &[]),
    ];
    let mut first_thumb = None;
    for (tags, ratings) in cases {
        let query = build_query(Source::Danbooru, tags, ratings);
        match danbooru::search(&net, &query, 1, 5, None).await {
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
        let request = net.client.get(&url).header("Referer", Source::Danbooru.referer());
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
}
