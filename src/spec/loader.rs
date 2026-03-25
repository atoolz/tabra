//! Spec loading, indexing, and hot-reload.
//!
//! Specs are stored as JSON files in:
//!   ~/.local/share/tabra/specs/<tool>.json
//!
//! The loader builds an in-memory index mapping CLI tool names to their
//! parsed Spec. It watches the specs directory for changes and reloads
//! modified specs automatically.

use crate::spec::types::Spec;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// In-memory index of all loaded specs, keyed by tool name.
#[derive(Default)]
pub struct SpecIndex {
    specs: HashMap<String, Spec>,
    specs_dir: PathBuf,
}

impl SpecIndex {
    /// Create an empty index (for testing/benchmarking).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a spec into the index.
    pub fn insert(&mut self, name: String, spec: Spec) {
        self.specs.insert(name, spec);
    }

    /// Create a new index and load all specs from the directory.
    pub fn load(specs_dir: PathBuf) -> Result<Self> {
        let mut index = Self {
            specs: HashMap::new(),
            specs_dir,
        };
        index.reload_all()?;
        Ok(index)
    }

    /// Get the specs directory path.
    pub fn specs_dir(&self) -> &Path {
        &self.specs_dir
    }

    /// Look up a spec by CLI tool name (e.g. "git", "docker").
    pub fn get(&self, tool_name: &str) -> Option<&Spec> {
        self.specs.get(tool_name)
    }

    /// Number of loaded specs.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Reload all specs from the directory.
    pub fn reload_all(&mut self) -> Result<()> {
        self.specs.clear();

        if !self.specs_dir.exists() {
            debug!("specs directory does not exist yet: {:?}", self.specs_dir);
            return Ok(());
        }

        let entries = fs::read_dir(&self.specs_dir)
            .with_context(|| format!("reading specs dir: {:?}", self.specs_dir))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                match self.load_spec_file(&path) {
                    Ok((name, spec)) => {
                        debug!("loaded spec: {name}");
                        self.specs.insert(name, spec);
                    }
                    Err(e) => {
                        warn!("failed to load spec {:?}: {e:#}", path);
                    }
                }
            }
        }

        info!(
            "loaded {} specs from {:?}",
            self.specs.len(),
            self.specs_dir
        );
        Ok(())
    }

    /// Reload a single spec file (used by the file watcher).
    pub fn reload_file(&mut self, path: &Path) -> Result<()> {
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            return Ok(());
        }

        match self.load_spec_file(path) {
            Ok((name, spec)) => {
                info!("reloaded spec: {name}");
                self.specs.insert(name, spec);
            }
            Err(e) => {
                warn!("failed to reload spec {:?}: {e:#}", path);
            }
        }
        Ok(())
    }

    /// Remove a spec when its file is deleted.
    pub fn remove_file(&mut self, path: &Path) {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if self.specs.remove(stem).is_some() {
                info!("removed spec: {stem}");
            }
        }
    }

    /// Parse a single JSON spec file. Returns (tool_name, Spec).
    fn load_spec_file(&self, path: &Path) -> Result<(String, Spec)> {
        let content = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
        let spec: Spec =
            serde_json::from_str(&content).with_context(|| format!("parsing {:?}", path))?;

        // Use the file stem as the tool name (e.g. "git.json" -> "git")
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        Ok((name, spec))
    }
}

/// Default specs directory: ~/.local/share/tabra/specs
pub fn default_specs_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("tabra")
        .join("specs")
}

/// Install (copy) spec JSON files from a source directory into the
/// Tabra specs directory.
pub fn install_specs(from: &Path) -> Result<()> {
    let target = default_specs_dir();
    fs::create_dir_all(&target).with_context(|| format!("creating specs dir: {:?}", target))?;

    let entries = fs::read_dir(from).with_context(|| format!("reading source dir: {:?}", from))?;

    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let dest = target.join(entry.file_name());
            fs::copy(&path, &dest).with_context(|| format!("copying {:?} -> {:?}", path, dest))?;
            count += 1;
        }
    }

    info!("installed {count} specs to {:?}", target);
    Ok(())
}

/// Validate a directory of JSON specs by deserializing each one.
/// Reports which specs pass and which fail with error details.
pub fn validate_specs(from: &Path) -> Result<()> {
    let entries = fs::read_dir(from).with_context(|| format!("reading specs dir: {:?}", from))?;

    let mut ok = 0;
    let mut failures: Vec<(String, String)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("  FAIL {name}: read error: {e}");
                failures.push((name, format!("read error: {e}")));
                continue;
            }
        };

        match serde_json::from_str::<crate::spec::types::Spec>(&content) {
            Ok(spec) => {
                let subcmd_count = spec.subcommands.as_ref().map_or(0, |s| s.len());
                let opt_count = spec.options.as_ref().map_or(0, |o| o.len());
                info!("  OK  {name} ({subcmd_count} subcommands, {opt_count} options)");
                ok += 1;
            }
            Err(e) => {
                warn!("  FAIL {name}: {e}");
                failures.push((name, e.to_string()));
            }
        }
    }

    info!("{ok} specs valid, {} failed", failures.len());

    if !failures.is_empty() {
        for (name, err) in &failures {
            eprintln!("  {name}: {err}");
        }
        anyhow::bail!("{} specs failed validation", failures.len());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_load_bundled_specs() {
        let specs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("specs");
        if !specs_dir.exists() {
            panic!(
                "specs/ directory not found. Run `make specs` or `node scripts/compile-specs.mjs` first."
            );
        }

        let entries: Vec<_> = fs::read_dir(&specs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();

        assert!(!entries.is_empty(), "No JSON specs found in specs/");

        let mut ok = 0;
        let mut failures = Vec::new();

        for entry in &entries {
            let path = entry.path();
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();

            match serde_json::from_str::<crate::spec::types::Spec>(&content) {
                Ok(_) => ok += 1,
                Err(e) => failures.push(format!("{name}: {e}")),
            }
        }

        if !failures.is_empty() {
            panic!(
                "{} of {} specs failed to deserialize:\n{}",
                failures.len(),
                entries.len(),
                failures.join("\n")
            );
        }

        eprintln!("{ok}/{} specs loaded successfully", entries.len());
    }
}
