//! OSC (Operating System Command) sequence parser for Tabra.
//!
//! The shell integration script emits private OSC sequences that carry
//! command line state (buffer text, cursor position, prompt boundaries).
//! These sequences are embedded in the PTY output stream alongside normal
//! terminal output.
//!
//! This parser strips Tabra's OSC codes and returns them as structured events,
//! while forwarding all other bytes (including non-Tabra OSC codes) unchanged.
//!
//! Wire format: \x1b]6973;<CODE>;<PAYLOAD>\x07

use base64::Engine;

/// Private OSC prefix. "6973" is a made-up namespace unlikely to conflict.
const OSC_PREFIX: &[u8] = b"\x1b]6973;";
const OSC_TERMINATOR: u8 = 0x07; // BEL

/// Parsed OSC event from the shell integration script.
#[derive(Debug, Clone, PartialEq)]
pub enum OscEvent {
    /// The shell reported the current command line buffer and cursor position.
    CommandLine { buffer: String, cursor: usize },
    /// The shell prompt is about to be drawn.
    PromptStart,
    /// The shell prompt has finished drawing.
    PromptEnd,
}

/// Accumulator-based OSC parser that processes PTY output byte by byte.
///
/// Extracts Tabra OSC sequences and returns them as events, while passing
/// through all other bytes unchanged.
pub struct OscParser {
    /// Current state of the parser.
    state: State,
    /// Accumulator for bytes that might be part of an OSC sequence.
    buf: Vec<u8>,
    /// How many bytes of OSC_PREFIX we've matched so far.
    prefix_matched: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// Normal passthrough mode.
    Normal,
    /// We've matched some bytes of the OSC_PREFIX and are checking more.
    MatchingPrefix,
    /// We're inside a confirmed Tabra OSC sequence, accumulating payload.
    InPayload,
}

impl OscParser {
    pub fn new() -> Self {
        Self {
            state: State::Normal,
            buf: Vec::with_capacity(256),
            prefix_matched: 0,
        }
    }

    /// Feed a chunk of PTY output bytes through the parser.
    ///
    /// Returns:
    /// - `passthrough`: bytes to forward to the real terminal (non-Tabra content)
    /// - `events`: parsed Tabra OSC events
    pub fn feed(&mut self, chunk: &[u8]) -> (Vec<u8>, Vec<OscEvent>) {
        let mut passthrough = Vec::with_capacity(chunk.len());
        let mut events = Vec::new();

        for &byte in chunk {
            match self.state {
                State::Normal => {
                    if byte == OSC_PREFIX[0] {
                        // Potential start of OSC: \x1b
                        self.state = State::MatchingPrefix;
                        self.prefix_matched = 1;
                        self.buf.clear();
                        self.buf.push(byte);
                    } else {
                        passthrough.push(byte);
                    }
                }

                State::MatchingPrefix => {
                    self.buf.push(byte);
                    if byte == OSC_PREFIX[self.prefix_matched] {
                        self.prefix_matched += 1;
                        if self.prefix_matched == OSC_PREFIX.len() {
                            // Full prefix matched: we're in a Tabra OSC payload
                            self.state = State::InPayload;
                            self.buf.clear(); // discard prefix bytes
                        }
                    } else {
                        // Mismatch: not a Tabra OSC. Flush accumulated bytes
                        // (except the mismatched byte) as passthrough.
                        // If the mismatched byte is ESC (start of a new potential
                        // OSC), restart prefix matching instead of going to Normal.
                        let accumulated_before = self.buf.len() - 1;
                        passthrough.extend_from_slice(&self.buf[..accumulated_before]);
                        self.buf.clear();
                        if byte == OSC_PREFIX[0] {
                            self.state = State::MatchingPrefix;
                            self.prefix_matched = 1;
                            self.buf.push(byte);
                        } else {
                            passthrough.push(byte);
                            self.prefix_matched = 0;
                            self.state = State::Normal;
                        }
                    }
                }

                State::InPayload => {
                    if byte == OSC_TERMINATOR {
                        // End of OSC payload: parse it
                        if let Some(event) = parse_payload(&self.buf) {
                            events.push(event);
                        }
                        self.buf.clear();
                        self.state = State::Normal;
                    } else {
                        self.buf.push(byte);
                        // Safety limit: if payload exceeds 64KB, abort and flush
                        if self.buf.len() > 65536 {
                            self.buf.clear();
                            self.state = State::Normal;
                        }
                    }
                }
            }
        }

        (passthrough, events)
    }
}

