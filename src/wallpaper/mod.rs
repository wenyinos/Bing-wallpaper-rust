//! Wallpaper Manager：设置桌面壁纸。
//!
//! MVP 采用系统注册表 WallpaperStyle（方案 §13 决策：程序级 crop/resize 推至 P2）。

#[cfg(windows)]
pub mod windows;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WallpaperError {
    #[error("壁纸文件不存在: {0}")]
    InvalidPath(String),
    #[error("Windows API 调用失败（GetLastError={0}）")]
    Api(u32),
    #[error("注册表写入失败（{key}，GetLastError={code}）")]
    Registry { key: &'static str, code: u32 },
}

/// 适配模式（方案 §13：Fill/Fit/Stretch/Center）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Fill,
    Fit,
    Stretch,
    Center,
    Span,
}

impl FitMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "fit" => FitMode::Fit,
            "stretch" => FitMode::Stretch,
            "center" => FitMode::Center,
            "span" => FitMode::Span,
            _ => FitMode::Fill,
        }
    }
}

/// 设置壁纸。`fit` 为配置中的字符串（"fill"/"fit"/"stretch"/"center"/"span"）。
pub fn set_wallpaper(path: &std::path::Path, fit: &str) -> Result<(), WallpaperError> {
    #[cfg(windows)]
    {
        windows::set_wallpaper(path, FitMode::parse(fit))
    }
    #[cfg(not(windows))]
    {
        let _ = (path, fit);
        Err(WallpaperError::Api(0)) // 非 Windows 平台未实现（发布目标仅为 Windows）
    }
}
