//! BingWallpaper-Rust —— 轻量级 Rust 壁纸客户端（Windows 7+）
//!
//! P0 范围：基础窗口 + Bing Provider + 图片下载 + 设置壁纸 + 单实例。
//! 托盘 / 定时更新 / 配置 UI 为 P1（见方案文档 §39）。

mod app;
mod downloader;
mod provider;
mod system;
mod wallpaper;

use std::sync::{Arc, Mutex};

use tracing::{error, info};

use app::config::Config;
use app::{App, Status};

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let data_dir = match app::config::data_dir() {
        Some(dir) => dir,
        None => {
            error!("无法确定本地数据目录（%LOCALAPPDATA%）");
            return Ok(());
        }
    };
    if let Err(err) = std::fs::create_dir_all(data_dir.join("cache")) {
        error!("创建数据目录失败: {err}");
        return Ok(());
    }
    info!("数据目录: {}", data_dir.display());

    let cfg = Config::load(&data_dir);

    match system::acquire_single_instance("BingWallpaper-Rust-SingleInstance") {
        Ok(true) => {}
        Ok(false) => {
            error!("已有 BingWallpaper-Rust 实例在运行");
            system::warn_already_running();
            return Ok(());
        }
        Err(err) => {
            // 互斥体创建失败不阻塞启动，只记录（避免系统状态差时完全不可用）
            error!("单实例检测失败: {err}");
        }
    }

    // tokio runtime 常驻后台线程，为 Provider/Downloader 提供异步执行环境
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("创建 tokio runtime 失败");
    let rt_handle = runtime.handle().clone();
    std::thread::Builder::new()
        .name("tokio-runtime".into())
        .spawn(move || runtime.block_on(std::future::pending::<()>()))
        .expect("启动 tokio 运行线程失败");

    let status = Arc::new(Mutex::new(Status::idle()));

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("BingWallpaper-Rust")
            .with_inner_size([640.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(
        "BingWallpaper-Rust",
        options,
        Box::new(move |cc| {
            Ok(Box::new(App::new(
                cfg,
                data_dir,
                rt_handle,
                status,
                cc.egui_ctx.clone(),
            )))
        }),
    )
}
