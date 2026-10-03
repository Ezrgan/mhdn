//! Fatal startup errors on Windows (no console subsystem), and where logs live.

use std::path::PathBuf;

/// `%APPDATA%\mhdn\logs` on Windows, `~/Library/Application Support/mhdn/logs` on macOS.
/// The directory is not created here.
pub fn log_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(root).join("mhdn").join("logs"))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/Application Support/mhdn")
                .join("logs"),
        )
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".config/mhdn").join("logs"))
    }
}

#[cfg(windows)]
pub fn report_startup_failure(message: &str) {
    write_log(message);
    show_message_box(message);
}

#[cfg(not(windows))]
pub fn report_startup_failure(message: &str) {
    eprintln!("{message}");
}

#[cfg(windows)]
fn write_log(message: &str) {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let Some(dir) = log_dir() else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("startup-{stamp}.log"));
    let _ = fs::write(path, message);
}

#[cfg(windows)]
fn show_message_box(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::prelude::OsStrExt;

    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let wide: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            title_wide().as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(windows)]
fn title_wide() -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::prelude::OsStrExt;

    OsStr::new("mhdn").encode_wide().chain(Some(0)).collect()
}
