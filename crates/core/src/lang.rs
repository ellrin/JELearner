//! The two study languages.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ja];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }

    /// Name used in prompts (the model reads English best for instructions).
    pub fn english_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ja => "Japanese",
        }
    }

    /// Name shown in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "英文",
            Lang::Ja => "日文",
        }
    }

    /// Level scale used for this language: CEFR for English, JLPT for Japanese.
    pub fn levels(self) -> &'static [&'static str] {
        match self {
            Lang::En => &["A2", "B1", "B2", "C1", "C2"],
            Lang::Ja => &["N5", "N4", "N3", "N2", "N1"],
        }
    }

    pub fn default_level(self) -> &'static str {
        match self {
            Lang::En => "B2",
            Lang::Ja => "N3",
        }
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for Lang {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" | "eng" | "english" | "英文" | "英語" => Ok(Lang::En),
            "ja" | "jp" | "jpn" | "japanese" | "日文" | "日語" | "日本語" => Ok(Lang::Ja),
            other => Err(format!("unknown language: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_spellings() {
        assert_eq!("EN".parse::<Lang>(), Ok(Lang::En));
        assert_eq!("日本語".parse::<Lang>(), Ok(Lang::Ja));
        assert!("fr".parse::<Lang>().is_err());
    }
}
