//! Azahar framebuffer layout.
//!
//! Both screens share one guest-pixel scale and are letterboxed into the render
//! area. `LargeScreen` draws the secondary screen at `1 / large_screen_proportion`.
//! `small_screen_position = 2` is bottom-right, matching Azahar's enum and the
//! layout recorded in `docs/SETUP_AZAHAR.md`.

use crate::camera::ScreenRect;

pub const BOTTOM_WIDTH: f32 = 320.0;
pub const BOTTOM_HEIGHT: f32 = 240.0;

/// `layout_option` in `qt-config.ini`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutOption {
    Default = 0,
    SingleScreen = 1,
    LargeScreen = 2,
    SideScreen = 3,
    SeparateWindows = 4,
    HybridScreen = 5,
    CustomLayout = 6,
}

impl LayoutOption {
    pub fn from_ini(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::Default,
            1 => Self::SingleScreen,
            2 => Self::LargeScreen,
            3 => Self::SideScreen,
            4 => Self::SeparateWindows,
            5 => Self::HybridScreen,
            6 => Self::CustomLayout,
            _ => return None,
        })
    }
}

/// `small_screen_position`. Ordinal 2 is [`Self::BottomRight`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmallScreenPosition {
    TopRight = 0,
    MiddleRight = 1,
    BottomRight = 2,
    TopLeft = 3,
    MiddleLeft = 4,
    BottomLeft = 5,
    Top = 6,
    Bottom = 7,
}

impl SmallScreenPosition {
    pub fn from_ini(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::TopRight,
            1 => Self::MiddleRight,
            2 => Self::BottomRight,
            3 => Self::TopLeft,
            4 => Self::MiddleLeft,
            5 => Self::BottomLeft,
            6 => Self::Top,
            7 => Self::Bottom,
            _ => return None,
        })
    }

    fn is_side(self) -> bool {
        !matches!(self, Self::Top | Self::Bottom)
    }

    fn on_left(self) -> bool {
        matches!(self, Self::TopLeft | Self::MiddleLeft | Self::BottomLeft)
    }
}

/// `aspect_ratio`. `Native` keeps 400:240 and 320:240.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AspectRatio {
    #[default]
    Native = 0,
    R16_9 = 1,
    R4_3 = 2,
    R21_9 = 3,
    R16_10 = 4,
}

impl AspectRatio {
    pub fn from_ini(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::Native,
            1 => Self::R16_9,
            2 => Self::R4_3,
            3 => Self::R21_9,
            4 => Self::R16_10,
            _ => return None,
        })
    }

    fn ratio(self) -> Option<f32> {
        match self {
            Self::Native => None,
            Self::R16_9 => Some(16.0 / 9.0),
            Self::R4_3 => Some(4.0 / 3.0),
            Self::R21_9 => Some(21.0 / 9.0),
            Self::R16_10 => Some(16.0 / 10.0),
        }
    }
}

/// Normalized or pixel rectangle from `custom_*` keys.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CustomRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl CustomRect {
    fn is_unset(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    fn is_fraction(self) -> bool {
        self.x <= 1.0 && self.y <= 1.0 && self.width <= 1.0 && self.height <= 1.0
    }
}

/// Inputs that select a layout. Sizes are the render area, after the status bar inset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutInput {
    pub width: u32,
    pub height: u32,
    pub option: LayoutOption,
    pub large_screen_proportion: f32,
    pub small_screen_position: SmallScreenPosition,
    pub swap_screen: bool,
    pub upright_screen: bool,
    pub screen_top_stretch: bool,
    pub aspect_ratio: AspectRatio,
    pub custom_top: CustomRect,
    pub custom_bottom: CustomRect,
    /// Second window of `SeparateWindows` (the screen that is not in the main window).
    pub separate_secondary: bool,
}

impl LayoutInput {
    /// Large screen, proportion 4, bottom-right touch screen. The machine's usual layout.
    pub fn large_bottom_right(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            option: LayoutOption::LargeScreen,
            large_screen_proportion: 4.0,
            small_screen_position: SmallScreenPosition::BottomRight,
            swap_screen: false,
            upright_screen: false,
            screen_top_stretch: false,
            aspect_ratio: AspectRatio::Native,
            custom_top: CustomRect::default(),
            custom_bottom: CustomRect::default(),
            separate_secondary: false,
        }
    }
}

