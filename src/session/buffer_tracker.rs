//! Local buffer tracker for the PTY session.
//!
//! Tracks the command line buffer by observing keystrokes forwarded to the PTY.
//! This eliminates the need for any shell integration script to report the buffer,
//! avoiding bind-x handlers and their associated readline redraw flash.
//!
//! Limitations: does not handle readline's complex editing (Ctrl-A, Ctrl-E, Ctrl-W,
//! Alt-B/F word movement, kill ring, etc.). These operations will desync the tracker,
//! but typing new characters quickly re-syncs it.

use super::keys::KeyEvent;

/// Tracks the command line buffer locally.
pub struct BufferTracker {
    /// Current buffer content.
    pub buffer: String,
    /// Cursor position (character index).
    pub cursor: usize,
    /// Whether we're in a command (after prompt, before Enter).
    pub active: bool,
}

impl BufferTracker {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            active: true,
        }
    }

    /// Process a key event that was forwarded to the PTY.
    /// Returns true if the buffer changed (popup should update).
    pub fn on_key(&mut self, event: &KeyEvent) -> bool {
        match event {
            KeyEvent::Char(c) => {
                // Insert character at cursor position
                let byte_pos = self.char_to_byte(self.cursor);
                self.buffer.insert(byte_pos, *c);
                self.cursor += 1;
                true
            }
            KeyEvent::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let byte_pos = self.char_to_byte(self.cursor);
                    // Find the byte range of the char at this position
                    let ch = self.buffer[byte_pos..].chars().next();
                    if let Some(ch) = ch {
                        self.buffer.drain(byte_pos..byte_pos + ch.len_utf8());
                    }
                    true
                } else {
                    false
                }
            }
            KeyEvent::Enter => {
                // Command submitted: clear buffer
                self.buffer.clear();
                self.cursor = 0;
                true
            }
            KeyEvent::CtrlC => {
                // Cancel: clear buffer
                self.buffer.clear();
                self.cursor = 0;
                true
            }
            KeyEvent::CtrlL => {
                // Clear screen: buffer stays the same
                false
            }
            KeyEvent::ArrowLeft => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                false // cursor moved but buffer content didn't change
            }
            KeyEvent::ArrowRight => {
                let char_len = self.buffer.chars().count();
                if self.cursor < char_len {
                    self.cursor += 1;
                }
                false
            }
            _ => false,
        }
    }

    /// Sync buffer from an OSC CommandLine event (if available).
    /// This corrects any desync from untracked editing operations.
    pub fn sync(&mut self, buffer: String, cursor: usize) {
        self.buffer = buffer;
        self.cursor = cursor;
    }

    /// Convert character index to byte position in the buffer.
    fn char_to_byte(&self, char_idx: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_idx)
            .map(|(byte_pos, _)| byte_pos)
            .unwrap_or(self.buffer.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_insert() {
        let mut t = BufferTracker::new();
        t.on_key(&KeyEvent::Char('g'));
        t.on_key(&KeyEvent::Char('i'));
        t.on_key(&KeyEvent::Char('t'));
        assert_eq!(t.buffer, "git");
        assert_eq!(t.cursor, 3);
    }

    #[test]
    fn test_backspace() {
        let mut t = BufferTracker::new();
        t.on_key(&KeyEvent::Char('g'));
        t.on_key(&KeyEvent::Char('i'));
        t.on_key(&KeyEvent::Backspace);
        assert_eq!(t.buffer, "g");
        assert_eq!(t.cursor, 1);
    }

    #[test]
    fn test_enter_clears() {
        let mut t = BufferTracker::new();
        t.on_key(&KeyEvent::Char('l'));
        t.on_key(&KeyEvent::Char('s'));
        t.on_key(&KeyEvent::Enter);
        assert_eq!(t.buffer, "");
        assert_eq!(t.cursor, 0);
    }

    #[test]
    fn test_space_in_buffer() {
        let mut t = BufferTracker::new();
        for c in "git ".chars() {
            t.on_key(&KeyEvent::Char(c));
        }
        assert_eq!(t.buffer, "git ");
        assert_eq!(t.cursor, 4);
    }
}
