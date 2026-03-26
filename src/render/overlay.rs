//! ANSI overlay renderer.
//!
//! Renders the autocomplete popup as an ANSI escape sequence string that the
//! shell hook can print below the cursor. The popup is drawn using cursor
//! movement, colors, and clearing, so it overlays the terminal without
//! disturbing the shell's own output.
//!
//! The popup is positioned relative to the current cursor position:
//! - It appears on the line below the prompt
//! - Its width adapts to content (with a max and min)
//! - It shows N items with the selected one highlighted

use crate::ipc::protocol::CompletionItem;
use crate::render::theme::{kind_icon_ascii, Theme};
use crossterm::style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor};
use std::fmt::Write;

/// Maximum number of visible items in the popup.
pub const MAX_VISIBLE_ITEMS: usize = 10;
/// Minimum popup width in columns.
pub const MIN_POPUP_WIDTH: usize = 30;
/// Maximum popup width in columns.
pub const MAX_POPUP_WIDTH: usize = 70;

/// Render the completion popup as an ANSI string.
///
/// The string contains escape sequences to:
/// 1. Save cursor position
/// 2. Move down one line
/// 3. Draw each row with colors
/// 4. Restore cursor position
///
/// Returns None if items is empty.
pub fn render_popup(
    items: &[CompletionItem],
    selected: usize,
    _query: &str,
    theme: &Theme,
    terminal_cols: Option<u16>,
) -> Option<String> {
    if items.is_empty() {
        return None;
    }

    // Use terminal width to constrain popup, or fall back to MAX_POPUP_WIDTH
    let max_width = terminal_cols
        .map(|c| (c as usize).saturating_sub(2).min(MAX_POPUP_WIDTH))
        .unwrap_or(MAX_POPUP_WIDTH);

    let visible_count = items.len().min(MAX_VISIBLE_ITEMS);
    // total popup lines = visible_count + 2 borders (used by inplace/erase externally)
    let visible_items = &items[..visible_count];

    // Calculate popup width based on content
    let content_width = visible_items
        .iter()
        .map(|item| {
            let icon_w = 2; // icon + space
            let name_w = item.display.chars().count();
            let desc_w = if item.description.is_empty() {
                0
            } else {
                item.description.chars().count().min(30) + 2
            };
            icon_w + name_w + desc_w
        })
        .max()
        .unwrap_or(MIN_POPUP_WIDTH);

    let popup_width = content_width.clamp(MIN_POPUP_WIDTH, max_width);

    let mut out = String::with_capacity(1024);
    // visible_count + 2 borders = total popup lines (used by erase_popup externally)

    // Hide cursor + DEC save cursor position (more reliable than SCO \x1b[s with scroll)
    write!(out, "\x1b[?25l\x1b7").ok();

    // Move to next line and draw top border
    write!(out, "\n\r\x1b[2K").ok();
    write_colored(
        &mut out,
        theme.border_fg,
        theme.popup_bg,
        &"─".repeat(popup_width),
    );

    // Draw each item row
    for (i, item) in visible_items.iter().enumerate() {
        let is_selected = i == selected;
        let bg = if is_selected {
            theme.selected_bg
        } else {
            theme.popup_bg
        };

        write!(out, "\n\r\x1b[2K").ok();

        // Icon
        let icon = kind_icon_ascii(item.kind);
        let icon_color = if item.is_dangerous {
            theme.danger_fg
        } else {
            theme.kind_fg
        };
        write_colored(&mut out, icon_color, bg, &format!("{icon} "));

        // Name with match highlighting
        let name_display = truncate(&item.display, popup_width.saturating_sub(4));
        let display_char_count = name_display.chars().count() as u32;
        let clamped_indices: Vec<u32> = item
            .match_indices
            .iter()
            .copied()
            .filter(|&i| i < display_char_count)
            .collect();
        write_name_highlighted(&mut out, &name_display, &clamped_indices, theme, bg);

        // Description
        let used = 2 + name_display.chars().count();
        let remaining = popup_width.saturating_sub(used);

        if !item.description.is_empty() && remaining > 5 {
            let desc = truncate(&item.description, remaining.saturating_sub(3));
            let pad = remaining
                .saturating_sub(desc.chars().count())
                .saturating_sub(3);
            write_colored(
                &mut out,
                theme.desc_fg,
                bg,
                &format!("{:>pad$} {desc} ", ""),
            );
        } else {
            write_colored(&mut out, theme.text_fg, bg, &" ".repeat(remaining));
        }
    }

    // Draw bottom border with item count
    write!(out, "\n\r\x1b[2K").ok();
    let count_str = if items.len() > MAX_VISIBLE_ITEMS {
        format!(" {}/{} ", visible_count, items.len())
    } else {
        String::new()
    };
    let border_len = popup_width.saturating_sub(count_str.len());
    write_colored(
        &mut out,
        theme.border_fg,
        theme.popup_bg,
        &format!("{}{count_str}", "─".repeat(border_len)),
    );

    // Reset colors
    write!(out, "\x1b[0m").ok();

    // DEC restore cursor position + show cursor.
    // \x1b8 restores the position saved by \x1b7. DEC save/restore is more
    // reliable than SCO (\x1b[s/\x1b[u) because it handles scroll offset
    // correctly on modern terminals (ghostty, alacritty, kitty, wezterm).
    write!(out, "\x1b8\x1b[?25h").ok();

    Some(out)
}

