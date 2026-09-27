//! 图库缩略图：宽不超过 480、高不超过 1440，存在「缓存」位置的 thumbs 目录，
//! 删掉后浏览图库时会重新生成。不透明的图存 JPEG，有透明区域的存 PNG。

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, ImageReader};
use tokio::sync::Semaphore;

use crate::sources::Source;

const MAX_WIDTH: u32 = 480;
const MAX_HEIGHT: u32 = 1440;
const JPEG_QUALITY: u8 = 85;

/// 解码大图很吃内存和 CPU：下载和浏览共用这几个名额，避免一屏缩略图同时解码几十张原图。
static SLOTS: Semaphore = Semaphore::const_new(3);

pub fn path(cache_root: &Path, source: Source, post_id: u64) -> PathBuf {
    cache_root.join("thumbs").join(source.as_str()).join(post_id.to_string())
}

/// 生成缩略图并写入 `dest`，返回编码后的内容。
pub async fn generate(src: PathBuf, dest: PathBuf) -> Result<Vec<u8>, String> {
    let _slot = SLOTS.acquire().await.map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || {
        let bytes = render(&src)?;
        write_atomic(&dest, &bytes).map_err(|e| format!("写入缩略图失败：{e}"))?;
        Ok(bytes)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn render(src: &Path) -> Result<Vec<u8>, String> {
    let image = ImageReader::open(src)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| format!("读取原图失败：{e}"))?
        .decode()
        .map_err(|e| format!("无法解码原图：{e}"))?;
    let image = if image.width() > MAX_WIDTH || image.height() > MAX_HEIGHT {
        image.thumbnail(MAX_WIDTH, MAX_HEIGHT)
    } else {
        image
    };
    encode(&image)
}

fn encode(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    if has_transparency(image) {
        image.write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    } else {
        JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
            .encode_image(&image.to_rgb8())
            .map_err(|e| e.to_string())?;
    }
    Ok(out.into_inner())
}

fn has_transparency(image: &DynamicImage) -> bool {
    image.color().has_alpha() && image.to_rgba8().pixels().any(|pixel| pixel.0[3] < u8::MAX)
}

fn write_atomic(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = dest.with_extension(format!("{}.part", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, dest).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn decode(bytes: &[u8]) -> DynamicImage {
        image::load_from_memory(bytes).unwrap()
    }

    #[tokio::test]
    async fn shrinks_opaque_images_to_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("wide.png");
        RgbImage::from_pixel(2000, 1000, Rgb([200, 40, 90])).save(&src).unwrap();
        let dest = path(dir.path(), Source::Danbooru, 7);
        let bytes = generate(src, dest.clone()).await.unwrap();
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
        let thumb = decode(&std::fs::read(&dest).unwrap());
        assert_eq!((thumb.width(), thumb.height()), (480, 240));
    }

    #[tokio::test]
    async fn keeps_transparency_as_png_and_never_upscales() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("small.png");
        let mut img = RgbaImage::from_pixel(100, 300, Rgba([10, 20, 30, 255]));
        img.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        img.save(&src).unwrap();
        let bytes = generate(src, dir.path().join("t")).await.unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");
        let thumb = decode(&bytes);
        assert_eq!((thumb.width(), thumb.height()), (100, 300));
    }

    /// 缩略图生成速度：几种常见尺寸的原图各生成几次，打印每张的耗时。
    /// 手动运行：cargo test --release --lib thumbs::tests::speed -- --ignored --nocapture
    #[test]
    #[ignore = "性能测试，需要手动运行"]
    fn speed() {
        let dir = tempfile::tempdir().unwrap();
        for (ext, width, height) in [("jpg", 2480, 3508), ("jpg", 4000, 6000), ("png", 2000, 3000), ("webp", 1600, 2400)] {
            // 渐变加一点纹理，免得编码器把纯色图压得太小、解码太快。
            let img = RgbImage::from_fn(width, height, |x, y| {
                Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, ((x ^ y) & 0xff) as u8])
            });
            let src = dir.path().join(format!("{width}x{height}.{ext}"));
            img.save(&src).unwrap();
            let size = std::fs::metadata(&src).unwrap().len() as f64 / 1_048_576.0;
            let runs = 5;
            let start = std::time::Instant::now();
            for _ in 0..runs {
                render(&src).unwrap();
            }
            let each = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;
            println!("{ext:>4} {width}×{height}（{size:.1} MB）：每张 {each:.0} ms");
        }
    }

    #[tokio::test]
    async fn reports_undecodable_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("broken.png");
        std::fs::write(&src, b"not an image").unwrap();
        assert!(generate(src, dir.path().join("t")).await.is_err());
    }
}
