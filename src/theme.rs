use ratatui::style::Color;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThemeKind {
    #[default]
    Mocha,
    Latte,
}

impl ThemeKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "mocha" | "dark" => Some(Self::Mocha),
            "latte" | "light" => Some(Self::Latte),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Mocha => "mocha",
            Self::Latte => "latte",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Mocha => Self::Latte,
            Self::Latte => Self::Mocha,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub border: Color,
    pub selected: Color,
    pub own: Color,
    pub other: Color,
    pub error: Color,
    pub success: Color,
    pub warning: Color,
    pub mention: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(ThemeKind::Mocha)
    }
}

impl Theme {
    pub fn new(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Mocha => Self {
                bg: Color::Rgb(30, 30, 46),
                fg: Color::Rgb(205, 214, 244),
                dim: Color::Rgb(108, 112, 134),
                accent: Color::Rgb(137, 180, 250),
                border: Color::Rgb(69, 71, 90),
                selected: Color::Rgb(49, 50, 68),
                own: Color::Rgb(69, 71, 90),
                other: Color::Rgb(24, 24, 37),
                error: Color::Rgb(243, 139, 168),
                success: Color::Rgb(166, 227, 161),
                warning: Color::Rgb(249, 226, 175),
                mention: Color::Rgb(245, 194, 231),
            },
            ThemeKind::Latte => Self {
                bg: Color::Rgb(239, 241, 245),
                fg: Color::Rgb(76, 79, 105),
                dim: Color::Rgb(156, 160, 176),
                accent: Color::Rgb(30, 102, 245),
                border: Color::Rgb(188, 192, 204),
                selected: Color::Rgb(204, 208, 218),
                own: Color::Rgb(220, 224, 232),
                other: Color::Rgb(230, 233, 239),
                error: Color::Rgb(210, 15, 57),
                success: Color::Rgb(64, 160, 43),
                warning: Color::Rgb(223, 142, 29),
                mention: Color::Rgb(136, 57, 239),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_parse_and_toggle() {
        assert_eq!(ThemeKind::parse("dark"), Some(ThemeKind::Mocha));
        assert_eq!(ThemeKind::parse("LIGHT"), Some(ThemeKind::Latte));
        assert_eq!(ThemeKind::Mocha.toggled(), ThemeKind::Latte);
    }
}
