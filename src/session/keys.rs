//! Raw byte to KeyEvent parser.
//!
//! Converts raw terminal input bytes into structured key events.
//! Handles ANSI escape sequences for arrow keys, function keys, etc.

/// A parsed key event from raw terminal input.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyEvent {
    /// A printable character.
    Char(char),
    /// Tab key (0x09 / Ctrl-I).
    Tab,
    /// Escape key (standalone, no following sequence within timeout).
    Escape,
    /// Enter / Return (0x0D / Ctrl-M).
    Enter,
    /// Backspace (0x7F or 0x08).
    Backspace,
    /// Arrow keys.
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// Ctrl+C (0x03).
    CtrlC,
    /// Ctrl+D (0x04).
    CtrlD,
    /// Ctrl+L (0x0C) - clear screen.
    CtrlL,
    /// Unrecognized byte sequence, forwarded as-is.
    Raw(Vec<u8>),
}

impl KeyEvent {
    /// Get the raw bytes to forward to the PTY for this key event.
    /// Returns None for events that should never be forwarded (handled internally).
    pub fn raw_bytes(&self) -> Vec<u8> {
        match self {
            KeyEvent::Char(c) => {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.as_bytes().to_vec()
            }
            KeyEvent::Tab => vec![0x09],
            KeyEvent::Escape => vec![0x1b],
            KeyEvent::Enter => vec![0x0d],
            KeyEvent::Backspace => vec![0x7f],
            KeyEvent::ArrowUp => vec![0x1b, b'[', b'A'],
            KeyEvent::ArrowDown => vec![0x1b, b'[', b'B'],
            KeyEvent::ArrowLeft => vec![0x1b, b'[', b'C'],
            KeyEvent::ArrowRight => vec![0x1b, b'[', b'D'],
            KeyEvent::CtrlC => vec![0x03],
            KeyEvent::CtrlD => vec![0x04],
            KeyEvent::CtrlL => vec![0x0c],
            KeyEvent::Raw(bytes) => bytes.clone(),
        }
    }
}

/// Parse a chunk of raw bytes from terminal stdin into key events.
///
/// `escape_pending` tracks whether the previous chunk ended with a lone ESC byte.
/// If true, the first byte of this chunk determines whether it's an escape sequence
/// or a standalone Escape keypress.
///
/// Returns the parsed events and whether an escape is pending for the next call.
pub fn parse_bytes(buf: &[u8], escape_pending: bool) -> (Vec<KeyEvent>, bool) {
    let mut events = Vec::new();
    let mut i = 0;
    let mut pending = escape_pending;

    // If we had a pending escape and got new bytes, check if it's a sequence
    if pending {
        if buf.is_empty() {
            // Timeout fired with no data: standalone Escape
            events.push(KeyEvent::Escape);
            return (events, false);
        }
        if buf[0] == b'[' {
            // CSI sequence: ESC [ ...
            i = 1; // skip the '['
            pending = false;
            if i < buf.len() {
                match buf[i] {
                    b'A' => {
                        events.push(KeyEvent::ArrowUp);
                        i += 1;
                    }
                    b'B' => {
                        events.push(KeyEvent::ArrowDown);
                        i += 1;
                    }
                    b'C' => {
                        events.push(KeyEvent::ArrowRight);
                        i += 1;
                    }
                    b'D' => {
                        events.push(KeyEvent::ArrowLeft);
                        i += 1;
                    }
                    _ => {
                        // Unknown CSI sequence: forward raw bytes
                        let mut raw = vec![0x1b, b'['];
                        // Consume until we hit a letter (the final byte of CSI)
                        while i < buf.len() {
                            raw.push(buf[i]);
                            if buf[i] >= 0x40 && buf[i] <= 0x7e {
                                i += 1;
                                break;
                            }
                            i += 1;
                        }
                        events.push(KeyEvent::Raw(raw));
                    }
                }
            } else {
                // ESC [ with nothing after: forward raw
                events.push(KeyEvent::Raw(vec![0x1b, b'[']));
            }
        } else {
            // ESC followed by non-[: emit standalone Escape then process byte
            events.push(KeyEvent::Escape);
            pending = false;
            // Don't increment i: the current byte still needs processing
        }
    }

    // Process remaining bytes
    while i < buf.len() {
        match buf[i] {
            0x1b => {
                // ESC: check if next byte is '[' for CSI
                if i + 1 < buf.len() && buf[i + 1] == b'[' {
                    // CSI sequence
                    i += 2;
                    if i < buf.len() {
                        match buf[i] {
                            b'A' => {
                                events.push(KeyEvent::ArrowUp);
                                i += 1;
                            }
                            b'B' => {
                                events.push(KeyEvent::ArrowDown);
                                i += 1;
                            }
                            b'C' => {
                                events.push(KeyEvent::ArrowRight);
                                i += 1;
                            }
                            b'D' => {
                                events.push(KeyEvent::ArrowLeft);
                                i += 1;
                            }
                            _ => {
                                let mut raw = vec![0x1b, b'['];
                                while i < buf.len() {
                                    raw.push(buf[i]);
                                    if buf[i] >= 0x40 && buf[i] <= 0x7e {
                                        i += 1;
                                        break;
                                    }
                                    i += 1;
                                }
                                events.push(KeyEvent::Raw(raw));
                            }
                        }
                    } else {
                        events.push(KeyEvent::Raw(vec![0x1b, b'[']));
                    }
                } else if i + 1 < buf.len() {
                    // ESC followed by non-[: Alt+key or other
                    events.push(KeyEvent::Raw(vec![0x1b, buf[i + 1]]));
                    i += 2;
                } else {
                    // Lone ESC at end of buffer: mark as pending
                    pending = true;
                    i += 1;
                }
            }
            0x03 => {
                events.push(KeyEvent::CtrlC);
                i += 1;
            }
            0x04 => {
                events.push(KeyEvent::CtrlD);
                i += 1;
            }
            0x09 => {
                events.push(KeyEvent::Tab);
                i += 1;
            }
            0x0c => {
                events.push(KeyEvent::CtrlL);
                i += 1;
            }
            0x0d => {
                events.push(KeyEvent::Enter);
                i += 1;
            }
            0x7f => {
                events.push(KeyEvent::Backspace);
                i += 1;
            }
            0x08 => {
                events.push(KeyEvent::Backspace);
                i += 1;
            }
            b if b >= 0x20 && b < 0x7f => {
                events.push(KeyEvent::Char(b as char));
                i += 1;
            }
            b if b >= 0x80 => {
                // UTF-8 multi-byte: try to decode
                let remaining = &buf[i..];
                if let Some((ch, len)) = decode_utf8_char(remaining) {
                    events.push(KeyEvent::Char(ch));
                    i += len;
                } else {
                    events.push(KeyEvent::Raw(vec![b]));
                    i += 1;
                }
            }
            b => {
                // Control character (Ctrl+A through Ctrl+Z etc.)
                events.push(KeyEvent::Raw(vec![b]));
                i += 1;
            }
        }
    }

    (events, pending)
}

