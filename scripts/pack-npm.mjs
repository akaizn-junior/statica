#!/usr/bin/env node
/**
 * Pack release binaries into npm/@statica/cli-* platform packages
 * and sync versions across all @statica packages.
 *
 * Usage:
 *   node scripts/pack-npm.mjs --version 0.7.0 --artifacts-dir ./dist-binaries
 *
 * Expected layout under --artifacts-dir (one file per target):
 *   aarch64-apple-darwin/statica
 *   x86_64-apple-darwin/statica
 *   x86_64-unknown-linux-gnu/statica
 *   aarch64-unknown-linux-gnu/statica
 *   x86_64-pc-windows-msvc/statica.exe
 *
 * Or flat names:
 *   statica-aarch64-apple-darwin
 *   statica-x86_64-apple-darwin
 *   …
 *   statica-x86_64-pc-windows-msvc.exe
 */

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const NPM_ROOT = path.join(ROOT, "npm", "@statica");

const TARGETS = [
  {
    triple: "aarch64-apple-darwin",
    pkg: "cli-darwin-arm64",
    binary: "statica",
  },
  {
    triple: "x86_64-apple-darwin",
    pkg: "cli-darwin-x64",
    binary: "statica",
  },
  {
    triple: "x86_64-unknown-linux-gnu",
    pkg: "cli-linux-x64-gnu",
    binary: "statica",
  },
  {
    triple: "aarch64-unknown-linux-gnu",
    pkg: "cli-linux-arm64-gnu",
    binary: "statica",
  },
  {
    triple: "x86_64-pc-windows-msvc",
    pkg: "cli-win32-x64",
    binary: "statica.exe",
  },
];

const PLATFORM_PKGS = TARGETS.map((t) => `@statica/${t.pkg}`);

function parseArgs(argv) {
  const out = {
    version: null,
    artifactsDir: null,
    skipBinaries: false,
    allowMissing: false,
    verify: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--version") out.version = argv[++i];
    else if (a === "--artifacts-dir") out.artifactsDir = argv[++i];
    else if (a === "--skip-binaries") out.skipBinaries = true;
    else if (a === "--allow-missing") out.allowMissing = true;
    else if (a === "--verify") out.verify = true;
    else if (a === "--help" || a === "-h") {
      console.log(
        `Usage: node scripts/pack-npm.mjs --version X.Y.Z [--artifacts-dir DIR] [--skip-binaries] [--allow-missing] [--verify]`,
      );
      process.exit(0);
    } else {
      console.error(`Unknown argument: ${a}`);
      process.exit(1);
    }
  }
  return out;
}

function readWorkspaceVersion() {
  const cargo = fs.readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
  const m = cargo.match(/\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error("Could not read workspace.package.version from Cargo.toml");
  return m[1];
}

function findBinary(artifactsDir, triple, binaryName) {
  const candidates = [
    path.join(artifactsDir, triple, binaryName),
    path.join(artifactsDir, triple, "statica"),
    path.join(artifactsDir, triple, "statica.exe"),
    path.join(artifactsDir, `statica-${triple}${binaryName.endsWith(".exe") ? ".exe" : ""}`),
    path.join(artifactsDir, binaryName === "statica.exe" ? `statica-${triple}.exe` : `statica-${triple}`),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c) && fs.statSync(c).isFile()) return c;
  }
  return null;
}

function writeJson(file, obj) {
  fs.writeFileSync(file, JSON.stringify(obj, null, 2) + "\n");
}

function syncPackageVersion(pkgDir, version, optionalDeps) {
  const pkgPath = path.join(pkgDir, "package.json");
  const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  pkg.version = version;
  if (optionalDeps) {
    pkg.optionalDependencies = Object.fromEntries(
      PLATFORM_PKGS.map((name) => [name, version]),
    );
  }
  writeJson(pkgPath, pkg);
}

function pack({ version, artifactsDir, skipBinaries, allowMissing }) {
  const ver = version ?? readWorkspaceVersion();
  console.log(`Packing npm packages at version ${ver}`);

  // Sync main + platform package.json versions
  syncPackageVersion(path.join(NPM_ROOT, "cli"), ver, true);
  for (const t of TARGETS) {
    syncPackageVersion(path.join(NPM_ROOT, t.pkg), ver, false);
  }

  // Ensure LICENSE is present
  const licenseSrc = path.join(ROOT, "LICENSE");
  for (const name of ["cli", ...TARGETS.map((t) => t.pkg)]) {
    const dest = path.join(NPM_ROOT, name, "LICENSE");
    fs.copyFileSync(licenseSrc, dest);
  }

  if (skipBinaries) {
    console.log("Skipped binary injection (--skip-binaries)");
    return;
  }

  if (!artifactsDir) {
    throw new Error("--artifacts-dir is required unless --skip-binaries is set");
  }
  const absArtifacts = path.resolve(artifactsDir);
  if (!fs.existsSync(absArtifacts)) {
    throw new Error(`Artifacts directory not found: ${absArtifacts}`);
  }

  let packed = 0;
  for (const t of TARGETS) {
    const src = findBinary(absArtifacts, t.triple, t.binary);
    if (!src) {
      const msg = `Missing binary for ${t.triple} (looked under ${absArtifacts}). Expected e.g. ${t.triple}/${t.binary}`;
      if (allowMissing) {
        console.warn(`  warn: ${msg}`);
        continue;
      }
      throw new Error(msg);
    }
    const binDir = path.join(NPM_ROOT, t.pkg, "bin");
    fs.mkdirSync(binDir, { recursive: true });
    const dest = path.join(binDir, t.binary);
    fs.copyFileSync(src, dest);
    fs.chmodSync(dest, 0o755);
    console.log(`  ${t.pkg}/bin/${t.binary} <- ${src}`);
    packed += 1;
  }

  if (packed === 0) {
    throw new Error("No platform binaries were packed");
  }

  console.log("Done.");
}

function verifyLocalSmoke() {
  execFileSync(process.execPath, [path.join(ROOT, "scripts", "verify-npm-cli.mjs")], {
    stdio: "inherit",
  });
}

const args = parseArgs(process.argv.slice(2));
try {
  pack(args);
  if (args.verify) {
    verifyLocalSmoke();
  }
} catch (err) {
  console.error(err.message || err);
  process.exit(1);
}
