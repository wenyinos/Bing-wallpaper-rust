//! HTTP 下载器：超时/UA 由共享 Client 统一配置；
//! 先写 `.part` 临时文件再原子替换，避免中断留下半张壁纸。

use std::path::Path;

use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::debug;

#[derive(Debug, Error)]
pub enum DownloaderError {
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("HTTP 状态码错误: {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("不支持的内容类型: {0}")]
    UnsupportedContentType(String),
    #[error("写入本地文件失败: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Downloader {
    pub http: reqwest::Client,
}

impl Downloader {
    /// 下载到 `dest`，返回写入字节数
    pub async fn download_to_file(&self, url: &str, dest: &Path) -> Result<u64, DownloaderError> {
        let response = self.http.get(url).send().await?;
        let response = response
            .error_for_status()
            .map_err(|err| match err.status() {
                Some(status) => DownloaderError::HttpStatus(status),
                None => DownloaderError::Network(err),
            })?;

        // Content-Type 检查（方案 §7）；服务器缺失该头时不拦截
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
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut response = response;
        let mut total: u64 = 0;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            total += chunk.len() as u64;
        }
        file.flush().await?;
        drop(file);

        // std/tokio 的 rename 在 Windows 上为 MoveFileExW(REPLACE_EXISTING) 语义
        tokio::fs::rename(&tmp, dest).await?;
        debug!("下载完成: {url} -> {}（{total} 字节）", dest.display());
        Ok(total)
    }
}