/// Try to decode a single UTF-8 character from the start of a byte slice.
/// Returns the character and its byte length, or None if invalid.
fn decode_utf8_char(bytes: &[u8]) -> Option<(char, usize)> {
    let s = std::str::from_utf8(bytes).ok().or_else(|| {
        // Try progressively shorter slices (up to 4 bytes for UTF-8)
        for len in (1..=4.min(bytes.len())).rev() {
            if let Ok(s) = std::str::from_utf8(&bytes[..len]) {
                return Some(s);
            }
        }
        None
    })?;
    let ch = s.chars().next()?;
    Some((ch, ch.len_utf8()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_chars() {
        let (events, pending) = parse_bytes(b"abc", false);
        assert!(!pending);
        assert_eq!(
            events,
            vec![
                KeyEvent::Char('a'),
                KeyEvent::Char('b'),
                KeyEvent::Char('c'),
            ]
        );
    }

    #[test]
    fn test_parse_arrow_keys() {
        let (events, pending) = parse_bytes(b"\x1b[A\x1b[B", false);
        assert!(!pending);
        assert_eq!(events, vec![KeyEvent::ArrowUp, KeyEvent::ArrowDown]);
    }

    #[test]
    fn test_parse_tab_enter_backspace() {
        let (events, pending) = parse_bytes(b"\x09\x0d\x7f", false);
        assert!(!pending);
        assert_eq!(
            events,
            vec![KeyEvent::Tab, KeyEvent::Enter, KeyEvent::Backspace]
        );
    }

    #[test]
    fn test_parse_lone_escape_pending() {
        let (events, pending) = parse_bytes(b"\x1b", false);
        assert!(pending);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_pending_escape_timeout() {
        let (events, pending) = parse_bytes(b"", true);
        assert!(!pending);
        assert_eq!(events, vec![KeyEvent::Escape]);
    }

    #[test]
    fn test_parse_pending_escape_then_bracket() {
        let (events, pending) = parse_bytes(b"[A", true);
        assert!(!pending);
        assert_eq!(events, vec![KeyEvent::ArrowUp]);
    }

    #[test]
    fn test_parse_ctrl_c_d() {
        let (events, _) = parse_bytes(b"\x03\x04", false);
        assert_eq!(events, vec![KeyEvent::CtrlC, KeyEvent::CtrlD]);
    }

    #[test]
    fn test_parse_mixed() {
        let (events, _) = parse_bytes(b"g\x1b[Bi\x09", false);
        assert_eq!(
            events,
            vec![
                KeyEvent::Char('g'),
                KeyEvent::ArrowDown,
                KeyEvent::Char('i'),
                KeyEvent::Tab,
            ]
        );
    }
}
