#!/usr/bin/env node
"use strict";

const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

/** @type {Record<string, string>} */
const PLATFORMS = {
  "darwin-arm64": "@statica/cli-darwin-arm64",
  "darwin-x64": "@statica/cli-darwin-x64",
  "linux-x64": "@statica/cli-linux-x64-gnu",
  "linux-arm64": "@statica/cli-linux-arm64-gnu",
  "win32-x64": "@statica/cli-win32-x64",
};

const key = `${process.platform}-${process.arch}`;
const platformPkg = PLATFORMS[key];
const binaryName = process.platform === "win32" ? "statica.exe" : "statica";

/** @param {string} message */
function fail(message) {
  console.error(message);
  process.exit(1);
}

if (!platformPkg) {
  fail(
    `statica does not ship a prebuilt binary for ${key}.\n` +
      `Supported: ${Object.keys(PLATFORMS).join(", ")}.\n` +
      `Install via Rust instead: cargo install statica --locked`,
  );
}

/** @returns {string} */
function resolveBinary() {
  /** @type {string[]} */
  const roots = [];

  try {
    roots.push(path.dirname(require.resolve(`${platformPkg}/package.json`)));
  } catch {
    // optionalDependency not installed into node_modules
  }

  // Monorepo / local pack layout: npm/@statica/cli-darwin-arm64 next to cli/
  roots.push(
    path.resolve(__dirname, "..", "..", platformPkg.replace("@statica/", "")),
  );

  for (const root of roots) {
    const candidate = path.join(root, "bin", binaryName);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  fail(
    `Could not find the statica binary from optional dependency ${platformPkg}.\n` +
      `Optional dependencies may have been skipped. Reinstall without --omit=optional\n` +
      `(pnpm: do not set optional=false; yarn: avoid --ignore-optional).\n` +
      `Or install via Rust: cargo install statica --locked`,
  );
}

const binPath = resolveBinary();
const args = process.argv.slice(2);

try {
  execFileSync(binPath, args, {
    stdio: "inherit",
    env: process.env,
    windowsHide: true,
  });
} catch (err) {
  const error = /** @type {NodeJS.ErrnoException & { status?: number; signal?: NodeJS.Signals }} */ (
    err
  );
  if (error.signal) {
    process.kill(process.pid, error.signal);
  }
  process.exit(typeof error.status === "number" ? error.status : 1);
}
