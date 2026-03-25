#!/usr/bin/env node

/**
 * Tabra Spec Compiler
 *
 * Compiles withfig/autocomplete specs from their compiled JavaScript format
 * into JSON that the Tabra Rust daemon can load.
 *
 * Usage:
 *   node scripts/compile-specs.mjs [--from <dir>] [--out <dir>] [--top <n>]
 *
 * Steps:
 *   1. Clones withfig/autocomplete (or uses --from for a local checkout)
 *   2. Runs `npm install && npm run build` to compile TS specs to JS
 *   3. Requires each compiled JS file and serializes the static parts to JSON
 *   4. Writes JSON files to --out (default: specs/)
 *
 * Functions in specs (generators, postProcess, custom) are replaced with
 * metadata markers so the Rust daemon knows they exist but doesn't try
 * to execute JavaScript.
 */

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { createRequire } from "node:module";

const args = process.argv.slice(2);

function getArg(name, defaultVal) {
  const idx = args.indexOf(name);
  if (idx === -1 || idx + 1 >= args.length) return defaultVal;
  return args[idx + 1];
}

const TOP_N = parseInt(getArg("--top", "50"), 10);
const OUT_DIR = resolve(getArg("--out", "specs"));
const FROM_DIR = getArg("--from", null);

// Top CLI tools by usage (ordered by priority)
const TOP_TOOLS = [
  "git", "docker", "kubectl", "npm", "cargo", "aws", "terraform",
  "gh", "yarn", "pip", "brew", "apt", "curl", "ssh", "rsync",
  "grep", "find", "ls", "cat", "cd", "cp", "mv", "rm", "mkdir",
  "chmod", "chown", "tar", "gzip", "unzip", "wget", "make",
  "python", "python3", "node", "go", "rustc", "rustup",
  "systemctl", "journalctl", "tmux", "screen", "vim", "nano",
  "psql", "mysql", "redis-cli", "mongosh",
  "pnpm", "bun", "deno", "npx", "pip3", "conda",
  "helm", "kind", "minikube", "podman", "nix",
];

/**
 * Replace JavaScript functions with metadata markers.
 * This preserves the structure while making it JSON-serializable.
 */
function sanitize(obj, path = "") {
  if (obj === null || obj === undefined) return obj;
  if (typeof obj === "function") {
    return { __tabra_function: true, path };
  }
  if (Array.isArray(obj)) {
    return obj.map((item, i) => sanitize(item, `${path}[${i}]`));
  }
  if (typeof obj === "object") {
    const result = {};
    for (const [key, value] of Object.entries(obj)) {
      result[key] = sanitize(value, `${path}.${key}`);
    }
    return result;
  }
  return obj;
}

/**
 * Load a compiled JS spec file and extract the spec object.
 */
function loadSpec(filePath) {
  try {
    const require = createRequire(import.meta.url);
    const mod = require(filePath);
    const spec = mod.default || mod;
    if (!spec || typeof spec !== "object") return null;
    // Handle versioned specs (some specs export { versions: {...} })
    if (spec.versions && !spec.name) {
      const versions = Object.keys(spec.versions).sort();
      if (versions.length > 0) {
        return spec.versions[versions[versions.length - 1]];
      }
    }
    return spec;
  } catch (err) {
    console.error(`  Failed to load ${basename(filePath)}: ${err.message}`);
    return null;
  }
}

async function main() {
  let buildDir;

  if (FROM_DIR) {
    buildDir = resolve(FROM_DIR);
    if (!existsSync(buildDir)) {
      console.error(`Directory not found: ${buildDir}`);
      process.exit(1);
    }
    console.log(`Using pre-built specs from: ${buildDir}`);
  } else {
    // Clone and build withfig/autocomplete
    const tmpDir = join(resolve("."), ".withfig-autocomplete");

    if (!existsSync(tmpDir)) {
      console.log("Cloning withfig/autocomplete...");
      execFileSync("git", [
        "clone", "--depth", "1",
        "https://github.com/withfig/autocomplete.git", tmpDir
      ], { stdio: "inherit" });
    } else {
      console.log("Using existing clone at .withfig-autocomplete/");
    }

    console.log("Installing dependencies...");
    execFileSync("npm", ["install"], { cwd: tmpDir, stdio: "inherit" });

    console.log("Building specs...");
    try {
      execFileSync("npm", ["run", "build"], { cwd: tmpDir, stdio: "inherit" });
    } catch {
      // Some builds fail partially; continue with what we have
      console.warn("Build had errors, continuing with available specs...");
    }

    buildDir = join(tmpDir, "build");
  }

  if (!existsSync(buildDir)) {
    console.error(`Build directory not found: ${buildDir}`);
    process.exit(1);
  }

  // Create output directory
  mkdirSync(OUT_DIR, { recursive: true });

  // Find all .js spec files
  const allFiles = readdirSync(buildDir)
    .filter((f) => f.endsWith(".js") && !f.startsWith("index") && !f.startsWith("_"));

  // Determine which files to compile (top tools first, then fill up to TOP_N)
  const orderedFiles = [];

  // First pass: add top tools that exist
  for (const tool of TOP_TOOLS) {
    const fileName = `${tool}.js`;
    if (allFiles.includes(fileName)) {
      orderedFiles.push(fileName);
    }
  }

  // Second pass: fill remaining slots
  if (orderedFiles.length < TOP_N) {
    for (const f of allFiles) {
      if (orderedFiles.length >= TOP_N) break;
      if (!orderedFiles.includes(f)) {
        orderedFiles.push(f);
      }
    }
  }

  console.log(`\nCompiling ${orderedFiles.length} specs to JSON...\n`);

  let success = 0;
  let failed = 0;

  for (const file of orderedFiles) {
    const toolName = file.replace(/\.js$/, "");
    const filePath = join(buildDir, file);

    const spec = loadSpec(filePath);
    if (!spec) {
      failed++;
      continue;
    }

    // Sanitize functions to markers
    const sanitized = sanitize(spec);

    // Ensure the spec has a name field
    if (!sanitized.name) {
      sanitized.name = toolName;
    }

    try {
      const json = JSON.stringify(sanitized, null, 2);
      const outPath = join(OUT_DIR, `${toolName}.json`);
      writeFileSync(outPath, json + "\n");
      console.log(`  OK  ${toolName} (${(json.length / 1024).toFixed(1)}KB)`);
      success++;
    } catch (err) {
      console.error(`  FAIL ${toolName}: ${err.message}`);
      failed++;
    }
  }

  console.log(`\nDone: ${success} compiled, ${failed} failed`);
  console.log(`Output: ${OUT_DIR}/`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
