//! As mesmas cores da web (`web/style.css`). O visual muda depois do MVP.

use iced::{Color, Theme, color, theme::Palette};

/// `--bg-0`: barra de servidores e painel do usuário.
pub const SURFACE_DARK: Color = color!(0x1e1f22);
/// `--bg-1`: cartões e lista de membros.
pub const SURFACE_CARD: Color = color!(0x2b2d31);
pub const MUTED: Color = color!(0x949ba4);
pub const ONLINE: Color = color!(0x23a55a);
pub const DANGER: Color = color!(0xf23f43);
/// Fundo da faixa de aviso.
pub const DANGER_BG: Color = color!(0x5c1d1f);

pub fn theme() -> Theme {
    Theme::custom(
        "Call",
        Palette {
            background: color!(0x313338),
            text: color!(0xdbdee1),
            primary: color!(0x5865f2),
            success: ONLINE,
            warning: color!(0xf0b232),
            danger: DANGER,
        },
    )
}
