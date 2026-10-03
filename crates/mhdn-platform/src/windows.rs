//! Win32 backend: find the Azahar window, follow it, and keep the winit window
//! topmost, click-through and transparent.
//!
//! Only borderless windowed and borderless fullscreen are supported. Exclusive
//! fullscreen owns the display, so no ordinary window can draw over it; that case is
//! detected in [`WinTracker::poll`] and the overlay parks rather than sitting on the
//! desktop behind the game.
//!
//! The geometry and style decisions live in `win_geom`, which is tested on every host.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
use windows_sys::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_NOTOPMOST,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_APPWINDOW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
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
    display, host_display, host_window, is_azahar_exe, needs_raise, overlay_ex_style,
    parked_ex_style, windows_insets, HostDisplay, RawMonitor, RawWindow, BASE_DPI, EX_APPWINDOW,
    EX_LAYERED, EX_NOACTIVATE, EX_NOREDIRECTIONBITMAP, EX_TOOLWINDOW, EX_TOPMOST, EX_TRANSPARENT,
};
use crate::OverlayUserEvent;

// The pure module repeats the style bits so it can be tested off Windows.
const _: () = assert!(EX_LAYERED == WS_EX_LAYERED);
const _: () = assert!(EX_TRANSPARENT == WS_EX_TRANSPARENT);
const _: () = assert!(EX_TOPMOST == WS_EX_TOPMOST);
const _: () = assert!(EX_NOACTIVATE == WS_EX_NOACTIVATE);
const _: () = assert!(EX_TOOLWINDOW == WS_EX_TOOLWINDOW);
const _: () = assert!(EX_APPWINDOW == WS_EX_APPWINDOW);
const _: () = assert!(EX_NOREDIRECTIONBITMAP == WS_EX_NOREDIRECTIONBITMAP);

/// Last mode requested, so `join_active_space` can repair the style the same way.
static CLICK_THROUGH: AtomicBool = AtomicBool::new(true);
/// The overlay's own window, so the tracker poll can look for it in the z-order.
static OVERLAY_HWND: AtomicUsize = AtomicUsize::new(0);
/// Set by the tracker poll when the host has moved above the overlay, e.g. by going
/// borderless fullscreen. Consumed by `join_active_space`.
static RAISE: AtomicBool = AtomicBool::new(false);

/// Topmost, click-through, no focus, no taskbar button. `click_through` is false while
/// calibration needs the mouse and the keyboard.
///
/// Click-through is `WS_EX_TRANSPARENT`, set here rather than through winit's
/// `set_cursor_hittest`: winit implements that one as `WS_EX_TRANSPARENT |
/// WS_EX_LAYERED`, and a layered window is composited with a single alpha for the
/// whole surface, which hides the per-pixel alpha the swapchain produces.
pub fn apply_click_through(window: &Window, click_through: bool) -> Result<(), PlatformError> {
    let hwnd = hwnd(window)?;
    CLICK_THROUGH.store(click_through, Ordering::Relaxed);
    OVERLAY_HWND.store(hwnd as usize, Ordering::Relaxed);
    // Winit owns the window flags and rewrites the whole extended style when one
    // changes, so go through it first and add the remaining bits afterwards.
    window.set_window_level(WindowLevel::AlwaysOnTop);
    if !reassert_style(hwnd, click_through) {
        raise(hwnd);
    }
    Ok(())
}

pub fn overlay_event_loop() -> Result<EventLoop<OverlayUserEvent>, EventLoopError> {
    EventLoop::<OverlayUserEvent>::with_user_event().build()
}

/// Windows has no spaces, but it has the same two problems the macOS call solves:
/// winit resets the extended style whenever it touches a window flag, and a game that
/// goes borderless fullscreen lands in front of the overlay. This puts the style back
/// and raises the overlay again. It returns true only when it had to.
pub fn join_active_space(window: &Window) -> Result<bool, PlatformError> {
    let hwnd = hwnd(window)?;
    let restyled = reassert_style(hwnd, CLICK_THROUGH.load(Ordering::Relaxed));
    let raised = RAISE.swap(false, Ordering::Relaxed);
    if raised && !restyled {
        raise(hwnd);
    }
    Ok(restyled || raised)
}

