//! 应用核心（UI 无关）：共享环境、更新/轮换任务、Provider 检查。
//!
//! v0.3：界面从 egui 迁移到 Win32 原生控件（纯软件渲染），
//! 本模块不再依赖任何 GUI 类型，跨平台可编译（供本机 check 验证）。

pub mod config;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::runtime::Handle;
use tracing::{error, info};

use crate::cache::CacheManager;
use crate::downloader::Downloader;
use crate::i18n::Lang;
use crate::provider::manifest::LoadedProvider;
use crate::provider::{ProviderContext, Wallpaper};
use crate::scheduler;
use crate::tray::TrayAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    pub message: String,
    pub last_set: Option<String>,
}

impl Status {
    pub fn idle(lang: Lang) -> Self {
        Self {
            running: false,
            message: crate::i18n::table(lang).status_ready.into(),
            last_set: None,
        }
    }
}

/// 更新动作共享环境：UI / 托盘 / 调度器三方共用的状态集合
#[derive(Clone)]
pub struct UpdateEnv {
    pub cfg: Arc<Mutex<config::Config>>,
    pub data_dir: PathBuf,
    pub status: Arc<Mutex<Status>>,
    /// 最近一次 fetch 的壁纸列表（"下一张"/自动轮换数据源）
    pub last_fetch: Arc<Mutex<Vec<Wallpaper>>>,
    pub fetch_cursor: Arc<Mutex<usize>>,
    /// Provider 注册表（内置 Bing + 用户 Manifest，同 id 覆盖）
    pub providers: Arc<Mutex<Vec<Arc<LoadedProvider>>>>,
    /// 事件通道（托盘事件 + Provider 重载请求）
    pub events_tx: std::sync::mpsc::Sender<TrayAction>,
    /// Provider 更新检查结果消息（后台写、UI 读）
    pub provider_check_msg: Arc<Mutex<Option<(String, Instant)>>>,
    /// 后台任务完成置位，UI 定时器轮询刷新
    pub ui_dirty: Arc<AtomicBool>,
    /// tokio runtime 句柄（后台网络任务）
    pub rt: Handle,
}

impl UpdateEnv {
    pub fn notify_ui(&self) {
        self.ui_dirty.store(true, Ordering::Relaxed);
    }
}

/// 供托盘 / 调度器 / UI 按钮共用的更新触发入口
pub fn spawn_update(env: &Arc<UpdateEnv>, lang: Lang) {
    let env = env.clone();
    let rt = env.rt.clone();
    rt.spawn(update_task(env, lang));
}

/// "下一张"：轮换最近 fetch 列表中的壁纸（7 天窗口内，方案托盘"下一张"）
pub fn spawn_next(env: &Arc<UpdateEnv>, lang: Lang) {
    let env = env.clone();
    let rt = env.rt.clone();
    rt.spawn(next_task(env, lang));
}

async fn update_task(env: Arc<UpdateEnv>, lang: Lang) {
    let t = crate::i18n::table(lang);
    set_status(&env, true, t.status_updating.into());
    match run_update(&env).await {
        Ok((title, date)) => {
            info!("壁纸更新完成: {title}（{date}）");
            if let Ok(mut s) = env.status.lock() {
                s.running = false;
                s.message = format!("{}{title}", t.status_done_prefix);
                s.last_set = Some(date);
            }
            env.notify_ui();
        }
        Err(err) => {
            error!("壁纸更新失败: {err}");
            set_status(&env, false, format!("{}{err}", t.status_failed_prefix));
        }
    }
}

async fn next_task(env: Arc<UpdateEnv>, lang: Lang) {
    let t = crate::i18n::table(lang);
    set_status(&env, true, t.status_updating.into());
    match run_next(&env).await {
        Ok((title, date)) => {
            info!("切换壁纸: {title}（{date}）");
            if let Ok(mut s) = env.status.lock() {
                s.running = false;
                s.message = format!("{}{title}", t.status_done_prefix);
                s.last_set = Some(date);
            }
            env.notify_ui();
        }
        Err(err) => {
            error!("切换壁纸失败: {err}");
            set_status(&env, false, format!("{}{err}", t.status_failed_prefix));
        }
    }
}

fn set_status(env: &Arc<UpdateEnv>, running: bool, message: String) {
    if let Ok(mut s) = env.status.lock() {
        s.running = running;
        s.message = message;
    }
    env.notify_ui();
}

type UpdateError = Box<dyn std::error::Error + Send + Sync>;

