//! Terminal colours: the ANSI palette of `src/renderer/components/terminalTheme.ts`, the
//! xterm 256-colour extension, and resolution of a cell's colour against the program's
//! OSC 4/10/11 overrides.

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::{COUNT, Colors};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// Foreground of every terminal. Matches the Electron theme.
pub const FOREGROUND: Rgb = rgb(0xd4d4d4);
pub const SELECTION_BACKGROUND: Rgb = rgb(0x264f78);
/// Background of a plain terminal tab.
pub const DEFAULT_BACKGROUND: Rgb = rgb(0x1e1e1e);
/// Background of a sandboxed terminal tab, so the sandbox is recognisable at a glance.
pub const SANDBOX_BACKGROUND: Rgb = rgb(0x1a1a2e);

const ANSI: [Rgb; 16] = [
    rgb(0x000000), // black
    rgb(0xcd3131), // red
    rgb(0x0dbc79), // green
    rgb(0xe5e510), // yellow
    rgb(0x2472c8), // blue
    rgb(0xbc3fbc), // magenta
    rgb(0x11a8cd), // cyan
    rgb(0xe5e5e5), // white
    rgb(0x666666), // bright black
    rgb(0xf14c4c), // bright red
    rgb(0x23d18b), // bright green
    rgb(0xf5f543), // bright yellow
    rgb(0x3b8eea), // bright blue
    rgb(0xd670d6), // bright magenta
    rgb(0x29b8db), // bright cyan
    rgb(0xffffff), // bright white
];

const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: ((hex >> 16) & 0xff) as u8,
        g: ((hex >> 8) & 0xff) as u8,
        b: (hex & 0xff) as u8,
    }
}

/// How much a dim (SGR 2) colour keeps of the normal one, as xterm.js renders it.
const DIM_FACTOR: f32 = 0.5;

fn dim(color: Rgb) -> Rgb {
    // Blend toward black; xterm.js draws dim text at half opacity over the background,
    // which against a near-black background is close to this.
    let scale = |c: u8| (f32::from(c) * DIM_FACTOR).round() as u8;
    Rgb {
        r: scale(color.r),
        g: scale(color.g),
        b: scale(color.b),
    }
}

/// Every colour a terminal can name, indexed like alacritty's [`Colors`].
#[derive(Clone)]
pub struct Palette {
    colors: [Rgb; COUNT],
}

impl Palette {
    pub fn new(background: Rgb) -> Self {
        let mut colors = [Rgb::default(); COUNT];
        colors[..16].copy_from_slice(&ANSI);
        let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
        for (cube, color) in colors[16..232].iter_mut().enumerate() {
            *color = Rgb {
                r: level(cube / 36),
                g: level((cube / 6) % 6),
                b: level(cube % 6),
            };
        }
        for (step, color) in colors[232..256].iter_mut().enumerate() {
            let gray = (8 + step * 10) as u8;
            *color = Rgb {
                r: gray,
                g: gray,
                b: gray,
            };
        }
        colors[NamedColor::Foreground as usize] = FOREGROUND;
        colors[NamedColor::Background as usize] = background;
        colors[NamedColor::Cursor as usize] = FOREGROUND;
        for (offset, normal) in ANSI[..8].iter().enumerate() {
            colors[NamedColor::DimBlack as usize + offset] = dim(*normal);
        }
        colors[NamedColor::BrightForeground as usize] = rgb(0xffffff);
        colors[NamedColor::DimForeground as usize] = dim(FOREGROUND);
        Self { colors }
    }

    pub fn background(&self) -> Rgb {
        self.colors[NamedColor::Background as usize]
    }

    /// Colour `index`, preferring what the program set with OSC 4/10/11/12.
    pub fn indexed(&self, overrides: &Colors, index: usize) -> Rgb {
        overrides[index].unwrap_or(self.colors[index])
    }

    /// A cell's foreground. Bold text in one of the eight base colours is drawn bright and
    /// dim text is darkened, as xterm.js does by default.
    pub fn foreground(&self, overrides: &Colors, color: Color, flags: Flags) -> Rgb {
        let color = match color {
            Color::Named(named) if flags.contains(Flags::DIM) && !flags.contains(Flags::BOLD) => {
                Color::Named(named.to_dim())
            }
            Color::Named(named) if flags.contains(Flags::BOLD) && (named as usize) < 8 => {
                Color::Named(named.to_bright())
            }
            Color::Indexed(index) if flags.contains(Flags::BOLD) && index < 8 => {
                Color::Indexed(index + 8)
            }
            other => other,
        };
        let resolved = self.resolve(overrides, color);
        match color {
            Color::Spec(_) | Color::Indexed(_) if flags.contains(Flags::DIM) => dim(resolved),
            _ => resolved,
        }
    }

    pub fn resolve(&self, overrides: &Colors, color: Color) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Named(named) => self.indexed(overrides, named as usize),
            Color::Indexed(index) => self.indexed(overrides, usize::from(index)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_colours_match_the_electron_theme() {
        let palette = Palette::new(DEFAULT_BACKGROUND);
        let none = Colors::default();
        assert_eq!(palette.indexed(&none, 1), rgb(0xcd3131));
        assert_eq!(palette.indexed(&none, 15), rgb(0xffffff));
        assert_eq!(palette.background(), DEFAULT_BACKGROUND);
        assert_eq!(
            palette.resolve(&none, Color::Named(NamedColor::Foreground)),
            FOREGROUND
        );
    }

    #[test]
    fn extended_colours_follow_xterm() {
        let palette = Palette::new(DEFAULT_BACKGROUND);
        let none = Colors::default();
        assert_eq!(palette.indexed(&none, 16), rgb(0x000000));
        assert_eq!(palette.indexed(&none, 196), rgb(0xff0000));
        assert_eq!(palette.indexed(&none, 231), rgb(0xffffff));
        assert_eq!(palette.indexed(&none, 232), rgb(0x080808));
        assert_eq!(palette.indexed(&none, 255), rgb(0xeeeeee));
    }

    #[test]
    fn program_overrides_win() {
        let palette = Palette::new(DEFAULT_BACKGROUND);
        let mut overrides = Colors::default();
        overrides[1] = Some(rgb(0x123456));
        assert_eq!(palette.indexed(&overrides, 1), rgb(0x123456));
    }

    #[test]
    fn bold_base_colours_are_bright_and_dim_is_darker() {
        let palette = Palette::new(DEFAULT_BACKGROUND);
        let none = Colors::default();
        assert_eq!(
            palette.foreground(&none, Color::Named(NamedColor::Red), Flags::BOLD),
            rgb(0xf14c4c)
        );
        assert_eq!(
            palette.foreground(&none, Color::Indexed(2), Flags::BOLD),
            rgb(0x23d18b)
        );
        let dimmed = palette.foreground(&none, Color::Named(NamedColor::White), Flags::DIM);
        assert!(dimmed.r < 0xe5);
        let dim_spec = palette.foreground(&none, Color::Spec(rgb(0x808080)), Flags::DIM);
        assert_eq!(dim_spec, rgb(0x404040));
    }
}
