#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(err) = mhdn_app::run() {
        let message = format!("mhdn: {err}");
        mhdn_platform::report_startup_failure(&message);
        std::process::exit(1);
    }
}
