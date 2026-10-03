//! Windows window math with no Win32 calls, so it is tested on every host.
//!
//! Win32 reports physical pixels. The rest of the crate works in logical units
//! (winit's `LogicalPosition`), so every rectangle is divided by its monitor's scale.

#![forbid(unsafe_code)]

use crate::geom::{Insets, Rect, DEFAULT_TITLE_BAR_PT};
use crate::select::HostWindow;
use crate::track::Display;

pub const BASE_DPI: u32 = 96;
/// Height of Azahar's Qt menu bar. It sits inside the client area, below the native
/// title bar, so it is the only top chrome left once the client rect is used.
pub const DEFAULT_MENU_BAR_DIP: f32 = 22.0;

pub const EX_TOPMOST: u32 = 0x0000_0008;
pub const EX_TRANSPARENT: u32 = 0x0000_0020;
pub const EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;
pub const EX_APPWINDOW: u32 = 0x0004_0000;
pub const EX_LAYERED: u32 = 0x0008_0000;
pub const EX_NOACTIVATE: u32 = 0x0800_0000;

/// A top-level window as read from Win32, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct RawWindow {
    pub hwnd: usize,
    pub pid: u32,
    /// Full path or bare file name of the owning executable. Empty when unreadable.
    pub exe: String,
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

pub fn to_logical(origin: (i32, i32), size: (i32, i32), dpi: u32) -> Rect {
    let scale = dpi_scale(dpi);
    Rect::new(
        origin.0 as f32 / scale,
        origin.1 as f32 / scale,
        size.0.max(0) as f32 / scale,
        size.1.max(0) as f32 / scale,
    )
}

/// Windows layer is always 0. The client rect already excludes the title bar and borders.
pub fn host_window(raw: &RawWindow) -> Option<HostWindow> {
    if raw.client_size.0 <= 0 || raw.client_size.1 <= 0 {
        return None;
    }
    Some(HostWindow {
        id: (raw.hwnd & 0xFFFF_FFFF) as u32,
        owner_name: owner_name(&raw.exe),
        owner_pid: i32::try_from(raw.pid).unwrap_or(0),
        layer: 0,
        bounds: to_logical(raw.client_origin, raw.client_size, raw.dpi),
        onscreen: raw.visible && !raw.minimized && !raw.cloaked,
    })
}

