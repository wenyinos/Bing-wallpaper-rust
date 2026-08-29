//! eframe 主窗口（P0：状态展示 + 立即更新；P0 UI 文本暂写死中文，P1 做 i18n）。

pub mod config;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui;
use tokio::runtime::Handle;
use tracing::{error, info};

use crate::downloader::Downloader;
use crate::provider::bing::BingProvider;
use crate::provider::{ProviderContext, WallpaperProvider};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    pub message: String,
    pub last_set: Option<String>,
}

impl Status {
    pub fn idle() -> Self {
        Self {
            running: false,
            message: "就绪，正在获取今日壁纸…".into(),
            last_set: None,
        }
    }
}

pub struct App {
    cfg: config::Config,
    data_dir: PathBuf,
    rt: Handle,
    status: Arc<Mutex<Status>>,
}

impl App {
    pub fn new(
        cfg: config::Config,
        data_dir: PathBuf,
        rt: Handle,
        status: Arc<Mutex<Status>>,
        ctx: egui::Context,
    ) -> Self {
        // 启动即拉取今日壁纸（缓存命中则零下载）
        let task = update_task(cfg.clone(), data_dir.clone(), status.clone(), ctx);
        rt.spawn(task);
        Self {
            cfg,
            data_dir,
            rt,
            status,
        }
    }
}

fn set_status(status: &Arc<Mutex<Status>>, running: bool, message: String, ctx: &egui::Context) {
    if let Ok(mut s) = status.lock() {
        s.running = running;
        s.message = message;
    }
    ctx.request_repaint();
}

async fn update_task(
    cfg: config::Config,
    data_dir: PathBuf,
    status: Arc<Mutex<Status>>,
    ctx: egui::Context,
) {
    set_status(&status, true, "正在获取 Bing 壁纸列表…".into(), &ctx);
    match run_update(&cfg, &data_dir).await {
        Ok((title, date)) => {
            info!("壁纸更新完成: {title}（{date}）");
            if let Ok(mut s) = status.lock() {
                s.running = false;
                s.message = format!("已设置：{title}");
                s.last_set = Some(date);
            }
            ctx.request_repaint();
        }
        Err(err) => {
            error!("壁纸更新失败: {err}");
            set_status(
                &status,
                false,
                format!("更新失败：{err}（可在故障排除后点击"立即更新"重试）"),
                &ctx,
            );
        }
    }
}

type UpdateError = Box<dyn std::error::Error + Send + Sync>;

/// Provider -> 下载 -> 缓存 -> 设置壁纸 的最小数据流（方案 §2 统一数据流）
async fn run_update(
    cfg: &config::Config,
    data_dir: &Path,
) -> Result<(String, String), UpdateError> {
    let http = reqwest::Client::builder()
        .user_agent(concat!("BingWallpaper-Rust/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let provider = BingProvider::from_preset(&cfg.bing_preset);
    let context = ProviderContext { http: http.clone() };
    let wallpapers = provider.fetch(&context).await?;
    let wp = wallpapers.first().ok_or("Provider 未返回任何壁纸")?;

    let cache_dir = data_dir.join("cache");
    tokio::fs::create_dir_all(&cache_dir).await?;
    let dest = cache_dir.join(format!("bing_{}.jpg", wp.id));
    if dest.exists() {
        info!("缓存命中: {}", dest.display());
    } else {
        Downloader { http }
            .download_to_file(&wp.image_url, &dest)
            .await?;
    }

    crate::wallpaper::set_wallpaper(&dest, &cfg.fit_mode)?;

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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let status = self.status.lock().map(|s| s.clone()).unwrap_or_else(|_| {
            // 锁中毒（后台任务 panic）时给出可见反馈，不静默
            Status {
                running: false,
                message: "内部状态异常，请重启应用".into(),
                last_set: None,
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("BingWallpaper-Rust");
            ui.separator();
            ui.add_space(8.0);
            ui.label(&status.message);
            if let Some(last) = &status.last_set {
                ui.label(format!("上次设置：{last}"));
            }
            ui.add_space(16.0);
            ui.add_enabled_ui(!status.running, |ui| {
                if ui.button("立即更新").clicked() {
                    let task = update_task(
                        self.cfg.clone(),
                        self.data_dir.clone(),
                        self.status.clone(),
                        ctx.clone(),
                    );
                    self.rt.spawn(task);
                }
            });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(format!(
                    "来源：Bing（预设 {}）    数据目录：{}",
                    self.cfg.bing_preset,
                    self.data_dir.display()
                ));
            });
        });
    }
}
