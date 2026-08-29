//! Win10+ 壁纸接口 IDesktopWallpaper（P2 多显示器基础；Win7 无此接口，由调用方回退）。

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL,
    DWPOS_FIT, DWPOS_SPAN, DWPOS_STRETCH,
};

use super::{FitMode, WallpaperError};
use crate::system::to_wide;

/// 通过 IDesktopWallpaper 设置全部显示器的壁纸（方案 §14 第一阶段：所有屏同一张）。
/// Win7 无此 COM 组件，CoCreateInstance 失败属预期，调用方应回退 SystemParametersInfoW。
pub fn set_via_desktop_wallpaper(path: &Path, fit: FitMode) -> Result<(), WallpaperError> {
    unsafe {
        // 线程可能已初始化 COM（winit/eframe）；此处仅确保，成败由 CoCreateInstance 判定
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let wallpaper: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)
            .map_err(|err| WallpaperError::Com(err.to_string()))?;

        let wide = to_wide(&path.to_string_lossy());
        // wallpaperid = NULL：作用于所有监视器
        wallpaper
            .SetWallpaper(PCWSTR::null(), PCWSTR(wide.as_ptr()))
            .map_err(|err| WallpaperError::Com(err.to_string()))?;
        wallpaper
            .SetPosition(position_of(fit))
            .map_err(|err| WallpaperError::Com(err.to_string()))?;
    }
    Ok(())
}

fn position_of(fit: FitMode) -> DESKTOP_WALLPAPER_POSITION {
    match fit {
        FitMode::Fill => DWPOS_FILL,
        FitMode::Fit => DWPOS_FIT,
        FitMode::Stretch => DWPOS_STRETCH,
        FitMode::Center => DWPOS_CENTER,
        FitMode::Span => DWPOS_SPAN,
    }
}
