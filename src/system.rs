//! 平台基础工具：宽字符转换、单实例互斥、消息框提示。
//!
//! Win32 实现仅在 Windows 目标编译；其他平台提供占位实现（CI 只构建 Windows，
//! 门控的目的是让本机 linux `cargo check` 也能验证其余全部跨平台代码）。

/// UTF-8 -> UTF-16（含 trailing NUL），供 Win32 宽字符接口使用
#[cfg(windows)]
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
mod imp {
    use super::to_wide;
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

    #[derive(Debug, thiserror::Error)]
    pub enum SystemError {
        #[error("Windows API 调用失败（GetLastError={0}）")]
        Api(u32),
    }

    /// 创建命名互斥体实现单实例。
    /// 返回 `Ok(true)` 表示本进程是首个实例；`Ok(false)` 表示已有实例。
    ///
    /// 互斥体句柄故意不关闭：其生命周期与进程绑定，进程退出时由系统自动销毁。
    pub fn acquire_single_instance(name: &str) -> Result<bool, SystemError> {
        let wide = to_wide(name);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            return Err(SystemError::Api(unsafe { GetLastError() }));
        }
        let already_exists = unsafe { GetLastError() == ERROR_ALREADY_EXISTS };
        Ok(!already_exists)
    }

    /// 已有实例在运行时的用户提示（消息框；后续可换托盘气泡）
    pub fn warn_already_running() {
        let text = to_wide("BingWallpaper-Rust 已经在运行。");
        let caption = to_wide("BingWallpaper-Rust");
        unsafe {
            MessageBoxW(0, text.as_ptr(), caption.as_ptr(), MB_ICONWARNING | MB_OK);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    #[derive(Debug, thiserror::Error)]
    pub enum SystemError {
        #[error("单实例检测在当前平台不可用")]
        #[allow(dead_code)]
        Unsupported,
    }

    pub fn acquire_single_instance(_name: &str) -> Result<bool, SystemError> {
        Ok(true)
    }

    pub fn warn_already_running() {}
}

pub use imp::*;