/// Takes the overlay out of the way while another app owns the keyboard, and brings it
/// back afterwards. Shrinking the window is not enough on Windows: `WS_EX_TOPMOST`
/// keeps even a one-pixel window in front of every normal window, so the parked
/// overlay is hidden and dropped out of the topmost band.
pub fn set_overlay_parked(window: &Window, parked: bool) -> Result<(), PlatformError> {
    let hwnd = hwnd(window)?;
    if parked {
        // Hidden first: winit rebuilds the whole extended style when a flag changes,
        // and a visible overlay without `TOOLWINDOW` grows a taskbar button.
        window.set_visible(false);
        window.set_window_level(WindowLevel::Normal);
        let current = ex_style(hwnd);
        let wanted = parked_ex_style(current);
        if wanted != current {
            unsafe {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);
                SetWindowPos(
                    hwnd,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        }
    } else {
        window.set_window_level(WindowLevel::AlwaysOnTop);
        window.set_visible(true);
        if !reassert_style(hwnd, CLICK_THROUGH.load(Ordering::Relaxed)) {
            raise(hwnd);
        }
    }
    Ok(())
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

fn ex_style(hwnd: HWND) -> u32 {
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
}

/// Puts the overlay back at the top of the topmost band without activating it.
fn raise(hwnd: HWND) {
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}

/// Returns true when the style had to change.
fn reassert_style(hwnd: HWND, click_through: bool) -> bool {
    let current = ex_style(hwnd);
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
    true
}

/// True while a Direct3D application owns the display in exclusive mode. Azahar's
/// borderless fullscreen does not set this; a real exclusive-mode game does, and
/// nothing another process creates is composited over it.
fn d3d_exclusive_fullscreen() -> bool {
    let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
    let result = unsafe { SHQueryUserNotificationState(&mut state) };
    result >= 0 && state == QUNS_RUNNING_D3D_FULL_SCREEN
}

/// How often the shell is asked whether Direct3D owns the display. It is a shell32
/// round trip and the answer only changes when a game switches mode.
const EXCLUSIVE_POLL: Duration = Duration::from_millis(250);

/// Polls Win32 for Azahar windows. Coordinates are logical, see `win_geom`.
pub struct WinTracker {
    insets: Insets,
    preferred_id: Option<u32>,
    /// Executable path per process, so a poll does not reopen every process.
    images: HashMap<u32, String>,
    exclusive: bool,
    exclusive_checked: Option<Instant>,
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
            exclusive: false,
            exclusive_checked: None,
        }
    }

    /// Only asked while the host fills a monitor and owns the keyboard, which is the
    /// only shape exclusive fullscreen can have.
    fn exclusive_fullscreen(&mut self, tracked: &TrackedWindow) -> bool {
        if !tracked.is_fullscreen || frontmost_pid() != Some(tracked.owner_pid) {
            self.exclusive = false;
            return false;
        }
        let now = Instant::now();
        if self
            .exclusive_checked
            .is_none_or(|then| now.saturating_duration_since(then) >= EXCLUSIVE_POLL)
        {
            self.exclusive_checked = Some(now);
            self.exclusive = d3d_exclusive_fullscreen();
        }
        self.exclusive
    }

    /// Azahar's windows, plus every top-level window in z-order, top first, the way
    /// `EnumWindows` hands them over.
    fn azahar_windows(&mut self) -> (Vec<HostWindow>, Vec<usize>) {
        let mut handles: Vec<HWND> = Vec::new();
        unsafe {
            EnumWindows(
                Some(collect_window),
                &mut handles as *mut Vec<HWND> as LPARAM,
            );
        }
        let z_order: Vec<usize> = handles.iter().map(|hwnd| *hwnd as usize).collect();
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
        (windows, z_order)
    }
}

impl WindowTracker for WinTracker {
    fn poll(&mut self) -> Option<TrackedWindow> {
        let (windows, z_order) = self.azahar_windows();
        let displays = list_displays();
        let tracked = track_windows(
            &windows,
            &displays,
            windows_insets(self.insets),
            None,
            self.preferred_id,
        )?;
        let exclusive = self.exclusive_fullscreen(&tracked);
        if host_display(tracked.is_fullscreen, exclusive) == HostDisplay::Exclusive {
            // Direct3D owns the display. Reporting the window would put the overlay on
            // the desktop underneath it, where it is both invisible and in the way.
            return None;
        }
        self.preferred_id = Some(tracked.id);
        let overlay = OVERLAY_HWND.load(Ordering::Relaxed);
        if let Some(host) = z_order
            .iter()
            .copied()
            .find(|hwnd| (hwnd & 0xFFFF_FFFF) as u32 == tracked.id)
        {
            if overlay != 0 && needs_raise(&z_order, overlay, host) {
                RAISE.store(true, Ordering::Relaxed);
            }
        }
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
