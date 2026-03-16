//! 管理員權限檢查與 UAC 提權模組

use std::env;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::Shell::IsUserAnAdmin;
use windows::core::PCWSTR;

/// 檢查當前進程是否以管理員權限執行
pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

/// 以管理員權限重新啟動當前程式
pub fn relaunch_as_admin() -> bool {
    let exe_path = match env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let args: Vec<String> = env::args().skip(1).collect();
    let args_str = args.join(" ");

    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
    let file: Vec<u16> = exe_path.as_os_str().encode_wide().chain(Some(0)).collect();
    let params: Vec<u16> = OsStr::new(&args_str)
        .encode_wide()
        .chain(Some(0))
        .collect();

    unsafe {
        let result = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR(params.as_ptr()),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );

        // ShellExecuteW 回傳值 > 32 代表成功
        result.0 as isize > 32
    }
}
