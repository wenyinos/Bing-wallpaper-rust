//! 配置加载/保存（%LOCALAPPDATA%\BingWallpaper-Rust\config.json，方案 §10）。
//!
//! 未知字段（旧版本配置）由 serde 自动忽略；缺字段回退默认值。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

pub const APP_DIR_NAME: &str = "BingWallpaper-Rust";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 当前使用的 Provider id（P3 起支持多 Provider）
    pub provider: String,
    /// Bing 预设：china | global（决策 #9）
    pub bing_preset: String,
    pub market: String,
    /// 决策 #10：日期驱动的自动更新开关
    pub auto_update: bool,
    /// 决策 #11：开机启动（改动时同步写注册表）
    pub startup: bool,
    pub cache_days: u32,
    /// fill | fit | stretch | center | span
    pub fit_mode: String,
    /// zh | en（决策 #14）
    pub language: String,
    /// P4：Provider 在线更新源
    pub provider_repo_url: String,
    /// P4：可选 ed25519 公钥（hex），配置后远程 Manifest 强制验签
    pub provider_repo_public_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "bing".into(),
            bing_preset: "china".into(),
            market: "zh-CN".into(),
            auto_update: true,
            startup: false,
            cache_days: 30,
            fit_mode: "fill".into(),
            language: "zh".into(),
            provider_repo_url: String::new(),
            provider_repo_public_key: None,
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
