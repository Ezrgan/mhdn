//! Win32 backend: find the Azahar window, follow it, and keep the winit window
//! topmost and click-through.
//!
//! Only borderless windowed and borderless fullscreen are supported. Exclusive
//! fullscreen owns the display, so no ordinary window can draw over it.
//!
//! This file has not been compiled on Windows yet. The geometry and style decisions live
//! in `win_geom`, which is tested on every host.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};
use winit::error::EventLoopError;
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowLevel};

use crate::error::PlatformError;
use crate::geom::Insets;
use crate::select::HostWindow;
use crate::track::{track_windows, Display, TrackedWindow, WindowTracker};
use crate::win_geom::{
    display, host_window, is_azahar_exe, overlay_ex_style, windows_insets, RawMonitor, RawWindow,
    BASE_DPI, EX_APPWINDOW, EX_LAYERED, EX_NOACTIVATE, EX_TOOLWINDOW, EX_TOPMOST, EX_TRANSPARENT,
};
use crate::OverlayUserEvent;

// The pure module repeats the style bits so it can be tested off Windows.
const _: () = assert!(EX_LAYERED == WS_EX_LAYERED);
const _: () = assert!(EX_TRANSPARENT == WS_EX_TRANSPARENT);
const _: () = assert!(EX_TOPMOST == WS_EX_TOPMOST);
const _: () = assert!(EX_NOACTIVATE == WS_EX_NOACTIVATE);
const _: () = assert!(EX_TOOLWINDOW == WS_EX_TOOLWINDOW);
const _: () = assert!(EX_APPWINDOW == WS_EX_APPWINDOW);

/// Last mode requested, so `join_active_space` can repair the style the same way.
static CLICK_THROUGH: AtomicBool = AtomicBool::new(true);

/// Topmost, click-through, no focus, no taskbar button. `click_through` is false while
/// calibration needs the mouse and the keyboard.
pub fn apply_click_through(window: &Window, click_through: bool) -> Result<(), PlatformError> {
    let hwnd = hwnd(window)?;
    CLICK_THROUGH.store(click_through, Ordering::Relaxed);
    // Winit owns the window flags and rewrites the whole extended style when one
    // changes, so go through it first and add the remaining bits afterwards.
    window.set_window_level(WindowLevel::AlwaysOnTop);
    let _ = window.set_cursor_hittest(!click_through);
    reassert_style(hwnd, click_through);
    arm_layered(hwnd);
    Ok(())
}

pub fn overlay_event_loop() -> Result<EventLoop<OverlayUserEvent>, EventLoopError> {
    EventLoop::<OverlayUserEvent>::with_user_event().build()
}

/// Windows has no spaces. Winit may reset the extended style when the window is shown,
/// so this puts the overlay style back. It returns true only when it had to.
pub fn join_active_space(window: &Window) -> Result<bool, PlatformError> {
    let hwnd = hwnd(window)?;
    Ok(reassert_style(hwnd, CLICK_THROUGH.load(Ordering::Relaxed)))
}

pub struct LatencyCritical;

pub fn begin_latency_critical() -> LatencyCritical {
    LatencyCritical
}

pub fn raise_thread_qos() -> bool {
    false
}

/// Process id that owns the foreground window. `None` when there is none.
pub fn frontmost_pid() -> Option<i32> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }
    i32::try_from(pid).ok()
}

/// Nothing to tint without a native window background. The spike is macOS only.
pub fn tint_spike(_window: &Window) -> Result<(), PlatformError> {
    Ok(())
}

/// Windows has no menu-bar item. This only remembers the label.
pub struct MenuStatus {
    title: Mutex<String>,
}

impl MenuStatus {
    pub fn install(_on_event: impl Fn(OverlayUserEvent) + 'static) -> Result<Self, PlatformError> {
        Ok(Self {
            title: Mutex::new(String::new()),
        })
    }

    pub fn set_title(&self, title: &str) {
        if let Ok(mut current) = self.title.lock() {
            current.clear();
            current.push_str(title);
        }
    }

    pub fn title(&self) -> String {
        self.title
            .lock()
            .map(|title| title.clone())
            .unwrap_or_default()
    }
}

fn hwnd(window: &Window) -> Result<HWND, PlatformError> {
    let handle = window
        .window_handle()
        .map_err(|err| PlatformError::Handle(err.to_string()))?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return Err(PlatformError::Handle("not a Win32 window".to_string()));
    };
    Ok(win32.hwnd.get() as HWND)
}

