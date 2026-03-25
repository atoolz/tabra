//! Color theme and icon mapping for the autocomplete popup.
//!
//! Uses ANSI 256-color codes for broad terminal compatibility.
//! The theme is designed to be visible on both dark and light backgrounds.

use crate::spec::types::SuggestionType;
use crossterm::style::Color;

/// Visual theme for the popup overlay.
pub struct Theme {
    /// Background color of the popup.
    pub popup_bg: Color,
    /// Default text color.
    pub text_fg: Color,
    /// Color for the selected item row.
    pub selected_bg: Color,
    /// Color for matched characters (highlight).
    pub match_fg: Color,
    /// Color for description text.
    pub desc_fg: Color,
    /// Color for the kind icon/badge.
    pub kind_fg: Color,
    /// Color for dangerous items.
    pub danger_fg: Color,
    /// Border color.
    pub border_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            popup_bg: Color::Rgb {
                r: 30,
                g: 30,
                b: 46,
            }, // dark surface
            text_fg: Color::Rgb {
                r: 205,
                g: 214,
                b: 244,
            }, // light text
            selected_bg: Color::Rgb {
                r: 69,
                g: 71,
                b: 90,
            }, // subtle highlight
            match_fg: Color::Rgb {
                r: 137,
                g: 180,
                b: 250,
            }, // blue accent
            desc_fg: Color::Rgb {
                r: 127,
                g: 132,
                b: 156,
            }, // muted
            kind_fg: Color::Rgb {
                r: 166,
                g: 227,
                b: 161,
            }, // green
            danger_fg: Color::Rgb {
                r: 243,
                g: 139,
                b: 168,
            }, // red
            border_fg: Color::Rgb {
                r: 88,
                g: 91,
                b: 112,
            }, // border
        }
    }
}

/// Map a SuggestionType to a single-character icon for the popup.
pub fn kind_icon(kind: SuggestionType) -> char {
    match kind {
        SuggestionType::Subcommand => '\u{f0a0e}', // nerd font: command
        SuggestionType::Option => '\u{eb88}',      // nerd font: symbol-key
        SuggestionType::Arg => '\u{eb69}',         // nerd font: symbol-variable
        SuggestionType::File => '\u{ea7b}',        // nerd font: file
        SuggestionType::Folder => '\u{ea83}',      // nerd font: folder
        SuggestionType::Special => '\u{eb5f}',     // nerd font: star
        SuggestionType::Mixin => '\u{eb62}',       // nerd font: puzzle
        SuggestionType::Shortcut => '\u{eb37}',    // nerd font: zap
    }
}

/// Fallback ASCII icons for terminals without Nerd Font support.
pub fn kind_icon_ascii(kind: SuggestionType) -> char {
    match kind {
        SuggestionType::Subcommand => 'C',
        SuggestionType::Option => 'F',
        SuggestionType::Arg => 'A',
        SuggestionType::File => 'f',
        SuggestionType::Folder => 'd',
        SuggestionType::Special => '*',
        SuggestionType::Mixin => 'M',
        SuggestionType::Shortcut => 'S',
    }
}
