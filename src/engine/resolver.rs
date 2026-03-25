//! Suggestion resolver: given a ParseContext, collects all candidate
//! suggestions from the spec tree.
//!
//! The resolver handles:
//! - Collecting subcommands, options, and arg suggestions for the current position
//! - Expanding templates (filepaths, folders) into real filesystem suggestions
//! - Collecting persistent options from parent subcommands
//! - Deduplicating already-used options

use crate::engine::parser::{ExpectedCompletion, ParseContext};
use crate::spec::types::{
    Arg, Opt, SingleOrArray, Spec, Subcommand, Suggestion, SuggestionType, TemplateString,
};
use std::path::Path;

/// A resolved suggestion ready for matching and display.
#[derive(Debug, Clone)]
pub struct ResolvedSuggestion {
    /// Text used for fuzzy/prefix matching.
    pub match_text: String,
    /// Text displayed in the popup.
    pub display_text: String,
    /// Text inserted into the terminal on accept.
    pub insert_text: String,
    /// Description/help text.
    pub description: String,
    /// Type of suggestion (for icon selection).
    pub kind: SuggestionType,
    /// Priority for ranking (0..100).
    pub priority: u8,
    /// Whether this is a dangerous action.
    pub is_dangerous: bool,
}

/// Resolve all candidate suggestions for a given parse context.
/// `cwd` is the shell's current working directory.
/// The `ctx.current_token` may contain a partial path (e.g. "./src/eng") which
/// is used to resolve the directory for filesystem template expansion.
pub fn resolve(spec: &Spec, ctx: &ParseContext, cwd: &str) -> Vec<ResolvedSuggestion> {
    // Walk the spec tree to find the current subcommand node.
    let current_cmd = walk_spec(spec, &ctx.spec_path);
    let mut suggestions = Vec::new();

    match &ctx.expected {
        ExpectedCompletion::SubcommandOrOptionOrArg { arg_index } => {
            // 1. Subcommands
            if let Some(subcmds) = &current_cmd.subcommands {
                for sc in subcmds {
                    if sc.hidden {
                        continue;
                    }
                    let names = sc.names();
                    let primary = match names.first() {
                        Some(n) => *n,
                        None => continue,
                    };
                    suggestions.push(ResolvedSuggestion {
                        match_text: primary.to_string(),
                        display_text: sc
                            .display_name
                            .clone()
                            .unwrap_or_else(|| primary.to_string()),
                        insert_text: sc
                            .insert_value
                            .clone()
                            .unwrap_or_else(|| primary.to_string()),
                        description: sc.description.clone().unwrap_or_default(),
                        kind: SuggestionType::Subcommand,
                        priority: sc.priority.unwrap_or(50),
                        is_dangerous: sc.is_dangerous,
                    });
                }
            }

            // 2. Options (exclude already used, respect exclusiveOn)
            collect_options(current_cmd, &ctx.used_options, &mut suggestions);

            // 3. Persistent options from root and parent subcommands
            if !ctx.spec_path.is_empty() {
                // Include root spec's persistent options (global flags like --verbose)
                let mut parent = spec;
                // Collect from root first
                if let Some(opts) = &parent.options {
                    for opt in opts.iter().filter(|o| o.is_persistent) {
                        if !ctx
                            .used_options
                            .iter()
                            .any(|u| opt.names().contains(&u.as_str()))
                        {
                            push_option(opt, &mut suggestions);
                        }
                    }
                }
                // Then walk through intermediate parents
                for &idx in &ctx.spec_path {
                    match parent.subcommands.as_ref().and_then(|sc| sc.get(idx)) {
                        Some(subcmd) => parent = subcmd,
                        None => break,
                    }
                    // Skip the current (deepest) subcommand since its options are already collected above
                    if std::ptr::eq(parent, current_cmd) {
                        break;
                    }
                    if let Some(opts) = &parent.options {
                        for opt in opts.iter().filter(|o| o.is_persistent) {
                            if !ctx
                                .used_options
                                .iter()
                                .any(|u| opt.names().contains(&u.as_str()))
                            {
                                push_option(opt, &mut suggestions);
                            }
                        }
                    }
                }
            }

            // 4. Positional arg suggestions
            if let Some(args) = &current_cmd.args {
                let arg_list: Vec<&Arg> = match args {
                    SingleOrArray::Single(a) => vec![a],
                    SingleOrArray::Array(a) => a.iter().collect(),
                };

                // Find the arg at the current index (or the last variadic one)
                let arg = if *arg_index < arg_list.len() {
                    Some(arg_list[*arg_index])
                } else {
                    // Check if last arg is variadic
                    arg_list.last().filter(|a| a.is_variadic).copied()
                };

                if let Some(arg) = arg {
                    collect_arg_suggestions(arg, cwd, &ctx.current_token, &mut suggestions);
                }
            }

            // 5. Additional suggestions
            if let Some(additional) = &current_cmd.additional_suggestions {
                for s in additional {
                    let suggestion = s.clone().into_suggestion();
                    push_suggestion(&suggestion, SuggestionType::Special, &mut suggestions);
                }
            }
        }

        ExpectedCompletion::OptionArg {
            option_name,
            arg_index,
        } => {
            // Find the option and its arg at the given index
            if let Some(opts) = &current_cmd.options {
                if let Some(opt) = opts
                    .iter()
                    .find(|o| o.names().iter().any(|n| n == option_name))
                {
                    if let Some(args) = &opt.args {
                        let arg_list: Vec<&Arg> = match args {
                            SingleOrArray::Single(a) => vec![a],
                            SingleOrArray::Array(a) => a.iter().collect(),
                        };
                        if let Some(arg) = arg_list.get(*arg_index) {
                            collect_arg_suggestions(arg, cwd, &ctx.current_token, &mut suggestions);
                        }
                    }
                }
            }
        }

        ExpectedCompletion::None => {
            // After "--", only positional args (typically filepaths)
            if let Some(args) = &current_cmd.args {
                let arg_list: Vec<&Arg> = match args {
                    SingleOrArray::Single(a) => vec![a],
                    SingleOrArray::Array(a) => a.iter().collect(),
                };
                if let Some(arg) = arg_list.last() {
                    collect_arg_suggestions(arg, cwd, &ctx.current_token, &mut suggestions);
                }
            }
        }
    }

    suggestions
}