/// Parse an OSC payload (the bytes between the prefix and terminator).
/// Format: CODE;ARG1;ARG2;...
fn parse_payload(payload: &[u8]) -> Option<OscEvent> {
    let s = std::str::from_utf8(payload).ok()?;
    let mut parts = s.splitn(3, ';');
    let code = parts.next()?;

    match code {
        "CL" => {
            // CommandLine: CL;base64(buffer);cursor
            let b64_buffer = parts.next()?;
            let cursor_str = parts.next()?;

            let buffer_bytes = base64::engine::general_purpose::STANDARD
                .decode(b64_buffer)
                .ok()?;
            let buffer = String::from_utf8(buffer_bytes).ok()?;
            let cursor = cursor_str.parse::<usize>().ok()?;

            Some(OscEvent::CommandLine { buffer, cursor })
        }
        "PS" => Some(OscEvent::PromptStart),
        "PE" => Some(OscEvent::PromptEnd),
        _ => None, // Unknown code: silently ignore
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_osc(code: &str, payload: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x1b]6973;");
        bytes.extend_from_slice(code.as_bytes());
        if !payload.is_empty() {
            bytes.push(b';');
            bytes.extend_from_slice(payload.as_bytes());
        }
        bytes.push(0x07);
        bytes
    }

    #[test]
    fn test_passthrough_normal_bytes() {
        let mut parser = OscParser::new();
        let (pass, events) = parser.feed(b"hello world");
        assert_eq!(pass, b"hello world");
        assert!(events.is_empty());
    }

    #[test]
    fn test_strip_tabra_osc() {
        let mut parser = OscParser::new();
        let b64 = base64::engine::general_purpose::STANDARD.encode("git ");
        let osc = make_osc("CL", &format!("{};4", b64));
        let mut input = b"before".to_vec();
        input.extend_from_slice(&osc);
        input.extend_from_slice(b"after");

        let (pass, events) = parser.feed(&input);
        assert_eq!(pass, b"beforeafter");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            OscEvent::CommandLine {
                buffer: "git ".to_string(),
                cursor: 4
            }
        );
    }

    #[test]
    fn test_prompt_events() {
        let mut parser = OscParser::new();
        let mut input = make_osc("PS", "");
        input.extend_from_slice(&make_osc("PE", ""));

        let (pass, events) = parser.feed(&input);
        assert!(pass.is_empty());
        assert_eq!(events, vec![OscEvent::PromptStart, OscEvent::PromptEnd]);
    }

    #[test]
    fn test_non_tabra_osc_passthrough() {
        let mut parser = OscParser::new();
        // Standard OSC (not 6973): \x1b]0;title\x07
        let input = b"\x1b]0;window title\x07rest";
        let (pass, events) = parser.feed(input);
        assert_eq!(pass, b"\x1b]0;window title\x07rest");
        assert!(events.is_empty());
    }

    #[test]
    fn test_split_across_chunks() {
        let mut parser = OscParser::new();
        let b64 = base64::engine::general_purpose::STANDARD.encode("ls");
        let osc = make_osc("CL", &format!("{};2", b64));

        // Split the OSC across two feed() calls
        let mid = osc.len() / 2;
        let (pass1, events1) = parser.feed(&osc[..mid]);
        let (pass2, events2) = parser.feed(&osc[mid..]);

        assert!(pass1.is_empty() || pass2.is_empty()); // OSC bytes should not pass through
        let all_events: Vec<_> = events1.into_iter().chain(events2).collect();
        assert_eq!(all_events.len(), 1);
        assert_eq!(
            all_events[0],
            OscEvent::CommandLine {
                buffer: "ls".to_string(),
                cursor: 2
            }
        );
    }
}
