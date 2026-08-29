//! BingWallpaper-Rust —— 轻量级 Rust 壁纸客户端（Windows 7+）
//!
//! P1 范围：P0 基础上 + i18n / 缓存索引 / 定时调度 / 系统托盘 / 开机启动 / 设置页。
//! `--minimized` 参数：启动时隐藏主窗口（开机自启动场景，方案 §30）。

mod app;
mod autostart;
mod cache;
mod downloader;
mod i18n;
mod provider;
mod scheduler;
mod system;
mod tray;
mod wallpaper;

use std::sync::{Arc, Mutex};

use tracing::{error, info};
use tracing_subscriber::fmt::writer::MakeWriterExt;

use app::config::Config;
use app::{App, Status};
use i18n::Lang;

/// 占位应用图标：运行时生成纯色 RGBA（真实 icon.ico 后续替换）
fn runtime_icon() -> eframe::egui::IconData {
    let (width, height) = (32u32, 32u32);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        rgba.extend_from_slice(&[36, 98, 217, 255]);
    }
    eframe::egui::IconData {
        width,
        height,
        rgba,
    }
}

fn main() -> eframe::Result<()> {
    let data_dir = match app::config::data_dir() {
        Some(dir) => dir,
        None => {
            eprintln!("无法确定本地数据目录（%LOCALAPPDATA%）");
            return Ok(());
        }
    };
    if let Err(err) = std::fs::create_dir_all(data_dir.join("cache")) {
        eprintln!("创建数据目录失败: {err}");
        return Ok(());
    }

    // 日志：stdout + 按天滚动文件（方案 §18）；guard 必须存活到进程结束
    let file_appender = tracing_appender::rolling::daily(data_dir.join("logs"), "app.log");
    let (log_writer, log_guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stdout.and(log_writer))
        .init();

    info!("数据目录: {}", data_dir.display());

    let cfg = Arc::new(Mutex::new(Config::load(&data_dir)));

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

    let lang = Lang::parse(&cfg.lock().map(|c| c.language.clone()).unwrap_or_default());
    let status = Arc::new(Mutex::new(Status::idle(lang)));

    // 系统托盘（决策 #5）
    let (tray_tx, tray_rx) = std::sync::mpsc::channel();
    tray::spawn(lang, tray_tx);

    // 开机自启动带 --minimized：主窗口隐藏，仅托盘驻留（方案 §15/§30）
    let minimized = std::env::args().any(|arg| arg == "--minimized");

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("BingWallpaper-Rust")
            .with_inner_size([680.0, 480.0])
            .with_icon(runtime_icon())
            .with_visible(!minimized),
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
                tray_rx,
                tray_tx,
                cc.egui_ctx.clone(),
            )))
        }),
    )
}
