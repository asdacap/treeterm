//! Colours, ported from the `:root` tokens in `src/renderer/styles/index.css` (dark only,
//! as in Electron). Component colours come from GPUI Kit's theme, recoloured to match.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla, rgb};

pub struct Colors {
    pub bg_primary: Hsla,
    pub bg_secondary: Hsla,
    pub bg_tertiary: Hsla,
    pub bg_hover: Hsla,
    pub bg_active: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub error: Hsla,
}

impl Global for Colors {}

impl Colors {
    fn dark() -> Self {
        Self {
            bg_primary: rgb(0x1e1e1e).into(),
            bg_secondary: rgb(0x252526).into(),
            bg_tertiary: rgb(0x2d2d2d).into(),
            bg_hover: rgb(0x3c3c3c).into(),
            bg_active: rgb(0x094771).into(),
            text_primary: rgb(0xcccccc).into(),
            text_secondary: rgb(0x858585).into(),
            border: rgb(0x3c3c3c).into(),
            accent: rgb(0x0078d4).into(),
            error: rgb(0xf48771).into(),
        }
    }
}

pub fn colors(cx: &App) -> &Colors {
    cx.global::<Colors>()
}

pub fn install(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let c = Colors::dark();
    let theme = Theme::global_mut(cx);
    theme.background = c.bg_primary;
    theme.foreground = c.text_primary;
    theme.border = c.border;
    theme.muted_foreground = c.text_secondary;
    theme.sidebar = c.bg_secondary;
    theme.sidebar_foreground = c.text_primary;
    theme.sidebar_border = c.border;
    theme.list_hover = c.bg_hover;
    theme.list_active = c.bg_active;
    theme.accent = c.bg_hover;
    theme.primary = c.accent;
    theme.title_bar = c.bg_secondary;
    theme.title_bar_border = c.border;
    cx.set_global(c);
}
