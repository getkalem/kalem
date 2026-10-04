//! Character formatting and paragraph alignment the views draw: the size
//! LaTeX's `\large` and `\fontsize` give, its colors, and the alignment of
//! LaTeX's `center`, `flushright` and `flushleft` and of Org's center
//! blocks.
//!
//! Earlier versions of Kalem wrote their own formatting into Org files
//! (`@@kalem:…@@`, `#+ATTR_KALEM:`, `#+KALEM:`); that ended with T2.13.13.
//! `crate::kinds` still finds it in old files.

use std::sync::{Arc, Mutex};

use crate::theme::Color;

/// A font family, interned so that styles stay `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontName(u32);

static FONTS: Mutex<Vec<Arc<str>>> = Mutex::new(Vec::new());

impl FontName {
    /// The name for `family`.
    pub fn new(family: &str) -> FontName {
        let mut fonts = FONTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(i) = fonts.iter().position(|f| &**f == family) {
            return FontName(i as u32);
        }
        fonts.push(family.into());
        FontName(fonts.len() as u32 - 1)
    }

    /// The family.
    pub fn family(self) -> Arc<str> {
        FONTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[self.0 as usize]
            .clone()
    }
}

/// The formatting of a span of text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CharFormat {
    /// The font family.
    pub font: Option<FontName>,
    /// The size, in tenths of a point.
    pub size: Option<u16>,
    /// The text color.
    pub color: Option<Color>,
    /// The highlight (background) color.
    pub highlight: Option<Color>,
}

impl CharFormat {
    /// Nothing set.
    pub fn is_empty(&self) -> bool {
        *self == CharFormat::default()
    }

    /// `self` with what `inner` sets on top.
    pub fn with(self, inner: CharFormat) -> CharFormat {
        CharFormat {
            font: inner.font.or(self.font),
            size: inner.size.or(self.size),
            color: inner.color.or(self.color),
            highlight: inner.highlight.or(self.highlight),
        }
    }
}

/// Paragraph alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Flush left, the default.
    #[default]
    Left,
    /// Centered.
    Center,
    /// Flush right.
    Right,
    /// Justified.
    Justify,
}
