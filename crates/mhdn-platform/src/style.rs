//! AppKit flags the overlay window must carry. Values match AppKit's `NSWindow.h`.

#![forbid(unsafe_code)]

/// `NSWindowCollectionBehaviorCanJoinAllSpaces`.
pub const CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
/// `NSWindowCollectionBehaviorStationary`.
pub const STATIONARY: u64 = 1 << 4;
/// `NSWindowCollectionBehaviorIgnoresCycle`.
pub const IGNORES_CYCLE: u64 = 1 << 6;
/// `NSWindowCollectionBehaviorFullScreenAuxiliary`.
pub const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

/// `NSScreenSaverWindowLevel`. Above normal and floating windows, including fullscreen apps.
pub const SCREEN_SAVER_LEVEL: i64 = 1000;

/// Joined spaces, stationary, ignored by Cmd-`, and allowed beside a fullscreen app.
pub fn overlay_collection_behavior() -> u64 {
    CAN_JOIN_ALL_SPACES | STATIONARY | IGNORES_CYCLE | FULL_SCREEN_AUXILIARY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_behavior_is_the_four_fullscreen_flags() {
        assert_eq!(
            overlay_collection_behavior(),
            CAN_JOIN_ALL_SPACES | STATIONARY | IGNORES_CYCLE | FULL_SCREEN_AUXILIARY
        );
        assert_eq!(overlay_collection_behavior() & (1 << 5), 0);
        assert_eq!(SCREEN_SAVER_LEVEL, 1000);
    }
}
