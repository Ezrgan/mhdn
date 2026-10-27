//! Windows window math with no Win32 calls, so it is tested on every host.
//!
//! On Windows the tracker works in **physical pixels** end to end. Monitor frames
//! still carry a logical scale factor for inset math.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use crate::geom::{Insets, Rect, DEFAULT_TITLE_BAR_PT};
use crate::select::HostWindow;
use crate::track::Display;

pub const BASE_DPI: u32 = 96;
/// Height of Azahar's Qt menu bar. It sits inside the client area, below the native
/// title bar, so it is the only top chrome left once the client rect is used.
pub const DEFAULT_MENU_BAR_DIP: f32 = 22.0;

pub const STYLE_POPUP: u32 = 0x8000_0000;
pub const STYLE_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const STYLE_CAPTION: u32 = 0x00C0_0000;
pub const STYLE_BORDER: u32 = 0x0080_0000;
pub const STYLE_THICKFRAME: u32 = 0x0004_0000;
pub const STYLE_SYSMENU: u32 = 0x0008_0000;
pub const STYLE_MINIMIZEBOX: u32 = 0x0002_0000;
pub const STYLE_MAXIMIZEBOX: u32 = 0x0001_0000;

pub const EX_TOPMOST: u32 = 0x0000_0008;
pub const EX_TRANSPARENT: u32 = 0x0000_0020;
pub const EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;
pub const EX_APPWINDOW: u32 = 0x0004_0000;
pub const EX_LAYERED: u32 = 0x0008_0000;
pub const EX_NOACTIVATE: u32 = 0x0800_0000;
pub const EX_WINDOWEDGE: u32 = 0x0000_0100;
pub const EX_CLIENTEDGE: u32 = 0x0000_0200;
pub const EX_DLGMODALFRAME: u32 = 0x0000_0001;

pub const MA_NOACTIVATE: i32 = 3;

/// `HTTRANSPARENT`. `WM_NCHITTEST` returning this hands the mouse to the window
/// underneath, including a window owned by another process.
pub const HTTRANSPARENT: i32 = -1;

/// A top-level window as read from Win32, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct RawWindow {
    pub hwnd: usize,
    pub pid: u32,
    /// Full path or bare file name of the owning executable. Empty when unreadable.
    pub exe: String,
    pub class_name: String,
    pub style: u32,
    pub ex_style: u32,
    /// Client-area origin in virtual-screen pixels (`ClientToScreen(0, 0)`).
    pub client_origin: (i32, i32),
    pub client_size: (i32, i32),
    pub visible: bool,
    pub minimized: bool,
    /// DWM cloaked, e.g. on another virtual desktop.
    pub cloaked: bool,
    pub dpi: u32,
}

/// A monitor as read from Win32, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawMonitor {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub dpi: u32,
}

pub fn dpi_scale(dpi: u32) -> f32 {
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / BASE_DPI as f32
    }
}

/// File stem of an executable path, accepting either separator.
pub fn exe_stem(path: &str) -> &str {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match name.len().checked_sub(4) {
        Some(split)
            if name.is_char_boundary(split) && name[split..].eq_ignore_ascii_case(".exe") =>
        {
            &name[..split]
        }
        _ => name,
    }
}

/// `azahar.exe` and renamed builds such as `azahar-qt.exe`. The dedicated room
/// server has no emulator window and is left out.
pub fn is_azahar_exe(path: &str) -> bool {
    let stem = exe_stem(path).to_ascii_lowercase();
    stem.contains("azahar") && !stem.contains("room")
}

/// The name `select_window` compares against. Every Azahar build reports "Azahar".
pub fn owner_name(path: &str) -> String {
    if is_azahar_exe(path) {
        "Azahar".to_string()
    } else {
        exe_stem(path).to_string()
    }
}

pub fn is_candidate_window_class(class_name: &str) -> bool {
    let lower = class_name.to_ascii_lowercase();
    !matches!(
        lower.as_str(),
        "consolewindowclass" | "pseudoconsolewindow" | "ime" | "msctfime ui" | "tooltips_class32"
    )
}

pub fn overlay_style(current: u32) -> u32 {
    (current | STYLE_POPUP | STYLE_CLIPSIBLINGS)
        & !(STYLE_CAPTION
            | STYLE_BORDER
            | STYLE_THICKFRAME
            | STYLE_SYSMENU
            | STYLE_MINIMIZEBOX
            | STYLE_MAXIMIZEBOX)
}

