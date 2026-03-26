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

    // Save cursor position
    write!(out, "\x1b[s").ok();

    // Draw top border
    write!(out, "\n\r").ok();
    write_colored(
        &mut out,
        theme.border_fg,
        theme.popup_bg,
        &"─".repeat(popup_width),
    );

    // Draw each row
    for (i, item) in visible_items.iter().enumerate() {
        let is_selected = i == selected;
        let bg = if is_selected {
            theme.selected_bg
        } else {
            theme.popup_bg
        };

        write!(out, "\n\r").ok();

        // Icon
        let icon = kind_icon_ascii(item.kind);
        let icon_color = if item.is_dangerous {
            theme.danger_fg
        } else {
            theme.kind_fg
        };
        write_colored(&mut out, icon_color, bg, &format!("{icon} "));

        // Name with match highlighting (clamp indices to truncated display length)
        let name_display = truncate(&item.display, popup_width.saturating_sub(4));
        let display_char_count = name_display.chars().count() as u32;
        let clamped_indices: Vec<u32> = item
            .match_indices
            .iter()
            .copied()
            .filter(|&i| i < display_char_count)
            .collect();
        write_name_highlighted(&mut out, &name_display, &clamped_indices, theme, bg);

        // Padding between name and description (use char count for column alignment)
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
            // Fill remaining with background
            write_colored(&mut out, theme.text_fg, bg, &" ".repeat(remaining));
        }
    }

    // Draw bottom border with item count
    write!(out, "\n\r").ok();
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

    // Restore cursor position
    write!(out, "\x1b[u").ok();

    Some(out)
}

/// Erase the popup area (used when dismissing).
pub fn erase_popup(num_lines: usize) -> String {
    let mut out = String::new();
    write!(out, "\x1b[s").ok(); // save cursor
    for _ in 0..num_lines + 2 {
        // +2 for borders
        write!(out, "\n\r\x1b[2K").ok(); // move down, clear line
    }
    write!(out, "\x1b[u").ok(); // restore cursor
    out
}

/// Render popup with in-place overwrite to avoid flicker.
/// If `prev_lines` > 0, clears any extra lines from the previous popup
/// that the new popup doesn't cover. All in one atomic write.
pub fn render_popup_inplace(
    items: &[CompletionItem],
    selected: usize,
    query: &str,
    theme: &Theme,
    terminal_cols: Option<u16>,
    prev_lines: usize,
) -> Option<String> {
    // Render the new popup content
    let content = render_popup(items, selected, query, theme, terminal_cols)?;

    let new_lines = items.len().min(MAX_VISIBLE_ITEMS);

    if prev_lines == 0 {
        // No previous popup: just show the new one
        return Some(content);
    }

    // Build atomic output: overwrite content + clear leftover lines
    let mut out = String::with_capacity(content.len() + 128);

    // The rendered popup already saves/restores cursor internally.
    // We need to strip the save/restore and handle it ourselves.
    // The popup format is: \x1b[s + content + \x1b[u
    // Strip the leading \x1b[s and trailing \x1b[u
    let inner = content.strip_prefix("\x1b[s").unwrap_or(&content);
    let inner = inner.strip_suffix("\x1b[u").unwrap_or(inner);

    out.push_str("\x1b[s"); // save cursor once

    // Write the popup content (overwrites previous lines in place)
    out.push_str(inner);

    // If previous popup had more lines, clear the extras
    if prev_lines > new_lines {
        let extra = prev_lines - new_lines;
        for _ in 0..extra {
            write!(out, "\n\r\x1b[2K").ok();
        }
    }

    out.push_str("\x1b[u"); // restore cursor once

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
