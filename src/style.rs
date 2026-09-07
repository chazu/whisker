//! Static ANSI styling.
//!
//! Style is structured configuration, never text the user embeds, and it is
//! applied after layout. Widths are therefore always measured on plain text, so
//! an escape sequence can never be counted as a column nor be cut in half by
//! shortening. Control characters remain stripped from all metadata.
use toml::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    /// One of the 16 ANSI colours, which follow the terminal's own theme.
    Named(u8),
    /// A 256-colour palette index.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    fn parse(text: &str) -> Result<Self, String> {
        const NAMES: [&str; 8] = [
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        ];
        let plain = text.trim();
        if let Some(index) = NAMES.iter().position(|name| *name == plain) {
            return Ok(Self::Named(index as u8));
        }
        if let Some(rest) = plain.strip_prefix("bright_")
            && let Some(index) = NAMES.iter().position(|name| *name == rest)
        {
            return Ok(Self::Named(index as u8 + 8));
        }
        if let Some(hex) = plain.strip_prefix('#') {
            if hex.len() != 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
                return Err(format!("colour {plain} must look like #rrggbb"));
            }
            let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
            return Ok(Self::Rgb(byte(0), byte(2), byte(4)));
        }
        if let Ok(index) = plain.parse::<u8>() {
            return Ok(Self::Indexed(index));
        }
        Err(format!(
            "unknown colour: {plain}; use a name like red or bright_red, 0-255, or #rrggbb"
        ))
    }

    /// SGR parameters, offset to select foreground (30) or background (40).
    fn codes(self, base: u8) -> Vec<String> {
        match self {
            // 90-97 are the bright variants of the 30-37 range.
            Self::Named(index) if index < 8 => vec![(base + index).to_string()],
            Self::Named(index) => vec![(base + 52 + index).to_string()],
            Self::Indexed(index) => vec![(base + 8).to_string(), "5".into(), index.to_string()],
            Self::Rgb(r, g, b) => vec![
                (base + 8).to_string(),
                "2".into(),
                r.to_string(),
                g.to_string(),
                b.to_string(),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    pub fn is_plain(self) -> bool {
        self == Self::default()
    }

    /// Wrap already-laid-out text. Reset first, so a style cannot leak in from
    /// earlier output, and reset after, so it cannot leak into the user's input.
    pub fn paint(self, text: &str) -> String {
        if self.is_plain() || text.is_empty() {
            return text.to_owned();
        }
        let mut codes: Vec<String> = Vec::new();
        if self.bold {
            codes.push("1".into());
        }
        if self.dim {
            codes.push("2".into());
        }
        if self.italic {
            codes.push("3".into());
        }
        if self.underline {
            codes.push("4".into());
        }
        if let Some(fg) = self.fg {
            codes.extend(fg.codes(30));
        }
        if let Some(bg) = self.bg {
            codes.extend(bg.codes(40));
        }
        format!("\u{1b}[0;{}m{text}\u{1b}[0m", codes.join(";"))
    }

    /// Fields set here win; anything unset falls back to the view-wide style.
    pub fn or(self, fallback: Self) -> Self {
        Self {
            fg: self.fg.or(fallback.fg),
            bg: self.bg.or(fallback.bg),
            bold: self.bold || fallback.bold,
            dim: self.dim || fallback.dim,
            italic: self.italic || fallback.italic,
            underline: self.underline || fallback.underline,
        }
    }

    pub fn parse(value: &Value, what: &str) -> Result<Self, String> {
        let table = value.as_table().ok_or_else(|| {
            format!("{what} must be a table like {{ fg = \"red\", bold = true }}")
        })?;
        let mut style = Self::default();
        for (key, value) in table {
            let field = format!("{what}.{key}");
            match key.as_str() {
                "fg" | "bg" => {
                    let text = value
                        .as_str()
                        .map(str::to_owned)
                        // Allow a bare 256-colour index without quotes.
                        .or_else(|| value.as_integer().map(|n| n.to_string()))
                        .ok_or_else(|| {
                            format!("{field} must be a colour name, 0-255, or #rrggbb")
                        })?;
                    let color = Color::parse(&text).map_err(|error| format!("{field}: {error}"))?;
                    if key == "fg" {
                        style.fg = Some(color);
                    } else {
                        style.bg = Some(color);
                    }
                }
                "bold" | "dim" | "italic" | "underline" => {
                    let on = value
                        .as_bool()
                        .ok_or_else(|| format!("{field} must be true or false"))?;
                    match key.as_str() {
                        "bold" => style.bold = on,
                        "dim" => style.dim = on,
                        "italic" => style.italic = on,
                        _ => style.underline = on,
                    }
                }
                _ => {
                    return Err(format!(
                        "unknown style key {key} in {what}; use fg, bg, bold, dim, italic, underline"
                    ));
                }
            }
        }
        Ok(style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(text: &str) -> Result<Style, String> {
        let table: toml::Table = format!("s = {text}").parse().unwrap();
        Style::parse(table.get("s").unwrap(), "s")
    }

    #[test]
    fn named_colours_use_the_terminal_theme_range() {
        assert_eq!(
            style("{ fg = \"red\" }").unwrap().paint("x"),
            "\u{1b}[0;31mx\u{1b}[0m"
        );
        assert_eq!(
            style("{ fg = \"bright_red\" }").unwrap().paint("x"),
            "\u{1b}[0;91mx\u{1b}[0m"
        );
        assert_eq!(
            style("{ bg = \"blue\" }").unwrap().paint("x"),
            "\u{1b}[0;44mx\u{1b}[0m"
        );
    }

    #[test]
    fn indexed_and_rgb_colours_render() {
        assert_eq!(
            style("{ fg = 208 }").unwrap().paint("x"),
            "\u{1b}[0;38;5;208mx\u{1b}[0m"
        );
        assert_eq!(
            style("{ fg = \"#ff8800\" }").unwrap().paint("x"),
            "\u{1b}[0;38;2;255;136;0mx\u{1b}[0m"
        );
    }

    #[test]
    fn attributes_combine_and_always_reset() {
        let painted = style("{ fg = \"green\", bold = true, underline = true }")
            .unwrap()
            .paint("x");
        assert_eq!(painted, "\u{1b}[0;1;4;32mx\u{1b}[0m");
        assert!(painted.ends_with("\u{1b}[0m"), "must reset: {painted:?}");
    }

    #[test]
    fn a_plain_style_adds_nothing() {
        assert_eq!(Style::default().paint("x"), "x");
        assert!(Style::default().is_plain());
        // An explicitly false attribute is still plain.
        assert!(style("{ bold = false }").unwrap().is_plain());
    }

    #[test]
    fn styling_empty_text_stays_empty() {
        assert_eq!(style("{ fg = \"red\" }").unwrap().paint(""), "");
    }

    #[test]
    fn a_segment_style_overrides_the_view_style() {
        let view = style("{ fg = \"blue\", dim = true }").unwrap();
        let segment = style("{ fg = \"red\" }").unwrap();
        let merged = segment.or(view);
        assert_eq!(merged.fg, Some(Color::Named(1)));
        assert!(merged.dim, "view attributes should still apply");
    }

    #[test]
    fn bad_styles_are_reported() {
        for (text, expected) in [
            ("{ fg = \"puce\" }", "unknown colour"),
            ("{ fg = \"#ff88\" }", "#rrggbb"),
            ("{ fg = \"#gggggg\" }", "#rrggbb"),
            ("{ blink = true }", "unknown style key"),
            ("{ bold = \"yes\" }", "true or false"),
            ("\"red\"", "must be a table"),
        ] {
            let error = style(text).expect_err(&format!("{text} should fail"));
            assert!(
                error.to_lowercase().contains(&expected.to_lowercase()),
                "{text} gave {error:?}, wanted {expected:?}"
            );
        }
    }
}