/// Erase the popup area (used when dismissing).
pub fn erase_popup(num_lines: usize) -> String {
    let total = num_lines + 2; // items + borders
    let mut out = String::new();
    write!(out, "\x1b[?25l\x1b7").ok(); // hide cursor + DEC save
    for _ in 0..total {
        write!(out, "\n\r\x1b[2K").ok();
    }
    write!(out, "\x1b8\x1b[?25h").ok(); // DEC restore + show cursor
    out
}

/// Render popup, erasing previous popup first if it existed.
/// Produces a single atomic string: erase old + render new.
pub fn render_popup_inplace(
    items: &[CompletionItem],
    selected: usize,
    query: &str,
    theme: &Theme,
    terminal_cols: Option<u16>,
    prev_lines: usize,
) -> Option<String> {
    if prev_lines == 0 {
        return render_popup(items, selected, query, theme, terminal_cols);
    }

    let prev_total = prev_lines + 2; // previous items + borders

    let mut out = String::with_capacity(2048);

    // 1. Hide cursor + DEC save (at prompt position)
    write!(out, "\x1b[?25l\x1b7").ok();

    // 2. Erase old popup: move down through each line, clear it
    for _ in 0..prev_total {
        write!(out, "\n\r\x1b[2K").ok();
    }

    // 3. DEC restore back to prompt position
    write!(out, "\x1b8").ok();

    // 4. Render new popup content. render_popup starts with \x1b[?25l\x1b7
    //    which we already emitted, so strip that prefix.
    let new_content = render_popup(items, selected, query, theme, terminal_cols)?;
    let inner = new_content
        .strip_prefix("\x1b[?25l\x1b7")
        .unwrap_or(&new_content);
    out.push_str(inner);

    Some(out)
}

fn write_colored(out: &mut String, fg: Color, bg: Color, text: &str) {
    write!(
        out,
        "{}{}{}{}{}",
        SetForegroundColor(fg),
        SetBackgroundColor(bg),
        text,
        SetForegroundColor(Color::Reset),
        SetBackgroundColor(Color::Reset),
    )
    .ok();
}

fn write_name_highlighted(
    out: &mut String,
    name: &str,
    match_indices: &[u32],
    theme: &Theme,
    bg: Color,
) {
    for (i, ch) in name.chars().enumerate() {
        let is_match = match_indices.contains(&(i as u32));
        let fg = if is_match {
            theme.match_fg
        } else {
            theme.text_fg
        };
        if is_match {
            write!(
                out,
                "{}{}{}{}{}{}",
                SetForegroundColor(fg),
                SetBackgroundColor(bg),
                SetAttribute(Attribute::Bold),
                ch,
                SetAttribute(Attribute::Reset),
                SetBackgroundColor(Color::Reset),
            )
            .ok();
        } else {
            write!(
                out,
                "{}{}{}",
                SetForegroundColor(fg),
                SetBackgroundColor(bg),
                ch,
            )
            .ok();
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len > 1 {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{truncated}…")
    } else {
        String::new()
    }
}
