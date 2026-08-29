//! BingWallpaper-Rust —— 轻量级 Rust 壁纸客户端（Windows 7+）
//!
//! v0.3：纯软件渲染 UI（Win32 原生控件 + GDI，零 GPU 依赖，替换 egui/glow）；
//! release 构建为窗口子系统（无 cmd 黑窗），日志写 %LOCALAPPDATA%\...\logs\app.log。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod cache;
mod downloader;
mod i18n;
mod icon;
mod provider;
mod scheduler;
mod system;
mod thumbs;
mod tray;
mod ui;
mod wallpaper;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tracing::{error, info};
use tracing_subscriber::fmt::writer::MakeWriterExt;

use app::config::Config;
use app::{Status, UpdateEnv};
use i18n::Lang;

fn main() {
    let data_dir = match app::config::data_dir() {
        Some(dir) => dir,
        None => {
            eprintln!("无法确定本地数据目录（%LOCALAPPDATA%）");
            return;
        }
    };
    if let Err(err) = std::fs::create_dir_all(data_dir.join("cache")) {
        eprintln!("创建数据目录失败: {err}");
        return;
    }

    // 日志：文件按天滚动（release 无控制台，stdout 仅 debug 构建使用）
    let file_appender = tracing_appender::rolling::daily(data_dir.join("logs"), "app.log");
    let (log_writer, _log_guard) = tracing_appender::non_blocking(file_appender);
    let _ = &log_writer;
    #[cfg(debug_assertions)]
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stdout.and(log_writer))
        .init();
    #[cfg(not(debug_assertions))]
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(log_writer)
        .init();

    // panic 钩子：任何线程 panic 先落日志再走默认终止（无控制台环境下唯一的现场线索）
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_hook(info);
    }));

    info!("数据目录: {}", data_dir.display());

    let cfg = Arc::new(Mutex::new(Config::load(&data_dir)));

    match system::acquire_single_instance("BingWallpaper-Rust-SingleInstance") {
        Ok(true) => {}
        Ok(false) => {
            error!("已有 BingWallpaper-Rust 实例在运行");
            system::warn_already_running();
            return;
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

    // 托盘事件通道（env 发送端 + UI/托盘接收端共用）
    let (tray_tx, tray_rx) = std::sync::mpsc::channel();

    // Provider 注册表（内置 Bing + 用户 Manifest，同 id 覆盖）
    let providers = Arc::new(Mutex::new(
        provider::manifest::load_all(&data_dir)
            .into_iter()
            .map(Arc::new)
            .collect(),
    ));

    let env = Arc::new(UpdateEnv {
        cfg,
        data_dir: data_dir.clone(),
        status: status.clone(),
        last_fetch: Arc::new(Mutex::new(Vec::new())),
        fetch_cursor: Arc::new(Mutex::new(0)),
        providers,
        events_tx: tray_tx.clone(),
        provider_check_msg: Arc::new(Mutex::new(None)),
        ui_dirty: Arc::new(AtomicBool::new(false)),
        rt: rt_handle,
    });

    // 系统托盘（决策 #5）
    tray::spawn(lang, tray_tx);

    // 开机自启动带 --minimized：主窗口隐藏，仅托盘驻留（方案 §15/§30）
    let minimized = std::env::args().any(|arg| arg == "--minimized");

    // 定时调度：日期驱动更新 + 7 天内壁纸自动轮换
    app::start_scheduler(&env);

    // UI 消息循环（阻塞；退出仅经托盘"退出"）
    ui::run(env, tray_rx, lang, !minimized);
}