pub fn display(raw: &RawMonitor) -> Display {
    Display {
        frame: to_logical(
            (raw.left, raw.top),
            (raw.right - raw.left, raw.bottom - raw.top),
            raw.dpi,
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

/// Extended style the overlay needs. Winit rewrites `GWL_EXSTYLE` whenever it changes
/// its own flags, so the caller re-applies this and compares with what it read.
///
/// Click-through adds `TRANSPARENT` and `NOACTIVATE`. Calibration drops both so the
/// window can take the mouse and the keyboard.
///
/// `LAYERED` is cleared on purpose. A layered window is composited with one uniform
/// alpha, which throws away the per-pixel alpha of the swapchain and leaves the clear
/// colour on screen as a black sheet. Winit adds the bit together with `TRANSPARENT`
/// whenever `set_cursor_hittest` is used, so the overlay sets `TRANSPARENT` itself and
/// takes the bit back off here.
pub fn overlay_ex_style(current: u32, click_through: bool) -> u32 {
    let mut style = (current | EX_TOPMOST | EX_TOOLWINDOW) & !(EX_APPWINDOW | EX_LAYERED);
    if click_through {
        style |= EX_TRANSPARENT | EX_NOACTIVATE;
    } else {
        style &= !(EX_TRANSPARENT | EX_NOACTIVATE);
    }
    style
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
    use crate::select::{select_window, TrackQuery};
    use crate::track::track_windows;

    fn azahar(hwnd: usize, origin: (i32, i32), size: (i32, i32), dpi: u32) -> RawWindow {
        RawWindow {
            hwnd,
            pid: 4242,
            exe: r"C:\Emu\Azahar\azahar.exe".to_string(),
            client_origin: origin,
            client_size: size,
            visible: true,
            minimized: false,
            cloaked: false,
            dpi,
        }
    }

    #[test]
    fn executable_names_are_matched_on_the_file_stem() {
        assert_eq!(exe_stem(r"C:\Emu\Azahar\azahar.exe"), "azahar");
        assert_eq!(exe_stem("C:/Emu/AZAHAR.EXE"), "AZAHAR");
        assert_eq!(exe_stem("azahar"), "azahar");
        assert!(is_azahar_exe(r"C:\Emu\azahar.exe"));
        assert!(is_azahar_exe("Azahar-Qt.exe"));
        assert!(!is_azahar_exe("azahar-room.exe"));
        assert!(!is_azahar_exe(r"C:\Windows\explorer.exe"));
        assert!(!is_azahar_exe(""));
        assert_eq!(owner_name("azahar-qt.exe"), "Azahar");
        assert_eq!(owner_name(r"C:\Windows\explorer.exe"), "explorer");
    }

    #[test]
    fn a_client_rect_is_converted_to_logical_units() {
        let host = host_window(&azahar(0x1A2B, (300, 150), (1200, 720), 144)).unwrap();
        assert_eq!(host.id, 0x1A2B);
        assert_eq!(host.owner_name, "Azahar");
        assert_eq!(host.owner_pid, 4242);
        assert_eq!(host.layer, 0);
        assert!(host.onscreen);
        assert_eq!(host.bounds, Rect::new(200.0, 100.0, 800.0, 480.0));
    }

    #[test]
    fn hidden_minimized_cloaked_or_empty_windows_are_not_on_screen() {
        for tweak in 0..3 {
            let mut raw = azahar(1, (0, 0), (800, 480), 96);
            match tweak {
                0 => raw.visible = false,
                1 => raw.minimized = true,
                _ => raw.cloaked = true,
            }
            assert!(!host_window(&raw).unwrap().onscreen);
        }
        assert!(host_window(&azahar(1, (0, 0), (0, 0), 96)).is_none());
        // A minimized window can report a negative client size.
        assert!(host_window(&azahar(1, (0, 0), (-32, -32), 96)).is_none());
    }

    #[test]
    fn a_zero_dpi_reading_falls_back_to_one_to_one() {
        assert_eq!(dpi_scale(0), 1.0);
        assert_eq!(dpi_scale(192), 2.0);
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
        let windows = [host_window(&azahar(9, (0, 0), (1920, 1080), 120)).unwrap()];
        let tracked = track_windows(
            &windows,
            &[display(&monitor)],
            windows_insets(Insets::chrome(false)),
            None,
            None,
        )
        .unwrap();
        assert!(tracked.is_fullscreen);
        assert_eq!(tracked.scale, 1.25);
        assert_eq!(tracked.content_rect, Rect::new(0.0, 0.0, 1536.0, 864.0));
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
        let tracked = track_windows(
            &windows,
            &[display(&monitor)],
            windows_insets(Insets::chrome(true)),
            None,
            None,
        )
        .unwrap();
        assert!(!tracked.is_fullscreen);
        assert_eq!(tracked.content_rect.x, 100.0);
        assert_eq!(tracked.content_rect.y, 80.0 + DEFAULT_MENU_BAR_DIP);
        assert_eq!(
            tracked.content_rect.height,
            600.0 - DEFAULT_MENU_BAR_DIP - 24.0
        );
    }

    #[test]
    fn an_explicit_title_inset_is_kept_and_zero_is_allowed() {
        let mut insets = Insets::chrome(false);
        insets.title_bar = 0.0;
        assert_eq!(windows_insets(insets).title_bar, 0.0);
        insets.title_bar = 31.0;
        assert_eq!(windows_insets(insets).title_bar, 31.0);
        assert_eq!(
            windows_insets(Insets::default()).title_bar,
            DEFAULT_MENU_BAR_DIP
        );
    }

    #[test]
    fn the_top_screen_window_wins_and_other_owners_are_ignored() {
        let bottom = azahar(1, (0, 0), (640, 480), 96);
        let top = azahar(2, (0, 0), (1000, 600), 96);
        let mut stranger = azahar(3, (0, 0), (1000, 600), 96);
        stranger.exe = r"C:\Windows\explorer.exe".to_string();
        stranger.pid = 7;
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
    fn click_through_adds_the_four_overlay_styles() {
        let style = overlay_ex_style(0, true);
        for bit in [EX_TRANSPARENT, EX_TOPMOST, EX_NOACTIVATE, EX_TOOLWINDOW] {
            assert_eq!(style & bit, bit);
        }
    }

    #[test]
    fn calibration_can_take_the_mouse_and_focus() {
        let style = overlay_ex_style(overlay_ex_style(0, true), false);
        assert_eq!(style & (EX_TRANSPARENT | EX_NOACTIVATE), 0);
        assert_eq!(
            style & (EX_TOPMOST | EX_TOOLWINDOW),
            EX_TOPMOST | EX_TOOLWINDOW
        );
    }

    #[test]
    fn the_layered_bit_is_taken_back_off_however_it_arrived() {
        // Winit sets LAYERED with TRANSPARENT behind `set_cursor_hittest`, and a
        // layered window is composited with one alpha for every pixel: the clear
        // colour would cover the game instead of the game showing through.
        for click_through in [true, false] {
            let style = overlay_ex_style(EX_LAYERED, click_through);
            assert_eq!(style & EX_LAYERED, 0);
        }
    }

    #[test]
    fn the_no_redirection_bitmap_bit_survives_a_restyle() {
        // Set at creation and never rewritten: the DirectComposition swapchain is the
        // only content the window has.
        let style = overlay_ex_style(EX_NOREDIRECTIONBITMAP, true);
        assert_eq!(style & EX_NOREDIRECTIONBITMAP, EX_NOREDIRECTIONBITMAP);
    }

    #[test]
    fn the_style_is_stable_and_leaves_other_bits_alone() {
        let other = 0x0000_0100; // WS_EX_WINDOWEDGE
        let once = overlay_ex_style(other | EX_APPWINDOW, true);
        assert_eq!(once & EX_APPWINDOW, 0);
        assert_eq!(once & other, other);
        assert_eq!(overlay_ex_style(once, true), once);
    }

    #[test]
    fn a_parked_overlay_leaves_the_topmost_band() {
        let shown = overlay_ex_style(EX_NOREDIRECTIONBITMAP, true);
        let parked = parked_ex_style(shown);
        assert_eq!(parked & EX_TOPMOST, 0);
        assert_eq!(parked & EX_TRANSPARENT, EX_TRANSPARENT);
        assert_eq!(parked & EX_NOREDIRECTIONBITMAP, EX_NOREDIRECTIONBITMAP);
        assert_eq!(parked_ex_style(parked), parked);
        // Showing it again is the same style it had before.
        assert_eq!(overlay_ex_style(parked, true), shown);
    }

    #[test]
    fn a_monitor_sized_window_is_borderless_unless_direct3d_owns_the_display() {
        assert_eq!(host_display(false, false), HostDisplay::Windowed);
        assert_eq!(host_display(true, false), HostDisplay::Borderless);
        assert_eq!(host_display(true, true), HostDisplay::Exclusive);
        // A windowed game while some other process is in exclusive mode is still
        // windowed, and the overlay keeps following it.
        assert_eq!(host_display(false, true), HostDisplay::Windowed);
    }

    #[test]
    fn the_overlay_is_raised_once_the_host_is_above_it() {
        let overlay = 0x11;
        let host = 0x22;
        assert!(!needs_raise(&[overlay, host], overlay, host));
        assert!(needs_raise(&[host, overlay], overlay, host));
        // Freshly created and not in the list yet: raise it.
        assert!(needs_raise(&[host], overlay, host));
        // No host on screen: there is nothing to be raised above.
        assert!(!needs_raise(&[overlay], overlay, host));
        assert!(!needs_raise(&[], overlay, host));
    }
}