/// Extended style the overlay needs. Winit rewrites `GWL_EXSTYLE` whenever it changes
/// its own flags, so the caller re-applies this and compares with what it read.
pub fn overlay_ex_style(current: u32, click_through: bool) -> u32 {
    let mut style = current & !(EX_APPWINDOW | EX_WINDOWEDGE | EX_CLIENTEDGE | EX_DLGMODALFRAME);
    if click_through {
        style |= EX_TOPMOST | EX_TOOLWINDOW | EX_TRANSPARENT | EX_NOACTIVATE | EX_LAYERED;
    } else {
        style |= EX_TOPMOST | EX_TOOLWINDOW;
        style &= !(EX_TRANSPARENT | EX_NOACTIVATE | EX_LAYERED);
    }
    style
}

/// `WM_NCHITTEST` result for the overlay.
pub fn overlay_nchittest(click_through: bool) -> Option<i32> {
    if click_through {
        Some(HTTRANSPARENT)
    } else {
        None
    }
}

/// `WM_MOUSEACTIVATE` result while click-through is on.
pub fn overlay_mouseactivate(click_through: bool) -> Option<i32> {
    if click_through {
        Some(MA_NOACTIVATE)
    } else {
        None
    }
}

/// Windows layer is always 0. Bounds are physical client pixels.
pub fn host_window(raw: &RawWindow) -> Option<HostWindow> {
    if raw.client_size.0 <= 0 || raw.client_size.1 <= 0 {
        return None;
    }
    if !is_candidate_window_class(&raw.class_name) || (raw.ex_style & EX_TOOLWINDOW) != 0 {
        return None;
    }
    Some(HostWindow {
        id: (raw.hwnd & 0xFFFF_FFFF) as u32,
        owner_name: owner_name(&raw.exe),
        owner_pid: i32::try_from(raw.pid).unwrap_or(0),
        layer: 0,
        bounds: Rect::new(
            raw.client_origin.0 as f32,
            raw.client_origin.1 as f32,
            raw.client_size.0 as f32,
            raw.client_size.1 as f32,
        ),
        onscreen: raw.visible && !raw.minimized && !raw.cloaked,
    })
}

pub fn display(raw: &RawMonitor) -> Display {
    Display {
        frame: Rect::new(
            raw.left as f32,
            raw.top as f32,
            (raw.right - raw.left).max(0) as f32,
            (raw.bottom - raw.top).max(0) as f32,
        ),
        scale: dpi_scale(raw.dpi),
    }
}

/// The macOS default title bar does not exist here. When the config left it at that
/// default, use the Qt menu bar height. An explicit value, including 0, is kept.
pub fn windows_insets(insets: Insets) -> Insets {
    let default_title = (insets.title_bar - DEFAULT_TITLE_BAR_PT).abs() < f32::EPSILON;
    Insets {
        title_bar: if default_title {
            DEFAULT_MENU_BAR_DIP
        } else {
            insets.title_bar
        },
        ..insets
    }
}

/// Chrome insets scaled to physical pixels for the host window's DPI.
pub fn physical_windows_insets(insets: Insets, scale: f32, frameless: bool) -> Insets {
    let base = windows_insets(insets);
    let s = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    Insets {
        title_bar: if frameless { 0.0 } else { base.title_bar * s },
        status_bar: base.status_bar * s,
        left: base.left * s,
        right: base.right * s,
    }
}

/// Candidate `qt-config.ini` paths, portable install first when `exe_path` is known.
pub fn candidate_azahar_configs(
    exe_path: Option<&str>,
    appdata: Option<&str>,
    local_appdata: Option<&str>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(exe) = exe_path {
        let normalized = exe.replace('\\', "/");
        if let Some(dir) = PathBuf::from(normalized)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            paths.push(dir.join("user/config/qt-config.ini"));
        }
    }
    if let Some(a) = appdata.filter(|s| !s.is_empty()) {
        let base = PathBuf::from(a);
        paths.push(base.join("Azahar/config/qt-config.ini"));
        paths.push(base.join("Citra/config/qt-config.ini"));
        paths.push(base.join("Lime3DS/config/qt-config.ini"));
    }
    if let Some(la) = local_appdata.filter(|s| !s.is_empty()) {
        paths.push(PathBuf::from(la).join("Azahar/config/qt-config.ini"));
    }
    paths
}

/// Extended style of a parked overlay. Dropping `TOPMOST` is what lets another app's
/// window own the space again; a topmost window stays in front of every normal one
/// however small it has been made.
pub fn parked_ex_style(current: u32) -> u32 {
    current & !EX_TOPMOST
}

/// How Azahar is putting its picture on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostDisplay {
    /// An ordinary window. The overlay follows its client rect.
    Windowed,
    /// A window the size of its monitor. A topmost window is still composited over it.
    Borderless,
    /// Direct3D owns the display. Nothing another process creates is composited over
    /// it, so the overlay stays parked instead of sitting on the desktop underneath.
    Exclusive,
}