/// A layered window is not shown, and gets no `WM_PAINT`, until its layered attributes
/// have been set once. Full opacity keeps the per-pixel alpha of the swapchain intact.
fn arm_layered(hwnd: HWND) {
    unsafe { SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) };
}

/// Returns true when the style had to change.
fn reassert_style(hwnd: HWND, click_through: bool) -> bool {
    let current = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let wanted = overlay_ex_style(current, click_through);
    if wanted == current {
        return false;
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    arm_layered(hwnd);
    true
}

/// Polls Win32 for Azahar windows. Coordinates are logical, see `win_geom`.
pub struct WinTracker {
    insets: Insets,
    preferred_id: Option<u32>,
    /// Executable path per process, so a poll does not reopen every process.
    images: HashMap<u32, String>,
}

impl WinTracker {
    pub fn new(insets: Insets) -> Self {
        // Winit sets this when the event loop starts. Do it here too, so a poll that
        // runs first still reads physical pixels. It fails harmlessly when already set.
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        Self {
            insets,
            preferred_id: None,
            images: HashMap::new(),
        }
    }

    fn azahar_windows(&mut self) -> Vec<HostWindow> {
        let mut handles: Vec<HWND> = Vec::new();
        unsafe {
            EnumWindows(
                Some(collect_window),
                &mut handles as *mut Vec<HWND> as LPARAM,
            );
        }
        let own_pid = std::process::id();
        let mut seen = HashSet::new();
        let mut windows = Vec::new();
        for hwnd in handles {
            if unsafe { IsWindowVisible(hwnd) } == 0 || unsafe { IsIconic(hwnd) } != 0 {
                continue;
            }
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid == 0 || pid == own_pid {
                continue;
            }
            seen.insert(pid);
            let exe = self
                .images
                .entry(pid)
                .or_insert_with(|| process_image(pid))
                .clone();
            if !is_azahar_exe(&exe) {
                continue;
            }
            if let Some(host) = read_window(hwnd, pid, exe).as_ref().and_then(host_window) {
                windows.push(host);
            }
        }
        self.images.retain(|pid, _| seen.contains(pid));
        windows
    }
}

impl WindowTracker for WinTracker {
    fn poll(&mut self) -> Option<TrackedWindow> {
        let windows = self.azahar_windows();
        let displays = list_displays();
        let tracked = track_windows(
            &windows,
            &displays,
            windows_insets(self.insets),
            None,
            self.preferred_id,
        )?;
        self.preferred_id = Some(tracked.id);
        Some(tracked)
    }

    fn set_insets(&mut self, insets: Insets) {
        self.insets = insets;
    }
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let handles = &mut *(lparam as *mut Vec<HWND>);
    handles.push(hwnd);
    1
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = &mut *(lparam as *mut Vec<HMONITOR>);
    monitors.push(monitor);
    1
}

fn process_image(pid: u32) -> String {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new();
        }
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return String::new();
        }
        let len = (len as usize).min(buffer.len());
        String::from_utf16_lossy(&buffer[..len])
    }
}

fn read_window(hwnd: HWND, pid: u32, exe: String) -> Option<RawWindow> {
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        if GetClientRect(hwnd, &mut client) == 0 || ClientToScreen(hwnd, &mut origin) == 0 {
            return None;
        }
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    Some(RawWindow {
        hwnd: hwnd as usize,
        pid,
        exe,
        client_origin: (origin.x, origin.y),
        client_size: (client.right - client.left, client.bottom - client.top),
        visible: true,
        minimized: false,
        cloaked: is_cloaked(hwnd),
        dpi: monitor_dpi(monitor),
    })
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
    };
    result >= 0 && cloaked != 0
}

fn monitor_dpi(monitor: HMONITOR) -> u32 {
    let (mut x, mut y) = (0u32, 0u32);
    let result = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
    if result >= 0 && x > 0 {
        x
    } else {
        BASE_DPI
    }
}

fn list_displays() -> Vec<Display> {
    let mut monitors: Vec<HMONITOR> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_monitor),
            &mut monitors as *mut Vec<HMONITOR> as LPARAM,
        );
    }
    monitors
        .into_iter()
        .filter_map(|monitor| {
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                rcMonitor: RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                rcWork: RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                dwFlags: 0,
            };
            if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
                return None;
            }
            Some(display(&RawMonitor {
                left: info.rcMonitor.left,
                top: info.rcMonitor.top,
                right: info.rcMonitor.right,
                bottom: info.rcMonitor.bottom,
                dpi: monitor_dpi(monitor),
            }))
        })
        .collect()
}
