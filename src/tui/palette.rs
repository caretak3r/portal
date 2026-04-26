//! Color palettes for the TUI. A `Palette` is the resolved set of colors used
//! across `ui.rs`; `Theme` (in `config`) selects which palette to load.

use ratatui::style::Color;

use crate::config::Theme;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // fields are wired into render code incrementally
pub struct Palette {
    /// Selected list-row background.
    pub selection_bg: Color,
    /// Selected list-row text.
    pub selection_fg: Color,
    /// Active-profile dot and "active" badge.
    pub active: Color,
    /// Modified entries in diff views.
    pub modified: Color,
    /// Added entries in diff views.
    pub added: Color,
    /// Removed entries in diff views.
    pub removed: Color,
    /// Hint / footer text.
    pub hint: Color,
    /// Section headers / column labels.
    pub header: Color,
    /// Status-line accent.
    pub accent: Color,
    /// Subdued / secondary text.
    pub muted: Color,
}

impl Palette {
    #[must_use]
    pub const fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Default => DEFAULT,
            Theme::CatppuccinMocha => CATPPUCCIN_MOCHA,
            Theme::TokyoNight => TOKYO_NIGHT,
            Theme::SolarizedDark => SOLARIZED_DARK,
            Theme::GruvboxDark => GRUVBOX_DARK,
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

const DEFAULT: Palette = Palette {
    selection_bg: Color::DarkGray,
    selection_fg: Color::White,
    active: Color::Green,
    modified: Color::Yellow,
    added: Color::Green,
    removed: Color::Red,
    hint: Color::Yellow,
    header: Color::Cyan,
    accent: Color::Magenta,
    muted: Color::Gray,
};

const CATPPUCCIN_MOCHA: Palette = Palette {
    selection_bg: rgb(49, 50, 68),    // surface0
    selection_fg: rgb(205, 214, 244), // text
    active: rgb(166, 227, 161),       // green
    modified: rgb(249, 226, 175),     // yellow
    added: rgb(166, 227, 161),        // green
    removed: rgb(243, 139, 168),      // red
    hint: rgb(249, 226, 175),         // yellow
    header: rgb(137, 220, 235),       // sky
    accent: rgb(203, 166, 247),       // mauve
    muted: rgb(127, 132, 156),        // overlay1
};

const TOKYO_NIGHT: Palette = Palette {
    selection_bg: rgb(40, 52, 87),    // bg_highlight
    selection_fg: rgb(192, 202, 245), // fg
    active: rgb(158, 206, 106),       // green
    modified: rgb(224, 175, 104),     // orange
    added: rgb(158, 206, 106),        // green
    removed: rgb(247, 118, 142),      // red
    hint: rgb(224, 175, 104),         // orange
    header: rgb(125, 207, 255),       // blue
    accent: rgb(187, 154, 247),       // purple
    muted: rgb(86, 95, 137),          // comment
};

const SOLARIZED_DARK: Palette = Palette {
    selection_bg: rgb(7, 54, 66),     // base02
    selection_fg: rgb(238, 232, 213), // base2
    active: rgb(133, 153, 0),         // green
    modified: rgb(181, 137, 0),       // yellow
    added: rgb(133, 153, 0),          // green
    removed: rgb(220, 50, 47),        // red
    hint: rgb(181, 137, 0),           // yellow
    header: rgb(38, 139, 210),        // blue
    accent: rgb(108, 113, 196),       // violet
    muted: rgb(88, 110, 117),         // base01
};

const GRUVBOX_DARK: Palette = Palette {
    selection_bg: rgb(60, 56, 54),    // bg1
    selection_fg: rgb(235, 219, 178), // fg
    active: rgb(184, 187, 38),        // green
    modified: rgb(250, 189, 47),      // yellow
    added: rgb(184, 187, 38),         // green
    removed: rgb(251, 73, 52),        // red
    hint: rgb(254, 128, 25),          // orange
    header: rgb(131, 165, 152),       // aqua
    accent: rgb(211, 134, 155),       // pink
    muted: rgb(146, 131, 116),        // gray
};
