//! As mesmas cores da web (`web/style.css`). O visual muda depois do MVP.

use iced::{Color, Theme, color, theme::Palette};

pub const SURFACE_CARD: Color = color!(0x2b2d31);
pub const MUTED: Color = color!(0x949ba4);

pub fn theme() -> Theme {
    Theme::custom(
        "Call",
        Palette {
            background: color!(0x313338),
            text: color!(0xdbdee1),
            primary: color!(0x5865f2),
            success: color!(0x23a55a),
            warning: color!(0xf0b232),
            danger: color!(0xf23f43),
        },
    )
}
