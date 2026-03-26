//! Popup state machine for the session event loop.
//!
//! Manages popup visibility, item selection, and rendering.
//! Receives command line updates (from OSC) and key events (from stdin),
//! produces actions for the event loop to execute (write to terminal, PTY, etc).

use super::keys::KeyEvent;
use tabra::ipc::async_client;
use tabra::ipc::protocol::{CompletionItem, Response};
use tabra::render::{overlay, theme::Theme};

/// Action the event loop should take in response to a popup state change.
#[derive(Debug)]
pub enum PopupAction {
    /// Write this ANSI string to the real terminal (show/update popup).
    Show(String),
    /// Erase the popup (write erase sequence to terminal).
    Erase { lines: usize },
    /// Forward these raw bytes to the PTY master (key not consumed by popup).
    ForwardKey(Vec<u8>),
    /// Accept the selected suggestion: erase popup and insert text.
    Accept {
        /// Start position of the token to replace (byte offset in buffer).
        token_start: usize,
        /// Text to insert.
        insert_text: String,
    },
    /// No action needed.
    Nothing,
}

/// Popup state: tracks visibility, items, selection, and last known buffer.
pub struct PopupState {
    pub visible: bool,
    pub items: Vec<CompletionItem>,
    pub selected: usize,
    pub last_buffer: String,
    pub last_cursor: usize,
    pub terminal_cols: u16,
    pub theme: Theme,
    popup_lines: usize,
}