/// Walk the spec tree using the spec_path indices.
/// Returns the deepest reachable subcommand (safe against stale indices from hot-reload).
fn walk_spec<'a>(spec: &'a Spec, path: &[usize]) -> &'a Subcommand {
    let mut current = spec;
    for &idx in path {
        match current.subcommands.as_ref().and_then(|sc| sc.get(idx)) {
            Some(subcmd) => current = subcmd,
            None => break,
        }
    }
    current
}

/// Collect option suggestions, excluding already-used ones.
fn collect_options(cmd: &Subcommand, used: &[String], suggestions: &mut Vec<ResolvedSuggestion>) {
    if let Some(opts) = &cmd.options {
        for opt in opts {
            // Skip if already used (unless repeatable)
            let already_used = opt.names().iter().any(|n| used.contains(&n.to_string()));
            if already_used {
                // TODO: check is_repeatable
                continue;
            }

            // Skip if excluded by another option
            if let Some(exclusive) = &opt.exclusive_on {
                if exclusive.iter().any(|e| used.contains(e)) {
                    continue;
                }
            }

            if opt.hidden {
                continue;
            }

            push_option(opt, suggestions);
        }
    }
}

/// Convert an Opt into a ResolvedSuggestion.
fn push_option(opt: &Opt, suggestions: &mut Vec<ResolvedSuggestion>) {
    // Prefer long name for display, but match on all names
    let primary = opt.long_name().unwrap_or_else(|| opt.primary_name());
    suggestions.push(ResolvedSuggestion {
        match_text: primary.to_string(),
        display_text: opt
            .display_name
            .clone()
            .unwrap_or_else(|| primary.to_string()),
        insert_text: opt
            .insert_value
            .clone()
            .unwrap_or_else(|| primary.to_string()),
        description: opt.description.clone().unwrap_or_default(),
        kind: SuggestionType::Option,
        priority: opt.priority.unwrap_or(50),
        is_dangerous: opt.is_dangerous,
    });
}

