#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");

function usage() {
  console.log(`Create a new statica site.

Usage:
  create-statica <name>
  npm create statica@latest my-site

Options:
  -h, --help     Show help
  -v, --version  Show version`);
}

const args = process.argv.slice(2);

if (args.includes("--help") || args.includes("-h")) {
  usage();
  process.exit(0);
}

if (args.includes("--version") || args.includes("-v")) {
  console.log(require("../package.json").version);
  process.exit(0);
}

const name = args.find((arg) => !arg.startsWith("-"));

if (!name) {
  usage();
  process.exit(1);
}

const extra = args.filter((arg) => arg !== name);
if (extra.length > 0) {
  console.error(`Unknown option: ${extra[0]}`);
  usage();
  process.exit(1);
}

let staticaShim;
try {
  staticaShim = require.resolve("@statica/cli/bin/statica.js", {
    paths: [process.cwd(), __dirname],
  });
} catch {
  console.error("Could not find @statica/cli. Reinstall create-statica and try again.");
  process.exit(1);
}

const result = spawnSync(process.execPath, [staticaShim, "new", name], {
  stdio: "inherit",
  env: process.env,
  windowsHide: true,
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(typeof result.status === "number" ? result.status : 1);
