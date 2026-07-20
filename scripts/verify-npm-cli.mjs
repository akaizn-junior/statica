#!/usr/bin/env node
/**
 * Smoke-test @statica/cli from local npm package paths (CI + after pack).
 *
 * Usage:
 *   node scripts/verify-npm-cli.mjs
 *   node scripts/verify-npm-cli.mjs --workspace /path/to/statica
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  let workspace = path.resolve(__dirname, "..");
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--workspace") workspace = path.resolve(argv[++i]);
    else if (argv[i] === "--help" || argv[i] === "-h") {
      console.log("Usage: node scripts/verify-npm-cli.mjs [--workspace DIR]");
      process.exit(0);
    }
  }
  return { workspace };
}

const PLATFORMS = {
  "darwin-arm64": "cli-darwin-arm64",
  "darwin-x64": "cli-darwin-x64",
  "linux-x64": "cli-linux-x64-gnu",
  "linux-arm64": "cli-linux-arm64-gnu",
  "win32-x64": "cli-win32-x64",
};

const key = `${process.platform}-${process.arch}`;
const platformDir = PLATFORMS[key];

function main() {
  const { workspace } = parseArgs(process.argv.slice(2));
  if (!platformDir) {
    console.log(`skip: no local platform mapping for ${key}`);
    process.exit(0);
  }

  const npmRoot = path.join(workspace, "npm", "@statica");
  const cliPkg = path.join(npmRoot, "cli");
  const platformPkg = path.join(npmRoot, platformDir);
  const binaryName = process.platform === "win32" ? "statica.exe" : "statica";
  const binaryPath = path.join(platformPkg, "bin", binaryName);

  if (!fs.existsSync(binaryPath)) {
    console.error(`missing platform binary: ${binaryPath}`);
    console.error("Run: node scripts/pack-npm.mjs --artifacts-dir …");
    process.exit(1);
  }

  const stat = fs.statSync(binaryPath);
  if (!stat.isFile() || stat.size < 1024) {
    console.error(`invalid binary at ${binaryPath} (size ${stat.size})`);
    process.exit(1);
  }

  const smokeDir = fs.mkdtempSync(path.join(process.env.TMPDIR || "/tmp", "statica-npm-smoke-"));
  const pkgJson = path.join(smokeDir, "package.json");
  fs.writeFileSync(
    pkgJson,
    JSON.stringify({ name: "statica-npm-smoke", private: true }, null, 2),
  );

  execFileSync(process.execPath, ["--version"], { cwd: smokeDir, stdio: "ignore" });

  // file: installs — same layout npm users get from optionalDependencies
  execFileSync("npm", ["install", platformPkg, cliPkg], {
    cwd: smokeDir,
    stdio: "inherit",
  });

  const shim = path.join(smokeDir, "node_modules", "@statica", "cli", "bin", "statica.js");
  if (!fs.existsSync(shim)) {
    console.error(`shim not installed: ${shim}`);
    process.exit(1);
  }

  const out = execFileSync(process.execPath, [shim, "-v"], {
    cwd: smokeDir,
    encoding: "utf8",
  }).trim();

  if (!out.startsWith("statica ")) {
    console.error(`unexpected -v output: ${out}`);
    process.exit(1);
  }

  console.log(`ok: ${out}`);
}

main();
