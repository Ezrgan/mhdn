//! Copy on-screen windows and displays. Coordinates stay top-left, matching winit.

use std::ffi::c_void;

use core_foundation::array::{CFArray, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryGetValue, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;

use crate::geom::Rect;
use crate::select::HostWindow;
use crate::track::Display;

const ON_SCREEN_ONLY: u32 = 1;
const EXCLUDE_DESKTOP: u32 = 1 << 4;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
    static kCGWindowNumber: CFStringRef;
    static kCGWindowOwnerName: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowBounds: CFStringRef;
    static kCGWindowIsOnscreen: CFStringRef;
}

pub fn list_windows() -> Vec<HostWindow> {
    let raw = unsafe { CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP, 0) };
    if raw.is_null() {
        return Vec::new();
    }
    let array = unsafe { CFArray::<CFDictionary>::wrap_under_create_rule(raw) };
    let mut windows = Vec::new();
    for index in 0..array.len() {
        let value = unsafe { CFArrayGetValueAtIndex(array.as_concrete_TypeRef(), index) };
        if value.is_null() {
            continue;
        }
        let dict = unsafe { CFDictionary::wrap_under_get_rule(value as CFDictionaryRef) };
        if let Some(window) = parse_window(&dict) {
            windows.push(window);
        }
    }
    windows
}

pub fn list_displays() -> Vec<Display> {
    let Some(marker) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(marker);
    let count = screens.count();
    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        frames.push(screens.objectAtIndex(index));
    }
    let primary = frames
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            frame.origin.x == 0.0 && frame.origin.y == 0.0
        })
        .map(|screen| screen.frame().size.height)
        .unwrap_or(0.0);
    frames
        .iter()
        .map(|screen| {
            let frame = screen.frame();
            let width = cgfloat_f32(frame.size.width);
            let height = cgfloat_f32(frame.size.height);
            Display {
                frame: Rect::new(
                    cgfloat_f32(frame.origin.x),
                    cgfloat_f32(primary) - cgfloat_f32(frame.origin.y) - height,
                    width,
                    height,
                ),
                scale: cgfloat_f32(screen.backingScaleFactor()),
            }
        })
        .collect()
}

fn parse_window(dict: &CFDictionary) -> Option<HostWindow> {
    let id = dict_i64(dict, unsafe { kCGWindowNumber })?
        .try_into()
        .ok()?;
    let owner_name = dict_string(dict, unsafe { kCGWindowOwnerName }).unwrap_or_default();
    let owner_pid = dict_i64(dict, unsafe { kCGWindowOwnerPID })
        .and_then(|pid| i32::try_from(pid).ok())
        .unwrap_or(0);
    let layer = dict_i64(dict, unsafe { kCGWindowLayer })
        .and_then(|layer| i32::try_from(layer).ok())
        .unwrap_or(0);
    let onscreen = dict_bool(dict, unsafe { kCGWindowIsOnscreen }).unwrap_or(true);
    let bounds = dict_bounds(dict)?;
    Some(HostWindow {
        id,
        owner_name,
        owner_pid,
        layer,
        bounds,
        onscreen,
    })
}

fn dict_bounds(dict: &CFDictionary) -> Option<Rect> {
    let raw = dict_value(dict, unsafe { kCGWindowBounds });
    if raw.is_null() {
        return None;
    }
    let bounds = unsafe { CFDictionary::wrap_under_get_rule(raw as CFDictionaryRef) };
    let x_key = CFString::from_static_string("X");
    let y_key = CFString::from_static_string("Y");
    let w_key = CFString::from_static_string("Width");
    let h_key = CFString::from_static_string("Height");
    Some(Rect::new(
        dict_f64(&bounds, x_key.as_concrete_TypeRef())? as f32,
        dict_f64(&bounds, y_key.as_concrete_TypeRef())? as f32,
        dict_f64(&bounds, w_key.as_concrete_TypeRef())? as f32,
        dict_f64(&bounds, h_key.as_concrete_TypeRef())? as f32,
    ))
}

fn dict_value(dict: &CFDictionary, key: CFStringRef) -> *const c_void {
    unsafe { CFDictionaryGetValue(dict.as_concrete_TypeRef(), key.cast()) }
}

fn dict_f64(dict: &CFDictionary, key: CFStringRef) -> Option<f64> {
    let raw = dict_value(dict, key);
    if raw.is_null() {
        return None;
    }
    unsafe { CFNumber::wrap_under_get_rule(raw.cast()) }.to_f64()
}

fn dict_i64(dict: &CFDictionary, key: CFStringRef) -> Option<i64> {
    let raw = dict_value(dict, key);
    if raw.is_null() {
        return None;
    }
    unsafe { CFNumber::wrap_under_get_rule(raw.cast()) }.to_i64()
}

fn dict_bool(dict: &CFDictionary, key: CFStringRef) -> Option<bool> {
    let raw = dict_value(dict, key);
    if raw.is_null() {
        return None;
    }
    Some(bool::from(unsafe {
        CFBoolean::wrap_under_get_rule(raw.cast())
    }))
}

fn dict_string(dict: &CFDictionary, key: CFStringRef) -> Option<String> {
    let raw = dict_value(dict, key);
    if raw.is_null() {
        return None;
    }
    Some(unsafe { CFString::wrap_under_get_rule(raw.cast()) }.to_string())
}

fn cgfloat_f32(value: impl Into<f64>) -> f32 {
    let value = value.into();
    if value.is_finite() {
        value as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_windows_does_not_panic() {
        let _ = list_windows();
    }
}
