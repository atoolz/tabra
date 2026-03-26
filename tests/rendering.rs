//! End-to-end rendering tests.
//!
//! Verifies the popup ANSI output is correct without a visual terminal.
//! Tests the actual escape sequences produced by render_popup and erase_popup.

use tabra::ipc::protocol::CompletionItem;
use tabra::render::{overlay, theme::Theme};
use tabra::spec::types::SuggestionType;

fn make_items(names: &[&str]) -> Vec<CompletionItem> {
    names
        .iter()
        .map(|n| CompletionItem {
            display: n.to_string(),
            insert: n.to_string(),
            description: format!("Description for {n}"),
            kind: SuggestionType::Subcommand,
            match_indices: vec![],
            is_dangerous: false,
        })
        .collect()
}

#[test]
fn test_render_popup_uses_relative_cursor_movement() {
    let items = make_items(&["add", "commit", "push"]);
    let theme = Theme::default();

    let rendered = overlay::render_popup(&items, 0, "", &theme, Some(80)).unwrap();

    // Must NOT contain save/restore cursor (breaks with scroll)
    assert!(
        !rendered.contains("\x1b[s"),
        "render_popup should NOT use save cursor (\\x1b[s)"
    );
    assert!(
        !rendered.contains("\x1b[u"),
        "render_popup should NOT use restore cursor (\\x1b[u)"
    );

    // Must contain cursor-up to return to prompt (3 items + 2 borders = 5 lines)
    assert!(
        rendered.contains("\x1b[5A\r"),
        "render_popup should end with \\x1b[5A\\r (cursor up 5 lines), got: {:?}",
        &rendered[rendered.len().saturating_sub(30)..]
    );

    // Must contain hide/show cursor
    assert!(
        rendered.starts_with("\x1b[?25l"),
        "should start with hide cursor"
    );
    assert!(
        rendered.ends_with("\x1b[?25h"),
        "should end with show cursor"
    );
}

#[test]
fn test_render_popup_uses_newline_cr() {
    let items = make_items(&["add", "commit"]);
    let theme = Theme::default();

    let rendered = overlay::render_popup(&items, 0, "", &theme, Some(80)).unwrap();

    // Must use \n\r for line breaks (not \x1b[E which is unreliable)
    let newline_cr_count = rendered.matches("\n\r").count();
    // 2 items + 1 top border + 1 bottom border = 4 \n\r sequences
    assert_eq!(
        newline_cr_count, 4,
        "expected 4 \\n\\r sequences (2 items + 2 borders), got {newline_cr_count}"
    );

    // Must NOT contain CSI E
    assert!(
        !rendered.contains("\x1b[E"),
        "should not use CSI E (\\x1b[E)"
    );
}

#[test]
fn test_render_popup_contains_item_names() {
    let items = make_items(&["checkout", "cherry-pick", "clean"]);
    let theme = Theme::default();

    let rendered = overlay::render_popup(&items, 0, "", &theme, Some(80)).unwrap();

    assert!(rendered.contains("checkout"), "should contain 'checkout'");
    assert!(
        rendered.contains("cherry"),
        "should contain 'cherry' (may be truncated to 'cherry-pi…')"
    );
    assert!(rendered.contains("clean"), "should contain 'clean'");
}

#[test]
fn test_render_popup_contains_descriptions() {
    let items = make_items(&["add"]);
    let theme = Theme::default();

    let rendered = overlay::render_popup(&items, 0, "", &theme, Some(80)).unwrap();

    assert!(
        rendered.contains("Description for add"),
        "should contain description"
    );
}

#[test]
fn test_render_popup_contains_border() {
    let items = make_items(&["add"]);
    let theme = Theme::default();

    let rendered = overlay::render_popup(&items, 0, "", &theme, Some(80)).unwrap();

    assert!(rendered.contains("─"), "should contain border character");
}

#[test]
fn test_render_popup_item_count_indicator() {
    // More than MAX_VISIBLE_ITEMS (10) should show count
    let names: Vec<&str> = (0..15)
        .map(|i| match i {
            0 => "add",
            1 => "apply",
            2 => "archive",
            3 => "bisect",
            4 => "blame",
            5 => "branch",
            6 => "checkout",
            7 => "cherry-pick",
            8 => "clean",
            9 => "clone",
            10 => "commit",
            11 => "config",
            12 => "diff",
            13 => "fetch",
            _ => "grep",
        })
        .collect();
    let items = make_items(&names);
    let theme = Theme::default();

    let rendered = overlay::render_popup(&items, 0, "", &theme, Some(80)).unwrap();

    assert!(
        rendered.contains("10/15"),
        "should contain item count '10/15'"
    );
}

