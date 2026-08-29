//! 应用图标：编译期嵌入 assets/icon.ico，运行时解码为 RGBA。
//!
//! 同时服务三处：exe 资源图标（build.rs/winresource）、窗口图标（egui）、托盘图标（tray-icon）。

pub const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.ico");

/// 解码为 RGBA 位图；解码失败返回 None（调用方回退占位图标）
pub fn decoded() -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory_with_format(ICON_BYTES, image::ImageFormat::Ico).ok()?;
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}
