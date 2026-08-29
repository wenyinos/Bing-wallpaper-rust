//! 配置加载/保存（%LOCALAPPDATA%\BingWallpaper-Rust\config.json，方案 §10）。
//!
//! P0 只读取 `bing_preset` 与 `fit_mode`；其余字段为 P1（配置 UI / 定时器）预留，
//! P1 落地前以 `#[allow(dead_code)]` 标注，避免警告噪音。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

pub const APP_DIR_NAME: &str = "BingWallpaper-Rust";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(dead_code)] // P1 启用：auto_update/update_interval_hours/cache_days/market/language
pub struct Config {
    pub provider: String,
    pub bing_preset: String,
    pub market: String,
    pub auto_update: bool,
    pub update_interval_hours: u32,
    pub cache_days: u32,
    pub fit_mode: String,
    pub language: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "bing".into(),
            bing_preset: "china".into(),
            market: "zh-CN".into(),
            auto_update: true,
            update_interval_hours: 24,
            cache_days: 30,
            fit_mode: "fill".into(),
            language: "zh".into(),
        }
    }
}

/// %LOCALAPPDATA%\BingWallpaper-Rust（决策 #11）
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join(APP_DIR_NAME))
}

impl Config {
    /// 读取失败（含首次启动无文件）一律回退默认配置，不阻塞启动
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("config.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(cfg) => cfg,
                Err(err) => {
                    warn!("config.json 解析失败，使用默认配置: {err}");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).expect("配置序列化失败");
        std::fs::write(dir.join("config.json"), text)
    }
}
