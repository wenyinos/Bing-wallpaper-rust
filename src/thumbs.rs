//! 缩略图生成（方案 §20：历史壁纸网格预览）。

use std::path::{Path, PathBuf};

use tracing::debug;

pub const THUMB_WIDTH: u32 = 320;

/// cache/bing/xxx.jpg -> cache/thumbnails/bing_xxx.jpg（扁平化目录）
pub fn thumbnail_path(cache_dir: &Path, entry_file: &str) -> PathBuf {
    let flat = entry_file.replace(['/', '\\'], "_");
    cache_dir
        .join("thumbnails")
        .join(format!("{flat}.thumb.jpg"))
}

/// 确保缩略图存在并返回其路径；失败返回 None（历史页跳过显示）
pub fn ensure_thumbnail(cache_dir: &Path, source: &Path, entry_file: &str) -> Option<PathBuf> {
    let dst = thumbnail_path(cache_dir, entry_file);
    if dst.exists() {
        return Some(dst);
    }
    let img = image::io::Reader::open(source)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    std::fs::create_dir_all(dst.parent()?).ok()?;
    let thumb = img.thumbnail(THUMB_WIDTH, THUMB_WIDTH);
    if let Err(err) = thumb.save_with_format(&dst, image::ImageFormat::Jpeg) {
        debug!("缩略图生成失败 {}: {err}", dst.display());
        return None;
    }
    debug!("缩略图已生成: {}", dst.display());
    Some(dst)
}
