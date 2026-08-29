//! 缓存系统（方案 §11）：图片文件 + JSON 索引（决策 #7，不使用 SQLite）。
//!
//! 目录结构：
//! ```text
//! cache/
//! ├── index.json        索引（含 last_set，调度器按"壁纸日期驱动"依赖它）
//! ├── bing/             按 Provider 分目录
//! │   └── <id>.jpg
//! └── thumbnails/       P2：历史壁纸缩略图
//! ```

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::provider::Wallpaper;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub provider: String,
    pub wallpaper_id: String,
    /// 相对 cache 目录的文件路径（含 provider 子目录）
    pub file: String,
    pub title: Option<String>,
    pub date: Option<NaiveDate>,
    pub bytes: u64,
    pub added_at: DateTime<Utc>,
}

/// 调度器依赖的"上次设置"记录（决策 #10：按壁纸日期驱动）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSet {
    pub provider: String,
    pub wallpaper_id: String,
    pub date: NaiveDate,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Index {
    #[serde(default)]
    pub entries: Vec<CacheEntry>,
    #[serde(default)]
    pub last_set: Option<LastSet>,
    /// 最近一次执行过期清理的日期（每日一次去重）
    #[serde(default)]
    pub last_cleanup: Option<NaiveDate>,
}

pub struct CacheManager {
    dir: PathBuf,
}

impl CacheManager {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("cache"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    pub fn load_index(&self) -> Index {
        match std::fs::read_to_string(self.index_path()) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(index) => index,
                Err(err) => {
                    warn!("cache/index.json 解析失败，重建索引: {err}");
                    Index::default()
                }
            },
            Err(_) => Index::default(),
        }
    }

    /// 原子写盘（tmp + rename），避免写一半损坏索引
    pub fn save_index(&self, index: &Index) {
        let tmp = self.dir.join("index.json.part");
        match serde_json::to_string_pretty(index) {
            Ok(text) => {
                if let Err(err) = std::fs::write(&tmp, text) {
                    warn!("写缓存索引失败: {err}");
                    return;
                }
                if let Err(err) = std::fs::rename(&tmp, self.index_path()) {
                    warn!("替换缓存索引失败: {err}");
                }
            }
            Err(err) => warn!("序列化缓存索引失败: {err}"),
        }
    }

    /// 约定缓存文件路径：cache/<provider>/<id>.jpg
    pub fn path_for(&self, provider: &str, wallpaper_id: &str) -> PathBuf {
        self.dir.join(provider).join(format!("{wallpaper_id}.jpg"))
    }

    /// 下载完成后登记索引（含 last_set 更新）
    pub fn record_download(&self, provider: &str, wp: &Wallpaper, bytes: u64, set_date: NaiveDate) {
        let mut index = self.load_index();
        let file = format!("{provider}/{wallpaper_id}.jpg", wallpaper_id = wp.id);
        let entry = CacheEntry {
            provider: provider.to_string(),
            wallpaper_id: wp.id.clone(),
            file,
            title: if wp.title.is_empty() {
                None
            } else {
                Some(wp.title.clone())
            },
            date: wp.published_at.map(|d| d.date_naive()),
            bytes,
            added_at: Utc::now(),
        };
        index
            .entries
            .retain(|e| !(e.provider == provider && e.wallpaper_id == wp.id));
        index.entries.push(entry);
        index.last_set = Some(LastSet {
            provider: provider.to_string(),
            wallpaper_id: wp.id.clone(),
            date: set_date,
        });
        self.save_index(&index);
    }

    /// 按天数清理过期缓存（方案 §11：默认 30 天），返回删除数量
    pub fn cleanup(&self, days: u32) -> usize {
        let index = self.load_index();
        let last_set = index.last_set.clone();
        let last_cleanup = index.last_cleanup;
        let deadline = Utc::now() - chrono::Duration::days(days as i64);
        let (keep, expired): (Vec<_>, Vec<_>) = index
            .entries
            .into_iter()
            .partition(|e| e.added_at > deadline);

        let mut removed = 0;
        for entry in &expired {
            let path = self.dir.join(&entry.file);
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => warn!("删除过期缓存失败 {}: {err}", path.display()),
            }
        }
        if removed > 0 {
            debug!("缓存清理：删除 {removed} 个过期文件");
            self.save_index(&Index {
                entries: keep,
                last_set,
                last_cleanup,
            });
        }
        removed
    }

    /// 每日一次的过期清理（去重），调度器每日循环调用
    pub fn cleanup_daily(&self, days: u32) -> usize {
        let mut index = self.load_index();
        let today = Self::today();
        if index.last_cleanup == Some(today) {
            return 0;
        }
        let removed = self.cleanup(days);
        index.last_cleanup = Some(today);
        self.save_index(&index);
        removed
    }

    pub fn entries(&self) -> Vec<CacheEntry> {
        self.load_index().entries
    }

    /// 用户手动设置某张缓存壁纸后，仅更新 last_set（日期记为当天，
    /// 使调度器当日不再用新壁纸覆盖用户选择，见决策 #10 语义）
    pub fn record_last_set(&self, provider: &str, wallpaper_id: &str) {
        let mut index = self.load_index();
        index.last_set = Some(LastSet {
            provider: provider.to_string(),
            wallpaper_id: wallpaper_id.to_string(),
            date: Self::today(),
        });
        self.save_index(&index);
    }

    /// 历史页删除：移除文件与索引项
    pub fn remove_entry(&self, provider: &str, wallpaper_id: &str) {
        let mut index = self.load_index();
        if let Some(pos) = index
            .entries
            .iter()
            .position(|e| e.provider == provider && e.wallpaper_id == wallpaper_id)
        {
            let entry = index.entries.remove(pos);
            let path = self.dir.join(&entry.file);
            if let Err(err) = std::fs::remove_file(&path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    warn!("删除缓存文件失败 {}: {err}", path.display());
                }
            }
            self.save_index(&index);
        }
    }

    pub fn thumbnail_dir(&self) -> PathBuf {
        self.dir.join("thumbnails")
    }

    pub fn today() -> NaiveDate {
        Local::now().date_naive()
    }

    /// 供调度器使用的判定：今天是否已有设置记录
    pub fn is_today_set(&self) -> bool {
        match self.load_index().last_set {
            Some(ls) => ls.date == Self::today(),
            None => false,
        }
    }
}
