//! BingWallpaper-Rust —— 轻量级 Rust 壁纸客户端（Windows 7+）
//!
//! P1 范围：P0 基础上 + i18n / 缓存索引 / 定时调度 / 系统托盘 / 开机启动 / 设置页。
//! `--minimized` 参数：启动时隐藏主窗口（开机自启动场景，方案 §30）。

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
mod ui_font;
mod wallpaper;

use std::sync::{Arc, Mutex};

use tracing::{error, info};
use tracing_subscriber::fmt::writer::MakeWriterExt;

use app::config::Config;
use app::{App, Status};
use i18n::Lang;

/// 窗口图标：解码嵌入的 icon.ico；失败回退纯色占位
fn runtime_icon() -> eframe::egui::IconData {
    if let Some((width, height, rgba)) = icon::decoded() {
        return eframe::egui::IconData {
            width,
            height,
            rgba,
        };
    }
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
    let (log_writer, _log_guard) = tracing_appender::non_blocking(file_appender);
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

    // 系统托盘事件通道：env 需要发送端（P4 重载事件），先建通道再构建 env
    let (tray_tx, tray_rx) = std::sync::mpsc::channel();

    // Provider 注册表（P3：内置 Bing + 用户 Manifest，同 id 覆盖）
    let providers = Arc::new(Mutex::new(
        provider::manifest::load_all(&data_dir)
            .into_iter()
            .map(Arc::new)
            .collect(),
    ));

    let env = Arc::new(app::UpdateEnv {
        cfg,
        data_dir: data_dir.clone(),
        status: status.clone(),
        last_fetch: Arc::new(Mutex::new(Vec::new())),
        fetch_cursor: Arc::new(Mutex::new(0)),
        providers,
        events_tx: tray_tx.clone(),
        provider_check_msg: Arc::new(Mutex::new(None)),
    });

    // 系统托盘（决策 #5）
    tray::spawn(lang, tray_tx);

    // 开机自启动带 --minimized：主窗口隐藏，仅托盘驻留（方案 §15/§30）
    let minimized = std::env::args().any(|arg| arg == "--minimized");

    // 渲染稳定性硬化：VM / 远程桌面 / 老驱动下，MSAA、depth/stencil 缓冲与
    // 垂直同步是黑屏花屏三大根因；egui 自带 feathering 抗锯齿不依赖它们。
    // 注意不用 HardwareAcceleration::Off——WGL 软件像素格式只有 OpenGL 1.1，
    // glow 无法工作，反而导致初始化失败。
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("BingWallpaper-Rust")
            .with_inner_size([680.0, 480.0])
            .with_icon(runtime_icon())
            .with_visible(!minimized),
        renderer: eframe::Renderer::Glow,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        vsync: false,
        ..Default::default()
    };

    if let Err(err) = eframe::run_native(
        "BingWallpaper-Rust",
        options,
        Box::new(move |cc| {
            // eframe 0.27 的 app_creator 返回 Box<dyn App>（0.28 起才是 Result）
            Box::new(App::new(env, rt_handle, tray_rx, cc.egui_ctx.clone())) as Box<dyn eframe::App>
        }),
    ) {
        // 常见原因：虚拟机 SVGA / 远程桌面（GDI Generic 仅 GL 1.1）/ 老显卡驱动
        let msg = format!("界面初始化失败（当前环境可能不支持 OpenGL 2.0+）: {err}");
        error!("{msg}");
        eprintln!("{msg}");
        eprintln!("提示：虚拟机请启用 3D 加速，远程桌面建议本地登录后使用。");
        std::process::exit(1);
    }
    Ok(())
}
