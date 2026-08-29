//! 定时调度（决策 #10：按"壁纸日期"驱动）。
//!
//! 触发点与动作：
//! - 启动后 30 秒：检查今日壁纸是否已设置
//! - 内部每 5 分钟醒来比对日期（覆盖睡眠唤醒/跨天，期间零网络请求）
//! - 日期变化且 auto_update 开启时，才联网拉取并设置
//!
//! 手动更新不经过调度器（直接由 UI/托盘触发）。

use std::sync::Arc;
use std::time::Duration;

use eframe::egui;
use tokio::runtime::Handle;
use tracing::{debug, info};

use crate::app::spawn_update;
use crate::app::UpdateEnv;
use crate::cache::CacheManager;
use crate::i18n::Lang;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(300);

pub struct SchedulerDeps {
    pub rt: Handle,
    pub env: Arc<UpdateEnv>,
    pub ctx: egui::Context,
}

pub fn spawn(deps: SchedulerDeps) {
    let env = deps.env.clone();
    let lang = Lang::parse(
        &env.cfg
            .lock()
            .map(|c| c.language.clone())
            .unwrap_or_default(),
    );
    let data_dir = env.data_dir.clone();

    std::thread::Builder::new()
        .name("scheduler".into())
        .spawn(move || {
            let cache = CacheManager::new(&data_dir);
            std::thread::sleep(FIRST_CHECK_DELAY);
            loop {
                let (auto, cache_days) = match env.cfg.lock() {
                    Ok(cfg) => (cfg.auto_update, cfg.cache_days),
                    Err(_) => (false, 30),
                };
                if auto && !cache.is_today_set() {
                    info!("调度器：检测到今日壁纸未设置，触发更新");
                    spawn_update(&deps.rt, &env, &deps.ctx, lang);
                } else {
                    debug!("调度器：今日壁纸已设置或自动更新关闭，跳过");
                }
                // 顺带做每日一次的过期缓存清理
                cache.cleanup_daily(cache_days);
                std::thread::sleep(CHECK_INTERVAL);
            }
        })
        .expect("启动调度线程失败");
}
