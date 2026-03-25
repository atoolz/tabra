//! Fuzzy matching engine backed by nucleo.
//!
//! Takes the current token (query) and a list of ResolvedSuggestions,
//! scores each one, and returns a ranked list of matches.

use crate::engine::resolver::ResolvedSuggestion;
use crate::spec::types::FilterStrategy;
use nucleo::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo::Matcher;
use nucleo::Utf32Str;

/// A scored suggestion after matching.
#[derive(Debug, Clone)]
pub struct ScoredSuggestion {
    pub suggestion: ResolvedSuggestion,
    /// Match score from nucleo (higher is better). None = no match.
    pub score: Option<u32>,
    /// Byte ranges of the matched characters in match_text (for highlighting).
    pub match_indices: Vec<u32>,
}

/// Match and rank suggestions against the query token.
///
/// Returns suggestions sorted by: score (desc), then priority (desc), then alphabetical.
/// Non-matching suggestions are filtered out unless `query` is empty (in which
/// case all suggestions are returned, sorted by priority).
pub fn match_suggestions(
    query: &str,
    suggestions: &[ResolvedSuggestion],
    strategy: FilterStrategy,
) -> Vec<ScoredSuggestion> {
    if query.is_empty() {
        // No filtering, just sort by priority desc then alphabetical
        let mut scored: Vec<ScoredSuggestion> = suggestions
            .iter()
            .map(|s| ScoredSuggestion {
                suggestion: s.clone(),
                score: None,
                match_indices: Vec::new(),
            })
            .collect();
        scored.sort_by(|a, b| {
            b.suggestion
                .priority
                .cmp(&a.suggestion.priority)
                .then_with(|| a.suggestion.match_text.cmp(&b.suggestion.match_text))
        });
        return scored;
    }

    let atom_kind = match strategy {
        FilterStrategy::Prefix => AtomKind::Prefix,
        FilterStrategy::Fuzzy => AtomKind::Fuzzy,
        FilterStrategy::Default => AtomKind::Fuzzy,
    };

    let pattern = Atom::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        atom_kind,
        false,
    );

    let mut matcher = Matcher::default();

    let mut scored: Vec<ScoredSuggestion> = suggestions
        .iter()
        .filter_map(|s| {
            let mut buf = Vec::new();
            let haystack = Utf32Str::new(&s.match_text, &mut buf);
            let mut indices = Vec::new();
            let score = pattern.indices(haystack, &mut matcher, &mut indices);
            score.map(|sc| ScoredSuggestion {
                suggestion: s.clone(),
                score: Some(sc as u32),
                match_indices: indices,
            })
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.suggestion.priority.cmp(&a.suggestion.priority))
            .then_with(|| a.suggestion.match_text.cmp(&b.suggestion.match_text))
    });

    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::types::SuggestionType;

    fn make_suggestion(name: &str, priority: u8) -> ResolvedSuggestion {
        ResolvedSuggestion {
            match_text: name.to_string(),
            display_text: name.to_string(),
            insert_text: name.to_string(),
            description: String::new(),
            kind: SuggestionType::Subcommand,
            priority,
            is_dangerous: false,
        }
    }

    #[test]
    fn test_empty_query_returns_all() {
        let suggestions = vec![
            make_suggestion("commit", 50),
            make_suggestion("checkout", 50),
            make_suggestion("clone", 50),
        ];
        let results = match_suggestions("", &suggestions, FilterStrategy::Default);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_fuzzy_match() {
        let suggestions = vec![
            make_suggestion("commit", 50),
            make_suggestion("checkout", 50),
            make_suggestion("push", 50),
        ];
        let results = match_suggestions("co", &suggestions, FilterStrategy::Default);
        // "commit" and "checkout" should match, "push" should not
        assert!(results.len() >= 2);
        assert!(results.iter().all(|r| r.suggestion.match_text != "push"));
    }
}
