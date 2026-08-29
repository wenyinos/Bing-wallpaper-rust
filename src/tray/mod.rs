//! 系统托盘（决策 #5：tray-icon crate，方案 §15）。
//!
//! 专用线程创建托盘并泵 Win32 消息；菜单/点击事件转发为 `TrayAction`，
//! 由 eframe 主循环轮询消费（UI 侧执行动作，托盘线程不直接触碰 egui）。

use std::sync::mpsc::Sender;
use std::time::Duration;

use tracing::{error, warn};

#[cfg(windows)]
pub fn spawn(lang: crate::i18n::Lang, tx: Sender<TrayAction>) {
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            if let Err(err) = run(lang, tx) {
                error!("托盘初始化失败（后台继续运行）: {err}");
            }
        })
        .expect("启动托盘线程失败");
}

#[cfg(not(windows))]
pub fn spawn(_lang: crate::i18n::Lang, _tx: Sender<TrayAction>) {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    UpdateNow,
    Settings,
    About,
    Quit,
}

#[cfg(windows)]
fn run(lang: crate::i18n::Lang, tx: Sender<TrayAction>) -> Result<(), Box<dyn std::error::Error>> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::TrayIconBuilder;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };

    let t = crate::i18n::table(lang);

    let mut menu = Menu::new();
    let open = MenuItem::with_id("open", t.tray_open, true, None);
    let update = MenuItem::with_id("update", t.tray_update, true, None);
    let settings = MenuItem::with_id("settings", t.tray_settings, true, None);
    let about = MenuItem::with_id("about", t.tray_about, true, None);
    let quit = MenuItem::with_id("quit", t.tray_quit, true, None);
    menu.append_items(&[&open, &update, &settings, &about, &quit])?;

    let icon = solid_icon(32, 32, [36, 98, 217, 255])?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(t.tray_tooltip)
        .with_icon(icon)
        .build()?;

    // 消息泵 + 菜单事件轮询（50ms 粒度，CPU 占用可忽略）
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        for event in MenuEvent::receiver().try_iter() {
            let action = match event.id.0.as_str() {
                "open" => Some(TrayAction::Open),
                "update" => Some(TrayAction::UpdateNow),
                "settings" => Some(TrayAction::Settings),
                "about" => Some(TrayAction::About),
                "quit" => Some(TrayAction::Quit),
                _ => {
                    warn!("未知托盘菜单事件: {}", event.id.0);
                    None
                }
            };
            if let Some(action) = action {
                if tx.send(action).is_err() {
                    return Ok(()); // UI 已退出，托盘线程随之结束
                }
            }
        }

        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 无图标资源时的占位图标：运行时生成纯色 RGBA（真实 icon.ico 后续替换）
#[cfg(windows)]
fn solid_icon(
    width: u32,
    height: u32,
    rgba: [u8; 4],
) -> Result<tray_icon::Icon, Box<dyn std::error::Error>> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        data.extend_from_slice(&rgba);
    }
    Ok(tray_icon::Icon::from_rgba(data, width, height)?)
}