/// Collect suggestions for a positional argument.
fn collect_arg_suggestions(
    arg: &Arg,
    cwd: &str,
    partial_token: &str,
    suggestions: &mut Vec<ResolvedSuggestion>,
) {
    // Static suggestions
    if let Some(statics) = &arg.suggestions {
        for s in statics {
            let suggestion = s.clone().into_suggestion();
            push_suggestion(&suggestion, SuggestionType::Arg, suggestions);
        }
    }

    // Template-based suggestions
    if let Some(template) = &arg.template {
        let templates = match template {
            SingleOrArray::Single(t) => vec![*t],
            SingleOrArray::Array(ts) => ts.clone(),
        };
        for t in templates {
            expand_template(t, cwd, partial_token, suggestions);
        }
    }

    // Generator templates
    if let Some(generators) = &arg.generators {
        let gens = match generators {
            SingleOrArray::Single(g) => vec![g],
            SingleOrArray::Array(gs) => gs.iter().collect(),
        };
        for gen in gens {
            if let Some(template) = &gen.template {
                let templates = match template {
                    SingleOrArray::Single(t) => vec![*t],
                    SingleOrArray::Array(ts) => ts.clone(),
                };
                for t in templates {
                    expand_template(t, cwd, partial_token, suggestions);
                }
            }
            // TODO: execute generator scripts for dynamic suggestions
        }
    }
}

/// Expand a template into filesystem suggestions.
/// If `partial_token` contains a path prefix (e.g. "./src/eng"), the directory
/// component is resolved relative to `cwd` for listing.
fn expand_template(
    template: TemplateString,
    cwd: &str,
    partial_token: &str,
    suggestions: &mut Vec<ResolvedSuggestion>,
) {
    // Resolve directory from partial token's path component
    let (dir_path, prefix) = if partial_token.contains('/') {
        let token_path = Path::new(partial_token);
        let dir = token_path.parent().unwrap_or(Path::new(""));
        let resolved = if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            Path::new(cwd).join(dir)
        };
        let _file_prefix = token_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("");
        let dir_prefix = if partial_token.starts_with('/') {
            dir.to_string_lossy().to_string()
        } else {
            let d = dir.to_string_lossy();
            if d.is_empty() {
                String::new()
            } else {
                format!("{d}/")
            }
        };
        (resolved, dir_prefix)
    } else {
        (Path::new(cwd).to_path_buf(), String::new())
    };

    let entries = match std::fs::read_dir(&dir_path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

        match template {
            TemplateString::Folders => {
                if is_dir {
                    suggestions.push(ResolvedSuggestion {
                        match_text: file_name.clone(),
                        display_text: format!("{file_name}/"),
                        insert_text: format!("{prefix}{file_name}/"),
                        description: String::new(),
                        kind: SuggestionType::Folder,
                        priority: 50,
                        is_dangerous: false,
                    });
                }
            }
            TemplateString::Filepaths => {
                let kind = if is_dir {
                    SuggestionType::Folder
                } else {
                    SuggestionType::File
                };
                let display = if is_dir {
                    format!("{file_name}/")
                } else {
                    file_name.clone()
                };
                suggestions.push(ResolvedSuggestion {
                    match_text: file_name.clone(),
                    display_text: display,
                    insert_text: if is_dir {
                        format!("{prefix}{file_name}/")
                    } else {
                        format!("{prefix}{file_name}")
                    },
                    description: String::new(),
                    kind,
                    priority: 50,
                    is_dangerous: false,
                });
            }
            TemplateString::History | TemplateString::Help => {
                // TODO: implement history and help templates
            }
        }
    }
}

/// Convert a Suggestion spec object into a ResolvedSuggestion.
fn push_suggestion(
    s: &Suggestion,
    default_kind: SuggestionType,
    suggestions: &mut Vec<ResolvedSuggestion>,
) {
    let names = s.name.as_ref().map(|n| n.to_vec()).unwrap_or_default();
    let primary = names.first().cloned().unwrap_or_default();

    suggestions.push(ResolvedSuggestion {
        match_text: primary.clone(),
        display_text: s.display_name.clone().unwrap_or_else(|| primary.clone()),
        insert_text: s.insert_value.clone().unwrap_or(primary),
        description: s.description.clone().unwrap_or_default(),
        kind: s.suggestion_type.unwrap_or(default_kind),
        priority: s.priority.unwrap_or(50),
        is_dangerous: s.is_dangerous,
    });
}
