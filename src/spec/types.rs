//! Core data structures mapping to the withfig/autocomplete spec format.
//!
//! These types represent the **static** subset of Fig specs that Tabra can
//! consume. Dynamic features (generators with `script`, `custom` functions,
//! `loadSpec` callbacks) are represented as data but only the static parts
//! (suggestions, templates) are evaluated at completion time. Generator
//! scripts may be executed by the daemon in the future.
//!
//! Reference: @withfig/autocomplete-types v1.31.0

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Utility types
// ---------------------------------------------------------------------------

/// Mirrors Fig's `SingleOrArray<T>`: a value can be a single item or a vec.
/// Deserializes transparently from both `"value"` and `["a", "b"]` in JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SingleOrArray<T> {
    Single(T),
    Array(Vec<T>),
}

impl<T> SingleOrArray<T> {
    pub fn into_vec(self) -> Vec<T> {
        match self {
            SingleOrArray::Single(v) => vec![v],
            SingleOrArray::Array(v) => v,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match self {
            SingleOrArray::Single(v) => std::slice::from_ref(v),
            SingleOrArray::Array(v) => v,
        }
    }
}

impl<T: Clone> SingleOrArray<T> {
    pub fn to_vec(&self) -> Vec<T> {
        match self {
            SingleOrArray::Single(v) => vec![v.clone()],
            SingleOrArray::Array(v) => v.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// SuggestionType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionType {
    Folder,
    File,
    Arg,
    Subcommand,
    Option,
    Special,
    Mixin,
    Shortcut,
}

// ---------------------------------------------------------------------------
// Template
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TemplateString {
    Filepaths,
    Folders,
    History,
    Help,
}

pub type Template = SingleOrArray<TemplateString>;

// ---------------------------------------------------------------------------
// FilterStrategy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum FilterStrategy {
    Fuzzy,
    Prefix,
    #[default]
    Default,
}

// ---------------------------------------------------------------------------
// Suggestion
// ---------------------------------------------------------------------------

/// A completion suggestion displayed in the popup.
/// Maps to Fig's `Suggestion` interface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// The string(s) used for matching/filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<SingleOrArray<String>>,

    /// Display text shown in the popup (defaults to name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Text inserted into the terminal on selection (defaults to name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_value: Option<String>,

    /// Description shown in the popup detail area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Icon identifier (single char, emoji, URL, or fig:// protocol).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Ranking priority 0..100. Higher ranks higher. Default 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,

    /// Type controls the default icon and autoexecute behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub suggestion_type: Option<SuggestionType>,

    /// If true, suggestion is hidden unless the user types the exact name.
    #[serde(default)]
    pub hidden: bool,

    /// If true, this is a dangerous action (no autoexecute).
    #[serde(default)]
    pub is_dangerous: bool,
}

// ---------------------------------------------------------------------------
// Generator
// ---------------------------------------------------------------------------

/// A generator produces dynamic suggestions at runtime.
/// For MVP, Tabra supports `template` and `splitOn` generators statically.
/// `script` generators will be executed by the daemon in a future version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Generator {
    /// Built-in template generator (filepaths, folders, history, help).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,

    /// Shell command to execute. Array of strings: ["git", "branch"].
    /// Can also be a function in TS specs, but JSON-compiled specs
    /// may inline the resolved command array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<serde_json::Value>,

    /// Split script output on this delimiter to produce suggestions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_on: Option<String>,

    /// Execution timeout in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_timeout: Option<u64>,

    /// Custom function (opaque in JSON, not executable by Tabra).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<serde_json::Value>,

    /// Post-process function (opaque in JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_process: Option<serde_json::Value>,

    /// Trigger for re-generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<serde_json::Value>,

    /// Query term extractor (opaque).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get_query_term: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Arg
// ---------------------------------------------------------------------------

/// A positional argument definition.
/// Maps to Fig's `Arg` interface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Arg {
    /// Human-readable name for the argument (display only, not for parsing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Description text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Static suggestions for this argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestions: Option<Vec<SuggestionOrString>>,

    /// Template-based generation (filepaths, folders).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,

    /// Dynamic generators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generators: Option<SingleOrArray<Generator>>,

    /// Filter strategy for suggestions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_strategy: Option<FilterStrategy>,

    /// True if the argument is optional.
    #[serde(default)]
    pub is_optional: bool,

    /// True if the argument accepts multiple values (variadic).
    #[serde(default)]
    pub is_variadic: bool,

    /// True if this argument is actually a new command to complete.
    #[serde(default)]
    pub is_command: bool,

    /// True for dangerous arguments (no autoexecute).
    #[serde(default)]
    pub is_dangerous: bool,

    /// Default value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,

    /// Debounce keystroke events for this arg.
    #[serde(default)]
    pub debounce: bool,

    /// Whether options can interrupt variadic args.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_can_break_variadic_arg: Option<bool>,

    /// Suggest current token at top of list.
    #[serde(default)]
    pub suggest_current_token: bool,

    /// Lazy-load a sub-spec for this argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_spec: Option<serde_json::Value>,
}

/// Fig specs allow suggestions to be either a string or a Suggestion object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SuggestionOrString {
    String(String),
    Suggestion(Suggestion),
}

