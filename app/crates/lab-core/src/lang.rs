//! The language of the reports.

/// Language of `report.html`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Russian.
    Ru,
    /// English.
    En,
}

impl Lang {
    /// Reads `ru` or `en`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "ru" => Some(Self::Ru),
            "en" => Some(Self::En),
            _ => None,
        }
    }

    /// Returns `ru` or `en`, whichever is in this language.
    #[must_use]
    pub fn pick<'a>(self, ru: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Ru => ru,
            Self::En => en,
        }
    }
}
