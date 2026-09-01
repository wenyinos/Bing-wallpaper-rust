//! HTTP 下载器：超时/UA 由共享 Client 统一配置；
//! 先写 `.part` 临时文件再原子替换，避免中断留下半张壁纸。
//!
//! 安全基线（安全审查 #3.2/#3.3）：仅 HTTPS、单文件大小上限、
//! 落盘前校验 JPEG/PNG magic bytes（Content-Type 可伪造/缺失）。

use std::path::Path;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::debug;

/// 单文件大小上限（壁纸 JPEG/PNG 远小于此），防止恶意服务器无限发送填满磁盘
const MAX_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum DownloaderError {
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("HTTP 状态码错误: {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("不支持的内容类型: {0}")]
    UnsupportedContentType(String),
    #[error("下载内容不是有效的 JPEG/PNG 图片")]
    InvalidImage,
    #[error("下载数据超过大小上限（{0} 字节）")]
    TooLarge(u64),
    #[error("仅允许 HTTPS 下载地址")]
    InsecureScheme,
    #[error("写入本地文件失败: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Downloader {
    pub http: reqwest::Client,
}

impl Downloader {
    /// 下载到 `dest`，返回写入字节数
    pub async fn download_to_file(&self, url: &str, dest: &Path) -> Result<u64, DownloaderError> {
        if !url.starts_with("https://") {
            return Err(DownloaderError::InsecureScheme);
        }
        let response = self.http.get(url).send().await?;
        let response = response
            .error_for_status()
            .map_err(|err| match err.status() {
                Some(status) => DownloaderError::HttpStatus(status),
                None => DownloaderError::Network(err),
            })?;

        // Content-Length 预检（存在且超限直接拒绝）
        if let Some(len) = response.content_length() {
            if len > MAX_DOWNLOAD_BYTES {
                return Err(DownloaderError::TooLarge(len));
            }
        }

        // Content-Type 检查（方案 §7）；服务器缺失该头时不拦截（落盘前还有 magic bytes 校验）
        if let Some(ct) = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
        {
            if !ct.starts_with("image/") {
                return Err(DownloaderError::UnsupportedContentType(ct.to_string()));
            }
        }

        let tmp = dest.with_extension("part");
        let total =
            match write_with_limit(&mut response, tokio::fs::File::create(&tmp).await?).await {
                Ok(total) => total,
                Err(err) => {
                    // 句柄已在 write_with_limit 内同步关闭，可安全清理
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(err);
                }
            };

        // magic bytes 校验：拒绝伪装成图片的任意内容进入系统图像解码器
        if !is_jpeg_or_png(&tmp).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(DownloaderError::InvalidImage);
        }

        // std/tokio 的 rename 在 Windows 上为 MoveFileExW(REPLACE_EXISTING) 语义
        tokio::fs::rename(&tmp, dest).await?;
        debug!("下载完成: {url} -> {}（{total} 字节）", dest.display());
        Ok(total)
    }
}

/// 流式写盘并强制大小上限。无论成败都先同步关闭文件句柄再返回：
/// tokio File 的关闭在后台任务执行，不关闭会让 Windows 上的
/// remove/rename 撞上句柄占用。
async fn write_with_limit(
    response: &mut reqwest::Response,
    mut file: tokio::fs::File,
) -> Result<u64, DownloaderError> {
    let result = async {
        let mut total: u64 = 0;
        while let Some(chunk) = response.chunk().await? {
            total += chunk.len() as u64;
            if total > MAX_DOWNLOAD_BYTES {
                return Err(DownloaderError::TooLarge(total));
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(total)
    }
    .await;
    let std_file = file.into_std().await;
    drop(std_file);
    result
}

/// JPEG（FF D8 FF）/ PNG（89 50 4E 47）魔数校验
async fn is_jpeg_or_png(path: &Path) -> bool {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut head = [0u8; 4];
    match file.read(&mut head).await {
        Ok(n) if n >= 4 => {}
        _ => return false,
    }
    head[0..3] == [0xFF, 0xD8, 0xFF] || head == [0x89, b'P', b'N', b'G']
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_only() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建测试 runtime 失败");
        let result = rt.block_on(async {
            let http = reqwest::Client::new();
            Downloader { http }
                .download_to_file("http://example.com/a.jpg", Path::new("/tmp/bwr-x.jpg"))
                .await
        });
        assert!(matches!(result, Err(DownloaderError::InsecureScheme)));
    }
}
