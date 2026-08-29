//! 定时调度（决策 #10：按"壁纸日期"驱动 + 7 天内自动轮换）。
//!
//! - 启动 30 秒后首查今日壁纸；此后每 5 分钟醒来比对日期（睡眠唤醒自适应）
//! - `rotate_minutes > 0` 时，按间隔在最近 7 天壁纸间自动轮换（实时切换）
//! - 轮换间隔以进程内计时为准（重启重置）

use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{debug, info};

use crate::app::{spawn_next, spawn_update, UpdateEnv};
use crate::cache::CacheManager;
use crate::i18n::Lang;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(300);

pub struct SchedulerDeps {
    pub env: Arc<UpdateEnv>,
    pub lang: Lang,
}

pub fn spawn(deps: SchedulerDeps) {
    let env = deps.env.clone();
    let lang = deps.lang;
    let data_dir = env.data_dir.clone();

    std::thread::Builder::new()
        .name("scheduler".into())
        .spawn(move || {
            let cache = CacheManager::new(&data_dir);
            std::thread::sleep(FIRST_CHECK_DELAY);
            let mut last_rotate = Instant::now();
            loop {
                let (auto, cache_days, rotate_minutes) = match env.cfg.lock() {
                    Ok(cfg) => (cfg.auto_update, cfg.cache_days, cfg.rotate_minutes),
                    Err(_) => (false, 30, 0),
                };
                if auto && !cache.is_today_set() {
                    info!("调度器：检测到今日壁纸未设置，触发更新");
                    spawn_update(&env, lang);
                } else {
                    debug!("调度器：今日壁纸已设置或自动更新关闭，跳过");
                }
                // 7 天内壁纸自动轮换（rotate_minutes = 0 关闭）
                if rotate_minutes > 0
                    && last_rotate.elapsed() >= Duration::from_secs(rotate_minutes as u64 * 60)
                {
                    info!("调度器：触发最近 7 天壁纸轮换");
                    spawn_next(&env, lang);
                    last_rotate = Instant::now();
                }
                cache.cleanup_daily(cache_days);
                std::thread::sleep(CHECK_INTERVAL);
            }
        })
        .expect("启动调度线程失败");
}
