//! 开机自启动（方案 §30）：HKCU\...\Run 注册表值，无需管理员权限，Win7 兼容。

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

use crate::system::to_wide;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "BingWallpaper-Rust";

#[derive(Debug, thiserror::Error)]
pub enum AutostartError {
    #[error("注册表操作失败（{op}，GetLastError={code}）")]
    Registry { op: &'static str, code: u32 },
    #[error("无法获取当前程序路径: {0}")]
    ExePath(String),
}

fn open_run_key(access: u32) -> Result<HKEY, AutostartError> {
    let mut hkey: HKEY = std::ptr::null_mut();
    let sub = to_wide(RUN_KEY);
    let code = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, access, &mut hkey) };
    if code != 0 {
        return Err(AutostartError::Registry {
            op: "open",
            code: unsafe { GetLastError() },
        });
    }
    Ok(hkey)
}

fn command_line() -> Result<Vec<u16>, AutostartError> {
    let exe = std::env::current_exe()
        .map_err(|e| AutostartError::ExePath(e.to_string()))?
        .to_string_lossy()
        .into_owned();
    // 引号包裹路径（可能含空格）；--minimized 启动时隐藏主窗口进托盘
    let cmd = format!("\"{exe}\" --minimized");
    Ok(to_wide(&cmd))
}

pub fn set_enabled(enable: bool) -> Result<(), AutostartError> {
    let hkey = open_run_key(KEY_SET_VALUE | KEY_QUERY_VALUE)?;
    let result = (|| {
        let name = to_wide(VALUE_NAME);
        if enable {
            let data = command_line()?;
            let code = unsafe {
                RegSetValueExW(
                    hkey,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            if code != 0 {
                return Err(AutostartError::Registry {
                    op: "set",
                    code: unsafe { GetLastError() },
                });
            }
        } else {
            let code = unsafe { RegDeleteValueW(hkey, name.as_ptr()) };
            // 值不存在视为已关闭
            if code != 0 && unsafe { GetLastError() } != 2
            /* ERROR_FILE_NOT_FOUND */
            {
                return Err(AutostartError::Registry { op: "delete", code });
            }
        }
        Ok(())
    })();
    unsafe { RegCloseKey(hkey) };
    result
}

pub fn is_enabled() -> bool {
    let Ok(hkey) = open_run_key(KEY_QUERY_VALUE) else {
        return false;
    };
    let name = to_wide(VALUE_NAME);
    let mut required: u32 = 0;
    let code = unsafe {
        RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut required,
        )
    };
    unsafe { RegCloseKey(hkey) };
    code == 0 && required > 0
}