impl PopupState {
    pub fn new(terminal_cols: u16) -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            selected: 0,
            last_buffer: String::new(),
            last_cursor: 0,
            terminal_cols,
            theme: Theme::default(),
            popup_lines: 0,
        }
    }

    /// Handle a command line update (from OSC). Fetches completions from daemon.
    pub async fn on_command_line(
        &mut self,
        buffer: String,
        cursor: usize,
        cwd: &str,
    ) -> PopupAction {
        self.last_buffer = buffer.clone();
        self.last_cursor = cursor;

        // Fetch completions from daemon
        let response =
            match async_client::complete(&buffer, cursor, cwd, Some(self.terminal_cols)).await {
                Ok(r) => r,
                Err(_) => return self.hide(),
            };

        match response {
            Response::Completions {
                items,
                rendered_popup,
                ..
            } => {
                if items.is_empty() {
                    return self.hide();
                }

                self.items = items;
                self.selected = 0;
                self.visible = true;

                // Use daemon's pre-rendered popup if available
                if let Some(popup) = rendered_popup {
                    let lines = popup.matches('\n').count().max(1);
                    self.popup_lines = lines;
                    PopupAction::Show(popup)
                } else {
                    self.render_current()
                }
            }
            _ => self.hide(),
        }
    }

    /// Handle a key event. Returns the action the event loop should take.
    pub fn on_key(&mut self, key: &KeyEvent) -> PopupAction {
        if !self.visible {
            // Popup not visible: forward all keys to PTY
            return PopupAction::ForwardKey(key.raw_bytes());
        }

        match key {
            KeyEvent::Tab => {
                // Accept the selected suggestion
                if self.selected < self.items.len() {
                    let item = &self.items[self.selected];
                    let insert_text = item.insert.clone();
                    let token_start = self.find_token_start();
                    self.visible = false;
                    self.popup_lines = 0;
                    self.items.clear();
                    PopupAction::Accept {
                        token_start,
                        insert_text,
                    }
                } else {
                    self.hide()
                }
            }

            KeyEvent::Escape => self.hide(),

            KeyEvent::ArrowDown => {
                if !self.items.is_empty() {
                    self.selected = (self.selected + 1) % self.items.len().min(10);
                    self.render_current()
                } else {
                    PopupAction::Nothing
                }
            }

            KeyEvent::ArrowUp => {
                if !self.items.is_empty() {
                    let max = self.items.len().min(10);
                    self.selected = (self.selected + max - 1) % max;
                    self.render_current()
                } else {
                    PopupAction::Nothing
                }
            }

            KeyEvent::Enter => {
                // Enter with popup visible: hide popup and forward Enter to execute
                self.hide();
                PopupAction::ForwardKey(key.raw_bytes())
            }

            // Any other key with popup visible: forward to PTY
            // (the OSC update after the key will trigger a new completion fetch)
            _ => PopupAction::ForwardKey(key.raw_bytes()),
        }
    }

    /// Hide the popup and return an Erase action.
    fn hide(&mut self) -> PopupAction {
        if self.visible {
            self.visible = false;
            let lines = self.popup_lines;
            self.popup_lines = 0;
            self.items.clear();
            self.selected = 0;
            PopupAction::Erase { lines }
        } else {
            PopupAction::Nothing
        }
    }

    /// Render the popup with the current selection.
    fn render_current(&mut self) -> PopupAction {
        if let Some(rendered) = overlay::render_popup(
            &self.items,
            self.selected,
            "",
            &self.theme,
            Some(self.terminal_cols),
        ) {
            let lines = rendered.matches('\n').count().max(1);
            self.popup_lines = lines;
            PopupAction::Show(rendered)
        } else {
            self.hide()
        }
    }

    /// Find the start of the current token in the last known buffer.
    /// Uses a forward walk matching the Rust parser's tokenizer logic.
    fn find_token_start(&self) -> usize {
        let before = &self.last_buffer[..self.last_cursor.min(self.last_buffer.len())];
        let mut last_boundary = 0;
        let mut in_sq = false;
        let mut in_dq = false;
        let mut escape_next = false;

        for (i, ch) in before.char_indices() {
            if escape_next {
                escape_next = false;
                continue;
            }
            match ch {
                '\\' if !in_sq => escape_next = true,
                '\'' if !in_dq => in_sq = !in_sq,
                '"' if !in_sq => in_dq = !in_dq,
                ' ' | '\t' if !in_sq && !in_dq => {
                    last_boundary = i + ch.len_utf8();
                }
                _ => {}
            }
        }
        last_boundary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_popup(cols: u16) -> PopupState {
        PopupState::new(cols)
    }

    #[test]
    fn test_forward_key_when_not_visible() {
        let mut popup = make_popup(80);
        let action = popup.on_key(&KeyEvent::Char('a'));
        match action {
            PopupAction::ForwardKey(bytes) => assert_eq!(bytes, b"a"),
            _ => panic!("expected ForwardKey"),
        }
    }

    #[test]
    fn test_escape_hides_popup() {
        let mut popup = make_popup(80);
        popup.visible = true;
        popup.popup_lines = 5;
        popup.items = vec![CompletionItem {
            display: "test".into(),
            insert: "test".into(),
            description: String::new(),
            kind: tabra::spec::types::SuggestionType::Subcommand,
            match_indices: vec![],
            is_dangerous: false,
        }];

        let action = popup.on_key(&KeyEvent::Escape);
        match action {
            PopupAction::Erase { lines } => assert_eq!(lines, 5),
            _ => panic!("expected Erase"),
        }
        assert!(!popup.visible);
    }

    #[test]
    fn test_arrow_down_navigates() {
        let mut popup = make_popup(80);
        popup.visible = true;
        popup.items = (0..5)
            .map(|i| CompletionItem {
                display: format!("item{i}"),
                insert: format!("item{i}"),
                description: String::new(),
                kind: tabra::spec::types::SuggestionType::Subcommand,
                match_indices: vec![],
                is_dangerous: false,
            })
            .collect();

        popup.on_key(&KeyEvent::ArrowDown);
        assert_eq!(popup.selected, 1);
        popup.on_key(&KeyEvent::ArrowDown);
        assert_eq!(popup.selected, 2);
    }

    #[test]
    fn test_tab_accepts() {
        let mut popup = make_popup(80);
        popup.visible = true;
        popup.last_buffer = "git ".to_string();
        popup.last_cursor = 4;
        popup.items = vec![CompletionItem {
            display: "commit".into(),
            insert: "commit".into(),
            description: "Record changes".into(),
            kind: tabra::spec::types::SuggestionType::Subcommand,
            match_indices: vec![],
            is_dangerous: false,
        }];

        let action = popup.on_key(&KeyEvent::Tab);
        match action {
            PopupAction::Accept {
                token_start,
                insert_text,
            } => {
                assert_eq!(token_start, 4);
                assert_eq!(insert_text, "commit");
            }
            _ => panic!("expected Accept"),
        }
        assert!(!popup.visible);
    }

    #[test]
    fn test_find_token_start() {
        let mut popup = make_popup(80);

        popup.last_buffer = "git commit -m 'hello world'".to_string();
        popup.last_cursor = 27;
        assert_eq!(popup.find_token_start(), 14); // start of 'hello world'

        popup.last_buffer = "git checkout ".to_string();
        popup.last_cursor = 13;
        assert_eq!(popup.find_token_start(), 13); // after space, empty token

        popup.last_buffer = "git ".to_string();
        popup.last_cursor = 4;
        assert_eq!(popup.find_token_start(), 4);
    }

    #[test]
    fn test_enter_hides_and_forwards() {
        let mut popup = make_popup(80);
        popup.visible = true;
        popup.popup_lines = 3;
        popup.items = vec![CompletionItem {
            display: "test".into(),
            insert: "test".into(),
            description: String::new(),
            kind: tabra::spec::types::SuggestionType::Subcommand,
            match_indices: vec![],
            is_dangerous: false,
        }];

        let action = popup.on_key(&KeyEvent::Enter);
        match action {
            PopupAction::ForwardKey(bytes) => assert_eq!(bytes, vec![0x0d]),
            _ => panic!("expected ForwardKey for Enter"),
        }
        // Popup should be hidden after Enter
        assert!(!popup.visible);
    }
}