async fn http_client() -> Result<reqwest::Client, UpdateError> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("BingWallpaper-Rust/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

/// 从注册表选择 Provider：按 cfg.provider 匹配，未命中回退第一个（内置 Bing）
fn select_provider(
    providers: &[Arc<LoadedProvider>],
    wanted: &str,
) -> Result<Arc<LoadedProvider>, UpdateError> {
    providers
        .iter()
        .find(|p| p.manifest.id == wanted)
        .or_else(|| providers.first())
        .cloned()
        .ok_or("Provider 注册表为空".into())
}

/// 下载（或缓存命中）并把单张壁纸设为桌面
async fn apply_wallpaper(
    provider_id: &str,
    wp: &Wallpaper,
    http: reqwest::Client,
    cache: &CacheManager,
    fit_mode: &str,
) -> Result<(String, String), UpdateError> {
    let dest = cache.path_for(provider_id, &wp.id);
    let mut bytes: u64 = 0;
    if dest.exists() {
        info!("缓存命中: {}", dest.display());
    } else {
        tokio::fs::create_dir_all(cache.dir().join(provider_id)).await?;
        bytes = Downloader { http }
            .download_to_file(&wp.image_url, &dest)
            .await?;
    }

    crate::wallpaper::set_wallpaper(&dest, fit_mode)?;
    cache.record_download(provider_id, wp, bytes, CacheManager::today());

    let title = if wp.title.is_empty() {
        wp.id.clone()
    } else {
        wp.title.clone()
    };
    let date = wp
        .published_at
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    Ok((title, date))
}

async fn run_update(env: &Arc<UpdateEnv>) -> Result<(String, String), UpdateError> {
    let snapshot = env
        .cfg
        .lock()
        .map(|c| c.clone())
        .map_err(|_| "配置状态异常")?;
    let selected = {
        let guard = env.providers.lock().map_err(|_| "状态异常")?;
        select_provider(&guard, &snapshot.provider)
    }?;
    let http = http_client().await?;
    let context = ProviderContext { http: http.clone() };
    let wallpapers = selected.provider.fetch(&context).await?;
    if let Ok(mut slot) = env.last_fetch.lock() {
        *slot = wallpapers.clone();
    }
    let wp = wallpapers.first().ok_or("Provider 未返回任何壁纸")?;
    let cache = CacheManager::new(&env.data_dir);
    apply_wallpaper(&selected.manifest.id, wp, http, &cache, &snapshot.fit_mode).await
}

async fn run_next(env: &Arc<UpdateEnv>) -> Result<(String, String), UpdateError> {
    let snapshot = env
        .cfg
        .lock()
        .map(|c| c.clone())
        .map_err(|_| "配置状态异常")?;
    let mut list = env
        .last_fetch
        .lock()
        .map(|l| l.clone())
        .map_err(|_| "状态异常")?;
    if list.is_empty() {
        let selected = {
            let guard = env.providers.lock().map_err(|_| "状态异常")?;
            select_provider(&guard, &snapshot.provider)
        }?;
        let http = http_client().await?;
        let context = ProviderContext { http };
        list = selected.provider.fetch(&context).await?;
        if let Ok(mut slot) = env.last_fetch.lock() {
            *slot = list.clone();
        }
    }
    // 轮换窗口：仅最近 7 天内的壁纸（空则回退全部，功能需求）
    let window_start = CacheManager::today() - chrono::Duration::days(6);
    let recent: Vec<Wallpaper> = list
        .iter()
        .filter(|wp| {
            wp.published_at
                .map(|d| d.date_naive() >= window_start)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    if !recent.is_empty() {
        list = recent;
    }
    let index = {
        let mut c = env.fetch_cursor.lock().map_err(|_| "状态异常")?;
        let i = *c % list.len();
        *c = i + 1;
        i
    };
    let wp = list[index].clone();
    let selected = {
        let guard = env.providers.lock().map_err(|_| "状态异常")?;
        select_provider(&guard, &snapshot.provider)
    }?;
    let cache = CacheManager::new(&env.data_dir);
    let http = http_client().await?;
    apply_wallpaper(&selected.manifest.id, &wp, http, &cache, &snapshot.fit_mode).await
}

/// P4：检查 Provider 在线更新（方案 §9 第二阶段）
pub fn spawn_provider_check(env: &Arc<UpdateEnv>, lang: Lang) {
    let t = crate::i18n::table(lang);
    if let Ok(mut slot) = env.provider_check_msg.lock() {
        *slot = Some((t.provider_checking.into(), Instant::now()));
    }
    let env = env.clone();
    let lang = lang;
    let rt = env.rt.clone();
    rt.spawn(async move {
        let snapshot = match env.cfg.lock() {
            Ok(cfg) => cfg.clone(),
            Err(err) => {
                error!("Provider 更新检查失败: {err}");
                env.notify_ui();
                return;
            }
        };
        let t = crate::i18n::table(lang);
        let (message, has_updates) = match http_client().await {
            Err(err) => (format!("{}{err}", t.status_failed_prefix), false),
            Ok(http) => {
                match crate::provider::repo::check_for_updates(
                    &http,
                    &snapshot.provider_repo_url,
                    snapshot.provider_repo_public_key.as_deref(),
                    &env.data_dir,
                )
                .await
                {
                    Ok(report) => {
                        info!("Provider 更新检查完成: {report}");
                        if report.updated.is_empty() {
                            (t.provider_up_to_date.into(), false)
                        } else {
                            (format!("{}: {report}", t.provider_check_update), true)
                        }
                    }
                    Err(err) => {
                        error!("Provider 更新检查失败: {err}");
                        (format!("{}{err}", t.status_failed_prefix), false)
                    }
                }
            }
        };
        if let Ok(mut slot) = env.provider_check_msg.lock() {
            *slot = Some((message, Instant::now()));
        }
        if has_updates {
            let _ = env.events_tx.send(TrayAction::ReloadProviders);
        }
        env.notify_ui();
    });
}

/// 启动定时调度（决策 #10 日期驱动 + 7 天轮换）
pub fn start_scheduler(env: &Arc<UpdateEnv>) {
    let lang = Lang::parse(
        &env.cfg
            .lock()
            .map(|c| c.language.clone())
            .unwrap_or_default(),
    );
    scheduler::spawn(scheduler::SchedulerDeps {
        env: env.clone(),
        lang,
    });
}