#[test]
fn test_render_popup_empty_returns_none() {
    let items: Vec<CompletionItem> = vec![];
    let theme = Theme::default();

    let result = overlay::render_popup(&items, 0, "", &theme, Some(80));
    assert!(result.is_none(), "empty items should return None");
}

#[test]
fn test_erase_popup_uses_relative_movement() {
    let erase = overlay::erase_popup(5);

    // Must NOT contain save/restore
    assert!(
        !erase.contains("\x1b[s"),
        "erase should NOT use save cursor"
    );
    assert!(
        !erase.contains("\x1b[u"),
        "erase should NOT use restore cursor"
    );

    // Must contain cursor-up (5 items + 2 borders = 7 lines)
    assert!(
        erase.contains("\x1b[7A\r"),
        "erase should contain \\x1b[7A\\r (cursor up 7)"
    );

    // Must contain \n\r\x1b[2K for each line
    let clear_count = erase.matches("\x1b[2K").count();
    assert_eq!(clear_count, 7, "should clear 7 lines (5 items + 2 borders)");
}

#[test]
fn test_render_popup_cursor_up_count_varies_with_items() {
    let theme = Theme::default();

    // 1 item: 1 + 2 borders = 3 lines
    let items1 = make_items(&["add"]);
    let r1 = overlay::render_popup(&items1, 0, "", &theme, Some(80)).unwrap();
    assert!(r1.contains("\x1b[3A\r"), "1 item: should cursor up 3");

    // 5 items: 5 + 2 borders = 7 lines
    let items5 = make_items(&["add", "commit", "push", "pull", "fetch"]);
    let r5 = overlay::render_popup(&items5, 0, "", &theme, Some(80)).unwrap();
    assert!(r5.contains("\x1b[7A\r"), "5 items: should cursor up 7");

    // 10 items: 10 + 2 borders = 12 lines
    let items10 = make_items(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    let r10 = overlay::render_popup(&items10, 0, "", &theme, Some(80)).unwrap();
    assert!(r10.contains("\x1b[12A\r"), "10 items: should cursor up 12");
}

#[test]
fn test_render_popup_inplace_clears_extra_lines() {
    let items = make_items(&["add"]); // 1 item
    let theme = Theme::default();

    // Previous popup had 5 items, new has 1
    let rendered = overlay::render_popup_inplace(&items, 0, "", &theme, Some(80), 5).unwrap();

    // Should clear 4 extra lines (prev 5+2=7, new 1+2=3, diff=4)
    let clear_count = rendered.matches("\x1b[2K").count();
    // 3 lines for new popup (border + item + border) + 4 extra clears = 7
    assert!(
        clear_count >= 7,
        "should clear at least 7 lines (3 new + 4 extra), got {clear_count}"
    );

    // Should cursor up the full distance (7 lines)
    assert!(
        rendered.contains("\x1b[7A\r"),
        "should cursor up 7 (3 new content + 4 cleared)"
    );
}

#[test]
fn test_render_popup_inplace_no_prev_equals_normal() {
    let items = make_items(&["add", "commit"]);
    let theme = Theme::default();

    let normal = overlay::render_popup(&items, 0, "", &theme, Some(80)).unwrap();
    let inplace = overlay::render_popup_inplace(&items, 0, "", &theme, Some(80), 0).unwrap();

    assert_eq!(
        normal, inplace,
        "inplace with prev_lines=0 should equal normal"
    );
}

#[test]
fn test_render_popup_width_constrained_by_terminal() {
    let items = make_items(&["a-very-long-command-name-that-should-be-truncated"]);
    let theme = Theme::default();

    // Terminal width 40 cols, popup max width = 40-2 = 38
    let rendered = overlay::render_popup(&items, 0, "", &theme, Some(40)).unwrap();

    // Each line should not exceed ~40 chars of visible content
    // (hard to check exactly because of ANSI codes, but the border should be ≤38 chars)
    assert!(
        rendered.contains("…"),
        "should truncate with ellipsis for narrow terminal"
    );
}
