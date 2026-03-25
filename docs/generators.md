# Generator Execution Design

## Overview

Withfig specs use "generators" to produce dynamic completion suggestions.
A generator runs a shell command, captures stdout, and parses the output
into suggestions. Example: `git checkout <TAB>` runs `git branch` to
list branch names.

## Generator Types in Withfig Specs

From analysis of the top 10 specs (git, docker, kubectl, npm, gh, cargo,
aws, brew, pip, helm), generators appear in three forms:

### 1. Array Scripts (executable)
```json
{
  "script": ["git", "branch", "--format", "%(refname:short)"],
  "splitOn": "\n",
  "postProcess": { "__tabra_function": true }
}
```
An array of strings. The first element is the command, the rest are args.
Tabra executes these via `Command::new(&args[0]).args(&args[1..])`.

### 2. String Scripts (executable)
```json
{
  "script": "git branch --format '%(refname:short)'"
}
```
A single string. Tabra executes via `Command::new("sh").arg("-c").arg(script)`.

### 3. Custom Functions (not executable)
```json
{
  "custom": { "__tabra_function": true, "path": "..." }
}
```
JavaScript functions that were replaced with markers by the compiler.
Tabra cannot execute these. They are silently skipped.

### 4. Templates (built-in)
```json
{
  "template": "filepaths"
}
```
or `"folders"`. These are handled by the resolver's existing filepath
completion logic, not by executing external commands.

### 5. Function Scripts (not executable)
```json
{
  "script": { "__tabra_function": true, "path": "..." }
}
```
The `script` field is a JS function, not a string/array. Silently skipped.

## Execution Model

### Subprocess
- Array scripts: `Command::new(&args[0]).args(&args[1..])`
- String scripts: `Command::new("sh").arg("-c").arg(script_str)`
- `cwd` set to the request's working directory
- `stdout` captured, `stderr` discarded
- Timeout: `script_timeout` from spec (default 5000ms)

### Output Parsing
1. Capture stdout as string
2. Split on `split_on` field (default: `"\n"`)
3. Trim each line
4. Skip empty lines
5. Each line becomes a suggestion with `kind: Special`

### Post-Processing
The `postProcess` field is always a `__tabra_function` (JS). Tabra skips
it and uses raw output lines as suggestions. This means some generators
will produce slightly different results than Fig (which could transform
the output), but the raw output is usually useful enough.

## Caching

Generators are expensive (50-500ms per execution). The daemon caches results:

- **Cache key**: `(script_hash, cwd)` - same script in same directory = cache hit
- **TTL**: 30 seconds (configurable per spec via `cache.ttl`)
- **Invalidation**: time-based only (no filesystem watching)
- **Strategy**: return cached result immediately, refresh in background if stale

This ensures the <5ms p99 latency target for cached completions while
keeping results reasonably fresh.

## What Tabra Cannot Execute

| Type | Executable? | Reason |
|------|------------|--------|
| Array script | Yes | Direct subprocess |
| String script | Yes | Via `sh -c` |
| Custom function | No | JavaScript, needs JS runtime |
| Function script | No | JavaScript in script field |
| Template | Built-in | Handled by resolver |

From the top 10 specs: ~40% of generators are executable scripts,
~50% are custom JS functions (skipped), ~10% are templates (built-in).
