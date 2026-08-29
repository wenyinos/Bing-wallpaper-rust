//! Win32 壁纸设置：SystemParametersInfoW + 注册表 WallpaperStyle（方案 §12）。

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
};

use super::{FitMode, WallpaperError};
use crate::system::to_wide;

const DESKTOP_KEY: &str = "Control Panel\\Desktop";

/// (WallpaperStyle, TileWallpaper) 的注册表取值（Windows 官方文档）
fn style_values(fit: FitMode) -> (&'static str, &'static str) {
    match fit {
        FitMode::Fill => ("10", "0"),
        FitMode::Fit => ("6", "0"),
        FitMode::Stretch => ("2", "0"),
        FitMode::Center => ("0", "0"),
        FitMode::Span => ("22", "0"),
    }
}

/// Win7 兼容路径：SystemParametersInfoW + 注册表 WallpaperStyle（方案 §12/§13 决策）
pub fn set_via_systemparams(path: &Path, fit: FitMode) -> Result<(), WallpaperError> {
    let path = path.to_string_lossy().into_owned();
    let wide = to_wide(&path);
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            wide.as_ptr() as *mut core::ffi::c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };
    if ok == 0 {
        return Err(WallpaperError::Api(unsafe { GetLastError() }));
    }
    set_desktop_style(fit)
}

fn set_desktop_style(fit: FitMode) -> Result<(), WallpaperError> {
    let (style, tile) = style_values(fit);
    let mut hkey: HKEY = std::ptr::null_mut();
    let sub_key = to_wide(DESKTOP_KEY);
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub_key.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if code != 0 {
        return Err(WallpaperError::Registry {
            key: DESKTOP_KEY,
            code,
        });
    }

    let result = (|| {
        for (name, value) in [("WallpaperStyle", style), ("TileWallpaper", tile)] {
            let name_wide = to_wide(name);
            let mut data = to_wide(value); // to_wide 已含 trailing NUL，符合 REG_SZ
            let code = unsafe {
                RegSetValueExW(
                    hkey,
                    name_wide.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            if code != 0 {
                return Err(WallpaperError::Registry { key: name, code });
            }
        }
        Ok(())
    })();

    unsafe { RegCloseKey(hkey) };
    result
}