/// `fullscreen` is the bounds match from `geom::is_fullscreen`; `d3d_exclusive` is the
/// shell reporting a Direct3D exclusive-mode application. Azahar's own "Fullscreen
/// Mode: Borderless Window" never sets the second one.
pub fn host_display(fullscreen: bool, d3d_exclusive: bool) -> HostDisplay {
    match (fullscreen, d3d_exclusive) {
        (false, _) => HostDisplay::Windowed,
        (true, false) => HostDisplay::Borderless,
        (true, true) => HostDisplay::Exclusive,
    }
}

/// Whether the overlay has to be put back on top of the host. `z_order` is top to
/// bottom, the way `EnumWindows` reports it. Going borderless fullscreen pushes the
/// game to the front of the z-order, and `WS_EX_TOPMOST` alone does not re-sort an
/// already placed window, so the overlay has to be raised again.
pub fn needs_raise(z_order: &[usize], overlay: usize, host: usize) -> bool {
    let Some(host_at) = z_order.iter().position(|hwnd| *hwnd == host) else {
        return false;
    };
    match z_order.iter().position(|hwnd| *hwnd == overlay) {
        Some(overlay_at) => overlay_at > host_at,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::DEFAULT_STATUS_BAR_PT;
    use crate::select::{select_window, TrackQuery};
    use crate::track::track_windows;

    fn azahar(hwnd: usize, origin: (i32, i32), size: (i32, i32), dpi: u32) -> RawWindow {
        RawWindow {
            hwnd,
            pid: 4242,
            exe: r"C:\Emu\Azahar\azahar.exe".to_string(),
            class_name: "Qt5152QWindowIcon".to_string(),
            style: STYLE_CAPTION,
            ex_style: 0,
            client_origin: origin,
            client_size: size,
            visible: true,
            minimized: false,
            cloaked: false,
            dpi,
        }
    }

    #[test]
    fn overlay_style_strips_caption_and_borders_and_sets_popup() {
        let styled = overlay_style(STYLE_CAPTION | STYLE_BORDER | STYLE_THICKFRAME);
        assert_ne!(styled & STYLE_CAPTION, STYLE_CAPTION);
        assert_ne!(styled & STYLE_BORDER, STYLE_BORDER);
        assert_eq!(styled & STYLE_POPUP, STYLE_POPUP);
        assert_eq!(styled & STYLE_CLIPSIBLINGS, STYLE_CLIPSIBLINGS);
    }

    #[test]
    fn overlay_ex_style_includes_layered_transparent_and_noactivate_for_click_through() {
        let style = overlay_ex_style(0, true);
        for bit in [
            EX_TRANSPARENT,
            EX_TOPMOST,
            EX_NOACTIVATE,
            EX_TOOLWINDOW,
            EX_LAYERED,
        ] {
            assert_eq!(style & bit, bit, "bit {bit:#x}");
        }
        assert_eq!(style & EX_APPWINDOW, 0);
        assert_eq!(style & EX_WINDOWEDGE, 0);
    }

    #[test]
    fn overlay_ex_style_clears_layered_transparent_and_noactivate_for_calibration() {
        let style = overlay_ex_style(overlay_ex_style(0, true), false);
        assert_eq!(style & (EX_TRANSPARENT | EX_NOACTIVATE | EX_LAYERED), 0);
        assert_eq!(
            style & (EX_TOPMOST | EX_TOOLWINDOW),
            EX_TOPMOST | EX_TOOLWINDOW
        );
    }

    #[test]
    fn overlay_mouseactivate_returns_ma_noactivate_only_when_click_through() {
        assert_eq!(overlay_mouseactivate(true), Some(MA_NOACTIVATE));
        assert_eq!(overlay_mouseactivate(false), None);
    }

    #[test]
    fn console_and_tool_windows_are_rejected_as_host_windows() {
        let mut raw = azahar(1, (0, 0), (800, 480), 96);
        raw.class_name = "ConsoleWindowClass".to_string();
        assert!(host_window(&raw).is_none());
        raw.class_name = "Qt5152QWindowIcon".to_string();
        raw.ex_style = EX_TOOLWINDOW;
        assert!(host_window(&raw).is_none());
    }

    #[test]
    fn physical_windows_insets_scales_by_dpi_and_drops_menu_when_frameless() {
        let insets = physical_windows_insets(Insets::chrome(true), 2.0, false);
        assert_eq!(insets.title_bar, DEFAULT_MENU_BAR_DIP * 2.0);
        assert_eq!(insets.status_bar, DEFAULT_STATUS_BAR_PT * 2.0);
        let frameless = physical_windows_insets(Insets::chrome(true), 2.0, true);
        assert_eq!(frameless.title_bar, 0.0);
    }

    #[test]
    fn candidate_azahar_configs_prioritizes_portable_user_folder() {
        let paths = candidate_azahar_configs(
            Some(r"C:\Games\Azahar\azahar.exe"),
            Some(r"C:\Users\me\AppData\Roaming"),
            None,
        );
        assert_eq!(paths.len(), 4);
        let portable = paths[0].to_string_lossy();
        assert!(portable.ends_with("user/config/qt-config.ini"));
        assert!(portable.contains("Games/Azahar") || portable.contains(r"Games\Azahar"));
    }

    #[test]
    fn executable_names_are_matched_on_the_file_stem() {
        assert_eq!(exe_stem(r"C:\Emu\Azahar\azahar.exe"), "azahar");
        assert!(is_azahar_exe(r"C:\Emu\azahar.exe"));
        assert_eq!(owner_name("azahar-qt.exe"), "Azahar");
    }

    #[test]
    fn a_client_rect_is_reported_in_physical_pixels() {
        let host = host_window(&azahar(0x1A2B, (300, 150), (1200, 720), 144)).unwrap();
        assert_eq!(host.bounds, Rect::new(300.0, 150.0, 1200.0, 720.0));
    }

    #[test]
    fn hidden_minimized_cloaked_or_empty_windows_are_not_on_screen() {
        let mut raw = azahar(1, (0, 0), (800, 480), 96);
        raw.visible = false;
        assert!(!host_window(&raw).unwrap().onscreen);
        assert!(host_window(&azahar(1, (0, 0), (0, 0), 96)).is_none());
    }

    #[test]
    fn a_borderless_window_covering_a_scaled_monitor_is_fullscreen() {
        let monitor = RawMonitor {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
            dpi: 120,
        };
        let scale = dpi_scale(120);
        let windows = [host_window(&azahar(9, (0, 0), (1920, 1080), 120)).unwrap()];
        let tracked = track_windows(
            &windows,
            &[display(&monitor)],
            physical_windows_insets(Insets::chrome(false), scale, false),
            None,
            None,
        )
        .unwrap();
        assert!(tracked.is_fullscreen);
        assert_eq!(tracked.scale, scale);
        assert_eq!(tracked.content_rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn a_windowed_client_keeps_only_the_menu_bar_inset() {
        let monitor = RawMonitor {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
            dpi: 96,
        };
        let windows = [host_window(&azahar(9, (100, 80), (1000, 600), 96)).unwrap()];
        let insets = physical_windows_insets(Insets::chrome(true), 1.0, false);
        let tracked = track_windows(&windows, &[display(&monitor)], insets, None, None).unwrap();
        assert!(!tracked.is_fullscreen);
        assert_eq!(tracked.content_rect.x, 100.0);
        assert_eq!(tracked.content_rect.y, 80.0 + DEFAULT_MENU_BAR_DIP);
        assert_eq!(
            tracked.content_rect.height,
            600.0 - DEFAULT_MENU_BAR_DIP - DEFAULT_STATUS_BAR_PT
        );
    }

    #[test]
    fn the_top_screen_window_wins_and_other_owners_are_ignored() {
        let bottom = azahar(1, (0, 0), (640, 480), 96);
        let top = azahar(2, (0, 0), (1000, 600), 96);
        let mut stranger = azahar(3, (0, 0), (1000, 600), 96);
        stranger.exe = r"C:\Windows\explorer.exe".to_string();
        let hosts: Vec<_> = [bottom, top, stranger]
            .iter()
            .filter_map(host_window)
            .collect();
        let query = TrackQuery {
            owner_name: "Azahar",
            owner_pid: None,
            preferred_id: None,
        };
        assert_eq!(select_window(&hosts, &query).unwrap().id, 2);
    }

    #[test]
    fn click_through_hit_test_is_httransparent() {
        assert_eq!(overlay_nchittest(true), Some(HTTRANSPARENT));
    }

    #[test]
    fn the_no_redirection_bitmap_bit_survives_a_restyle() {
        let style = overlay_ex_style(EX_NOREDIRECTIONBITMAP, true);
        assert_eq!(style & EX_NOREDIRECTIONBITMAP, EX_NOREDIRECTIONBITMAP);
    }

    #[test]
    fn a_parked_overlay_leaves_the_topmost_band() {
        let shown = overlay_ex_style(EX_NOREDIRECTIONBITMAP, true);
        let parked = parked_ex_style(shown);
        assert_eq!(parked & EX_TOPMOST, 0);
    }

    #[test]
    fn the_overlay_is_raised_once_the_host_is_above_it() {
        let overlay = 0x11;
        let host = 0x22;
        assert!(needs_raise(&[host, overlay], overlay, host));
    }

    #[test]
    fn a_monitor_sized_window_is_borderless_unless_direct3d_owns_the_display() {
        assert_eq!(host_display(false, false), HostDisplay::Windowed);
        assert_eq!(host_display(true, false), HostDisplay::Borderless);
        assert_eq!(host_display(true, true), HostDisplay::Exclusive);
    }
}