impl SuggestionOrString {
    /// Normalize to a full Suggestion.
    pub fn into_suggestion(self) -> Suggestion {
        match self {
            SuggestionOrString::String(s) => Suggestion {
                name: Some(SingleOrArray::Single(s)),
                ..Default::default()
            },
            SuggestionOrString::Suggestion(s) => s,
        }
    }
}

// ---------------------------------------------------------------------------
// Option (CLI flag/option)
// ---------------------------------------------------------------------------

/// A CLI option/flag definition.
/// Maps to Fig's `Option` interface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Opt {
    /// The exact flag name(s) as typed by the user: "-m", "--message", or both.
    pub name: SingleOrArray<String>,

    /// Display text in popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Text inserted on selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_value: Option<String>,

    /// Description shown in the popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Icon identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Ranking priority 0..100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,

    /// Arguments this option accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<SingleOrArray<Arg>>,

    /// Persistent across child subcommands (like cobra persistent flags).
    #[serde(default)]
    pub is_persistent: bool,

    /// Whether this option is required.
    #[serde(default)]
    pub is_required: bool,

    /// Whether an `=` separator is required for the argument.
    #[serde(default)]
    pub requires_equals: bool,

    /// Separator required between option and argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_separator: Option<serde_json::Value>,

    /// How many times this option can be repeated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_repeatable: Option<serde_json::Value>,

    /// Options that are mutually exclusive with this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive_on: Option<Vec<String>>,

    /// Options this one depends on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,

    /// Hidden unless exact match.
    #[serde(default)]
    pub hidden: bool,

    /// Dangerous action.
    #[serde(default)]
    pub is_dangerous: bool,

    /// Deprecated flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// ParserDirectives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ParserDirectives {
    /// Flags with single hyphen can have >1 char (disables chaining).
    #[serde(default)]
    pub flags_are_posix_noncompliant: bool,

    /// Options must come before any arguments.
    #[serde(default)]
    pub options_must_precede_arguments: bool,

    /// Required separators between option name and its argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_arg_separators: Option<SingleOrArray<String>>,
}

// ---------------------------------------------------------------------------
// Subcommand (and top-level Spec)
// ---------------------------------------------------------------------------

/// A subcommand definition. The top-level spec is also a Subcommand.
/// Maps to Fig's `Subcommand` interface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Subcommand {
    /// The subcommand name(s). Aliases are additional entries.
    pub name: SingleOrArray<String>,

    /// Display text in popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Text inserted on selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_value: Option<String>,

    /// Description shown in popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Icon identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Ranking priority 0..100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,

    /// Nested subcommands (recursive tree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcommands: Option<Vec<Subcommand>>,

    /// Whether a subcommand is mandatory.
    #[serde(default)]
    pub requires_subcommand: bool,

    /// Options/flags available on this subcommand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Opt>>,

    /// Positional arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<SingleOrArray<Arg>>,

    /// Filter strategy for suggestions under this subcommand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_strategy: Option<FilterStrategy>,

    /// Additional suggestions appended to the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_suggestions: Option<Vec<SuggestionOrString>>,

    /// Lazy-load another spec (string path or object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_spec: Option<serde_json::Value>,

    /// Dynamic spec generation (opaque in static JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_spec: Option<serde_json::Value>,

    /// Parser configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parser_directives: Option<ParserDirectives>,

    /// Whether to cache loadSpec/generateSpec results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<bool>,

    /// Hidden unless exact match.
    #[serde(default)]
    pub hidden: bool,

    /// Dangerous action.
    #[serde(default)]
    pub is_dangerous: bool,

    /// Deprecated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Spec (top-level alias)
// ---------------------------------------------------------------------------

/// A complete completion spec. This is the top-level Subcommand loaded from
/// a JSON file. The `name` field matches the CLI tool name (e.g. "git").
pub type Spec = Subcommand;

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl Subcommand {
    /// Get all names (primary + aliases) as a vec of &str.
    pub fn names(&self) -> Vec<&str> {
        match &self.name {
            SingleOrArray::Single(n) => vec![n.as_str()],
            SingleOrArray::Array(ns) => ns.iter().map(|s| s.as_str()).collect(),
        }
    }

    /// Get the primary (first) name.
    pub fn primary_name(&self) -> &str {
        match &self.name {
            SingleOrArray::Single(n) => n.as_str(),
            SingleOrArray::Array(ns) => ns.first().map(|s| s.as_str()).unwrap_or(""),
        }
    }
}

impl Opt {
    /// Get all names (e.g. ["-m", "--message"]) as a vec of &str.
    pub fn names(&self) -> Vec<&str> {
        match &self.name {
            SingleOrArray::Single(n) => vec![n.as_str()],
            SingleOrArray::Array(ns) => ns.iter().map(|s| s.as_str()).collect(),
        }
    }

    /// Get the primary (first) name.
    pub fn primary_name(&self) -> &str {
        match &self.name {
            SingleOrArray::Single(n) => n.as_str(),
            SingleOrArray::Array(ns) => ns.first().map(|s| s.as_str()).unwrap_or(""),
        }
    }

    /// Get the long-form name if available (starts with "--"), else primary.
    pub fn long_name(&self) -> Option<&str> {
        match &self.name {
            SingleOrArray::Single(n) => {
                if n.starts_with("--") {
                    Some(n.as_str())
                } else {
                    None
                }
            }
            SingleOrArray::Array(ns) => ns.iter().find(|n| n.starts_with("--")).map(|s| s.as_str()),
        }
    }
}