/// Integer screen rectangle. Origin is the top-left of the render area, Y grows down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn to_screen(self) -> ScreenRect {
        ScreenRect::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenRects {
    pub top: Rect,
    pub bottom: Rect,
}

impl ScreenRects {
    fn empty() -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy)]
struct Area {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Place the top and bottom screens inside the render area.
pub fn resolve(input: &LayoutInput) -> ScreenRects {
    if input.width == 0 || input.height == 0 {
        return ScreenRects::empty();
    }

    let window = Area {
        x: 0.0,
        y: 0.0,
        w: input.width as f32,
        h: input.height as f32,
    };
    let area = match input.aspect_ratio.ratio() {
        Some(aspect) if input.option != LayoutOption::CustomLayout => letterbox(window, aspect),
        _ => window,
    };

    let mut rects = match input.option {
        LayoutOption::Default => stacked(area, input.swap_screen, input.upright_screen),
        LayoutOption::SingleScreen => single(area, input.swap_screen, input.upright_screen),
        LayoutOption::LargeScreen => large(area, input),
        LayoutOption::SideScreen => side_by_side(area, input.swap_screen, input.upright_screen),
        LayoutOption::SeparateWindows => separate(area, input),
        LayoutOption::HybridScreen => hybrid(area, input),
        LayoutOption::CustomLayout => custom(window, input),
    };
    if input.screen_top_stretch {
        rects.top = Rect {
            x: 0,
            y: 0,
            width: input.width as i32,
            height: input.height as i32,
        };
    }
    rects
}

fn natives(upright: bool) -> (f32, f32, f32, f32) {
    if upright {
        (
            crate::TOP_HEIGHT,
            crate::TOP_WIDTH,
            BOTTOM_HEIGHT,
            BOTTOM_WIDTH,
        )
    } else {
        (
            crate::TOP_WIDTH,
            crate::TOP_HEIGHT,
            BOTTOM_WIDTH,
            BOTTOM_HEIGHT,
        )
    }
}

fn px(value: f32) -> i32 {
    value.round() as i32
}

fn letterbox(window: Area, aspect: f32) -> Area {
    let window_aspect = window.w / window.h;
    if window_aspect > aspect {
        let h = window.h;
        let w = (h * aspect).round();
        let x = window.x + ((window.w - w) / 2.0).round();
        Area {
            x,
            y: window.y,
            w,
            h,
        }
    } else {
        let w = window.w;
        let h = (w / aspect).round();
        let y = window.y + ((window.h - h) / 2.0).round();
        Area {
            x: window.x,
            y,
            w,
            h,
        }
    }
}

fn fit(area: Area, native_w: f32, native_h: f32) -> Rect {
    let scale = (area.w / native_w).min(area.h / native_h);
    let width = px(native_w * scale);
    let height = px(native_h * scale);
    Rect {
        x: px(area.x + (area.w - width as f32) / 2.0),
        y: px(area.y + (area.h - height as f32) / 2.0),
        width,
        height,
    }
}

fn single(area: Area, swapped: bool, upright: bool) -> ScreenRects {
    let (top_w, top_h, bot_w, bot_h) = natives(upright);
    if swapped {
        ScreenRects {
            top: Rect::default(),
            bottom: fit(area, bot_w, bot_h),
        }
    } else {
        ScreenRects {
            top: fit(area, top_w, top_h),
            bottom: Rect::default(),
        }
    }
}

fn stacked(area: Area, swapped: bool, upright: bool) -> ScreenRects {
    let (top_w, top_h, bot_w, bot_h) = natives(upright);
    let scale = (area.w / top_w.max(bot_w)).min(area.h / (top_h + bot_h));
    let top_pw = px(top_w * scale);
    let top_ph = px(top_h * scale);
    let bot_pw = px(bot_w * scale);
    let bot_ph = px(bot_h * scale);
    let content_w = top_pw.max(bot_pw);
    let origin_x = area.x + (area.w - content_w as f32) / 2.0;
    let origin_y = area.y + (area.h - (top_ph + bot_ph) as f32) / 2.0;
    let top = Rect {
        x: px(origin_x + (content_w - top_pw) as f32 / 2.0),
        y: px(if swapped {
            origin_y + bot_ph as f32
        } else {
            origin_y
        }),
        width: top_pw,
        height: top_ph,
    };
    let bottom = Rect {
        x: px(origin_x + (content_w - bot_pw) as f32 / 2.0),
        y: px(if swapped {
            origin_y
        } else {
            origin_y + top_ph as f32
        }),
        width: bot_pw,
        height: bot_ph,
    };
    ScreenRects { top, bottom }
}

fn side_by_side(area: Area, swapped: bool, upright: bool) -> ScreenRects {
    let (top_w, top_h, bot_w, bot_h) = natives(upright);
    let scale = (area.w / (top_w + bot_w)).min(area.h / top_h.max(bot_h));
    let top_pw = px(top_w * scale);
    let top_ph = px(top_h * scale);
    let bot_pw = px(bot_w * scale);
    let bot_ph = px(bot_h * scale);
    let content_w = top_pw + bot_pw;
    let content_h = top_ph.max(bot_ph);
    let origin_x = area.x + (area.w - content_w as f32) / 2.0;
    let origin_y = area.y + (area.h - content_h as f32) / 2.0;
    let top_x = if swapped {
        origin_x + bot_pw as f32
    } else {
        origin_x
    };
    let bot_x = if swapped {
        origin_x
    } else {
        origin_x + top_pw as f32
    };
    ScreenRects {
        top: Rect {
            x: px(top_x),
            y: px(origin_y + (content_h - top_ph) as f32 / 2.0),
            width: top_pw,
            height: top_ph,
        },
        bottom: Rect {
            x: px(bot_x),
            y: px(origin_y + (content_h - bot_ph) as f32 / 2.0),
            width: bot_pw,
            height: bot_ph,
        },
    }
}

fn large(area: Area, input: &LayoutInput) -> ScreenRects {
    let (top_w, top_h, bot_w, bot_h) = natives(input.upright_screen);
    let proportion = input.large_screen_proportion.clamp(1.0, 16.0);
    let small_scale = 1.0 / proportion;
    let (large_w, large_h, small_w, small_h, large_is_top) = if input.swap_screen {
        (
            bot_w,
            bot_h,
            top_w * small_scale,
            top_h * small_scale,
            false,
        )
    } else {
        (top_w, top_h, bot_w * small_scale, bot_h * small_scale, true)
    };

    let side = input.small_screen_position.is_side();
    let content_w = if side {
        large_w + small_w
    } else {
        large_w.max(small_w)
    };
    let content_h = if side {
        large_h.max(small_h)
    } else {
        large_h + small_h
    };
    let scale = (area.w / content_w).min(area.h / content_h);
    let large_pw = px(large_w * scale);
    let large_ph = px(large_h * scale);
    let small_pw = px(small_w * scale);
    let small_ph = px(small_h * scale);
    let total_w = if side {
        large_pw + small_pw
    } else {
        large_pw.max(small_pw)
    };
    let total_h = if side {
        large_ph.max(small_ph)
    } else {
        large_ph + small_ph
    };
    let origin_x = px(area.x + (area.w - total_w as f32) / 2.0);
    let origin_y = px(area.y + (area.h - total_h as f32) / 2.0);

    let (large_x, small_x) = if input.small_screen_position.on_left() {
        (origin_x + small_pw, origin_x)
    } else if side {
        (origin_x, origin_x + large_pw)
    } else {
        (
            origin_x + (total_w - large_pw) / 2,
            origin_x + (total_w - small_pw) / 2,
        )
    };

    let (large_y, small_y) = if side {
        let small_slack = total_h - small_ph;
        let large_y = origin_y + (total_h - large_ph) / 2;
        let small_y = match input.small_screen_position {
            SmallScreenPosition::TopLeft | SmallScreenPosition::TopRight => origin_y,
            SmallScreenPosition::MiddleLeft | SmallScreenPosition::MiddleRight => {
                origin_y + small_slack / 2
            }
            _ => origin_y + small_slack,
        };
        (large_y, small_y)
    } else if input.small_screen_position == SmallScreenPosition::Top {
        (origin_y + small_ph, origin_y)
    } else {
        (origin_y, origin_y + large_ph)
    };

    let large_rect = Rect {
        x: large_x,
        y: large_y,
        width: large_pw,
        height: large_ph,
    };
    let small_rect = Rect {
        x: small_x,
        y: small_y,
        width: small_pw,
        height: small_ph,
    };
    if large_is_top {
        ScreenRects {
            top: large_rect,
            bottom: small_rect,
        }
    } else {
        ScreenRects {
            top: small_rect,
            bottom: large_rect,
        }
    }
}

fn separate(area: Area, input: &LayoutInput) -> ScreenRects {
    let show_top = input.separate_secondary == input.swap_screen;
    single(area, !show_top, input.upright_screen)
}

fn hybrid(area: Area, input: &LayoutInput) -> ScreenRects {
    let (top_w, top_h, bot_w, bot_h) = natives(input.upright_screen);
    let proportion = input.large_screen_proportion.clamp(1.0, 16.0);
    let (large_w, large_h, small_w, small_h, large_is_top) = if input.swap_screen {
        (bot_w, bot_h, top_w, top_h, false)
    } else {
        (top_w, top_h, bot_w, bot_h, true)
    };
    let large_rect = fit(area, large_w, large_h);
    let scale = large_rect.width as f32 / large_w;
    let small_rect = place_overlay(
        large_rect,
        px(small_w * scale / proportion),
        px(small_h * scale / proportion),
        input.small_screen_position,
    );
    if large_is_top {
        ScreenRects {
            top: large_rect,
            bottom: small_rect,
        }
    } else {
        ScreenRects {
            top: small_rect,
            bottom: large_rect,
        }
    }
}

fn place_overlay(host: Rect, width: i32, height: i32, position: SmallScreenPosition) -> Rect {
    let x = if position.on_left() {
        host.x
    } else if position.is_side() {
        host.x + host.width - width
    } else {
        host.x + (host.width - width) / 2
    };
    let y = match position {
        SmallScreenPosition::TopLeft | SmallScreenPosition::TopRight | SmallScreenPosition::Top => {
            host.y
        }
        SmallScreenPosition::MiddleLeft | SmallScreenPosition::MiddleRight => {
            host.y + (host.height - height) / 2
        }
        SmallScreenPosition::BottomLeft
        | SmallScreenPosition::BottomRight
        | SmallScreenPosition::Bottom => host.y + host.height - height,
    };
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn custom(window: Area, input: &LayoutInput) -> ScreenRects {
    let top = if input.custom_top.is_unset() {
        fit(window, crate::TOP_WIDTH, crate::TOP_HEIGHT)
    } else {
        custom_rect(window, input.custom_top)
    };
    let bottom = if input.custom_bottom.is_unset() {
        Rect::default()
    } else {
        custom_rect(window, input.custom_bottom)
    };
    ScreenRects { top, bottom }
}

fn custom_rect(window: Area, custom: CustomRect) -> Rect {
    if custom.is_fraction() {
        Rect {
            x: px(window.x + custom.x * window.w),
            y: px(window.y + custom.y * window.h),
            width: px(custom.width * window.w),
            height: px(custom.height * window.h),
        }
    } else {
        Rect {
            x: px(custom.x),
            y: px(custom.y),
            width: px(custom.width),
            height: px(custom.height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn large_bottom_right_proportion_4_matches_golden_windows() {
        let cases = [
            (960, 480, rect(0, 0, 800, 480), rect(800, 360, 160, 120)),
            (
                1920,
                1080,
                rect(0, 60, 1600, 960),
                rect(1600, 780, 320, 240),
            ),
            (
                2560,
                1440,
                rect(0, 80, 2133, 1280),
                rect(2133, 1040, 427, 320),
            ),
        ];
        for (width, height, top, bottom) in cases {
            let got = resolve(&LayoutInput::large_bottom_right(width, height));
            assert_eq!(got.top, top, "{width}x{height} top");
            assert_eq!(got.bottom, bottom, "{width}x{height} bottom");
        }
    }

    #[test]
    fn all_eight_small_screen_positions_stay_inside() {
        let expected = [
            (SmallScreenPosition::TopRight, rect(800, 0, 160, 120)),
            (SmallScreenPosition::MiddleRight, rect(800, 180, 160, 120)),
            (SmallScreenPosition::BottomRight, rect(800, 360, 160, 120)),
            (SmallScreenPosition::TopLeft, rect(0, 0, 160, 120)),
            (SmallScreenPosition::MiddleLeft, rect(0, 180, 160, 120)),
            (SmallScreenPosition::BottomLeft, rect(0, 360, 160, 120)),
            (SmallScreenPosition::Top, rect(320, 0, 160, 120)),
            (SmallScreenPosition::Bottom, rect(320, 480, 160, 120)),
        ];
        for (position, bottom) in expected {
            let mut input = if matches!(
                position,
                SmallScreenPosition::Top | SmallScreenPosition::Bottom
            ) {
                LayoutInput::large_bottom_right(800, 600)
            } else {
                LayoutInput::large_bottom_right(960, 480)
            };
            input.small_screen_position = position;
            let got = resolve(&input);
            assert_eq!(got.bottom, bottom, "{position:?}");
            assert!(got.top.width > 0 && got.top.height > 0);
        }
    }

    #[test]
    fn swap_puts_the_top_screen_in_the_corner() {
        let mut input = LayoutInput::large_bottom_right(840, 480);
        input.swap_screen = true;
        let got = resolve(&input);
        assert_eq!(got.bottom, rect(0, 0, 640, 480));
        assert_eq!(got.top, rect(640, 360, 200, 120));
    }

    #[test]
    fn default_single_and_side_layouts() {
        let mut input = LayoutInput::large_bottom_right(800, 960);
        input.option = LayoutOption::Default;
        let got = resolve(&input);
        assert_eq!(got.top, rect(0, 0, 800, 480));
        assert_eq!(got.bottom, rect(80, 480, 640, 480));

        input.option = LayoutOption::SingleScreen;
        input.height = 480;
        let got = resolve(&input);
        assert_eq!(got.top, rect(0, 0, 800, 480));
        assert_eq!(got.bottom, Rect::default());

        input.swap_screen = true;
        let got = resolve(&input);
        assert_eq!(got.top, Rect::default());
        assert_eq!(got.bottom, rect(80, 0, 640, 480));

        input.swap_screen = false;
        input.option = LayoutOption::SideScreen;
        input.width = 1440;
        let got = resolve(&input);
        assert_eq!(got.top, rect(0, 0, 800, 480));
        assert_eq!(got.bottom, rect(800, 0, 640, 480));
    }

    #[test]
    fn separate_windows_show_one_screen() {
        let mut input = LayoutInput::large_bottom_right(800, 600);
        input.option = LayoutOption::SeparateWindows;
        let main = resolve(&input);
        assert_eq!(main.top, rect(0, 60, 800, 480));
        assert_eq!(main.bottom, Rect::default());

        input.separate_secondary = true;
        let secondary = resolve(&input);
        assert_eq!(secondary.top, Rect::default());
        assert_eq!(secondary.bottom, rect(0, 0, 800, 600));
    }

    #[test]
    fn hybrid_overlays_the_small_screen() {
        let mut input = LayoutInput::large_bottom_right(800, 480);
        input.option = LayoutOption::HybridScreen;
        let got = resolve(&input);
        assert_eq!(got.top, rect(0, 0, 800, 480));
        assert_eq!(got.bottom, rect(640, 360, 160, 120));
    }

    #[test]
    fn stretch_fills_the_window_with_the_top_screen() {
        let mut input = LayoutInput::large_bottom_right(960, 480);
        input.screen_top_stretch = true;
        let got = resolve(&input);
        assert_eq!(got.top, rect(0, 0, 960, 480));
        assert_eq!(got.bottom, rect(800, 360, 160, 120));
    }

    #[test]
    fn aspect_ratio_letterboxes_before_placing_screens() {
        let mut input = LayoutInput::large_bottom_right(1600, 1600);
        input.aspect_ratio = AspectRatio::R16_9;
        let got = resolve(&input);
        assert_eq!(got.top, rect(0, 400, 1333, 800));
        assert_eq!(got.bottom, rect(1333, 1000, 267, 200));
    }

    #[test]
    fn upright_single_screen_is_portrait() {
        let mut input = LayoutInput::large_bottom_right(600, 800);
        input.option = LayoutOption::SingleScreen;
        input.upright_screen = true;
        let got = resolve(&input);
        assert_eq!(got.top, rect(60, 0, 480, 800));
        assert!(got.top.height > got.top.width);
    }

    #[test]
    fn custom_fractions_and_pixels() {
        let mut input = LayoutInput::large_bottom_right(1000, 500);
        input.option = LayoutOption::CustomLayout;
        input.custom_top = CustomRect {
            x: 0.1,
            y: 0.2,
            width: 0.5,
            height: 0.4,
        };
        input.custom_bottom = CustomRect {
            x: 10.0,
            y: 20.0,
            width: 320.0,
            height: 240.0,
        };
        let got = resolve(&input);
        assert_eq!(got.top, rect(100, 100, 500, 200));
        assert_eq!(got.bottom, rect(10, 20, 320, 240));
    }

    #[test]
    fn empty_window_yields_empty_rects() {
        let got = resolve(&LayoutInput::large_bottom_right(0, 100));
        assert_eq!(got, ScreenRects::empty());
    }
}
