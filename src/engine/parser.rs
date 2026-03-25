//! Command-line parser: tokenizes the user's input buffer and walks the
//! spec tree to determine the current completion context.
//!
//! Given "git commit -m " and a cursor position, the parser determines:
//! - Which subcommand path we're in: ["git", "commit"]
//! - Which options have been consumed: ["-m"]
//! - What kind of completion is expected next: arg to -m? another option?
//!
//! This is NOT a shell parser. It does not handle pipes, redirections, or
//! variable expansion. The shell hook pre-processes those.

use crate::spec::types::{Arg, FilterStrategy, SingleOrArray, Spec};

/// The result of parsing the command line against a spec.
#[derive(Debug, Clone)]
pub struct ParseContext {
    /// The chain of subcommands from root to current position.
    /// e.g. for "git remote add", this would be ["git", "remote", "add"].
    pub subcommand_path: Vec<String>,

    /// Reference path into the spec tree (indices into subcommands arrays).
    /// Used to look up the current Subcommand node.
    pub spec_path: Vec<usize>,

    /// Options that have already been used in this subcommand scope.
    pub used_options: Vec<String>,

    /// What the parser expects at the cursor position.
    pub expected: ExpectedCompletion,

    /// The partial token being typed (for filtering/matching).
    pub current_token: String,

    /// Filter strategy in effect at this position.
    pub filter_strategy: FilterStrategy,
}

/// What kind of completion the parser expects at the cursor position.
#[derive(Debug, Clone)]
pub enum ExpectedCompletion {
    /// Expecting a subcommand, option, or argument of the current subcommand.
    SubcommandOrOptionOrArg {
        /// Index of the next positional arg expected (if any).
        arg_index: usize,
    },

    /// Expecting the argument value for a specific option (e.g. after "-m").
    OptionArg {
        /// The option name that needs an argument.
        option_name: String,
        /// Which arg of the option (most options have 1, some have 2+).
        arg_index: usize,
    },

    /// No more completions expected (e.g. after "--" separator).
    None,
}

/// Tokenize a command line buffer at the given cursor position.
/// `cursor` is a character index (as provided by ZLE's $CURSOR), not a byte offset.
/// Returns (tokens_before_cursor, partial_token_at_cursor).
pub fn tokenize(buffer: &str, cursor: usize) -> (Vec<String>, String) {
    // Convert character index to byte offset safely (ZLE sends char index, not byte offset)
    let byte_cursor = buffer
        .char_indices()
        .nth(cursor)
        .map(|(i, _)| i)
        .unwrap_or(buffer.len());
    let relevant = &buffer[..byte_cursor];

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for ch in relevant.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quote => {
                escape_next = true;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    // If the buffer ends with a space, the partial token is empty
    // (user is starting a new token). Otherwise, it's the last token.
    if relevant.ends_with(' ') || relevant.ends_with('\t') {
        if !current.is_empty() {
            tokens.push(current);
        }
        (tokens, String::new())
    } else {
        (tokens, current)
    }
}

/// Parse the command line against a spec to determine completion context.
pub fn parse(spec: &Spec, buffer: &str, cursor: usize) -> ParseContext {
    let (tokens, partial) = tokenize(buffer, cursor);

    let mut subcommand_path = Vec::new();
    let mut spec_path = Vec::new();
    let mut used_options: Vec<String> = Vec::new();
    let mut current_cmd = spec;
    let mut arg_index: usize = 0;
    let mut filter_strategy = FilterStrategy::Default;
    let mut hit_separator = false;

    // Skip the first token (the command name itself, e.g. "git")
    let command_tokens = if tokens.is_empty() {
        &[][..]
    } else {
        &tokens[1..]
    };
    if !tokens.is_empty() {
        subcommand_path.push(tokens[0].clone());
    }

    let mut i = 0;
    let mut pending_option_arg: Option<(String, usize)> = None;

    while i < command_tokens.len() {
        let token = &command_tokens[i];

        // If we're expecting an argument for an option, consume this token as the arg value
        if pending_option_arg.take().is_some() {
            i += 1;
            continue;
        }

        // Check for "--" separator
        if token == "--" {
            hit_separator = true;
            i += 1;
            continue;
        }

        // Try to match a subcommand
        if let Some(subcmds) = &current_cmd.subcommands {
            if let Some((idx, subcmd)) = subcmds
                .iter()
                .enumerate()
                .find(|(_, sc)| sc.names().iter().any(|n| n == token))
            {
                subcommand_path.push(token.clone());
                spec_path.push(idx);
                current_cmd = subcmd;
                used_options.clear();
                arg_index = 0;
                if let Some(fs) = &current_cmd.filter_strategy {
                    filter_strategy = *fs;
                }
                i += 1;
                continue;
            }
        }

        // Try to match an option
        if token.starts_with('-') {
            if let Some(opts) = &current_cmd.options {
                if let Some(opt) = opts.iter().find(|o| o.names().iter().any(|n| n == token)) {
                    used_options.push(token.clone());

                    // Check if this option takes an argument
                    if let Some(ref args) = opt.args {
                        let arg_list: Vec<&Arg> = match args {
                            SingleOrArray::Single(a) => vec![a],
                            SingleOrArray::Array(a) => a.iter().collect(),
                        };
                        if !arg_list.is_empty() && !arg_list[0].is_optional {
                            pending_option_arg = Some((token.clone(), 0));
                        }
                    }
                    i += 1;
                    continue;
                }
            }
        }

        // Otherwise it's a positional argument
        arg_index += 1;
        i += 1;
    }

    // Determine final expected completion
    let expected = if hit_separator {
        ExpectedCompletion::None
    } else if let Some((opt_name, aidx)) = pending_option_arg {
        ExpectedCompletion::OptionArg {
            option_name: opt_name,
            arg_index: aidx,
        }
    } else {
        ExpectedCompletion::SubcommandOrOptionOrArg { arg_index }
    };

    ParseContext {
        subcommand_path,
        spec_path,
        used_options,
        expected,
        current_token: partial,
        filter_strategy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let (tokens, partial) = tokenize("git commit -m ", 14);
        assert_eq!(tokens, vec!["git", "commit", "-m"]);
        assert_eq!(partial, "");
    }

    #[test]
    fn test_tokenize_partial() {
        let (tokens, partial) = tokenize("git comm", 8);
        assert_eq!(tokens, vec!["git"]);
        assert_eq!(partial, "comm");
    }

    #[test]
    fn test_tokenize_quoted() {
        let (tokens, partial) = tokenize("git commit -m 'hello world' ", 28);
        assert_eq!(tokens, vec!["git", "commit", "-m", "hello world"]);
        assert_eq!(partial, "");
    }
}
