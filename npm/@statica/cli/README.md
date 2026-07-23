# @statica/cli

**statica** — **Powered HTML**

npm port of the Rust `statica` CLI — same binary as `cargo install statica-cli`.

Prebuilt binaries ship as **optional dependencies** (esbuild / Biome pattern). No postinstall scripts, no JS API, no exports.

## Install

```bash
npm i -D @statica/cli
```

Works with npm, pnpm, yarn, and bun. Do not omit optional dependencies.

## Usage

Add to `package.json`:

```json
{
  "scripts": {
    "build": "statica build .",
    "dev": "statica watch ."
  }
}
```

Or run directly:

```bash
npx statica build .
statica -v
statica watch
```

## Platforms

| npm package | Target |
| ----------- | ------ |
| `@statica/cli-darwin-arm64` | macOS Apple Silicon |
| `@statica/cli-darwin-x64` | macOS Intel |
| `@statica/cli-linux-x64-gnu` | Linux x64 (glibc) |
| `@statica/cli-linux-arm64-gnu` | Linux arm64 (glibc) |
| `@statica/cli-win32-x64` | Windows x64 |

Unsupported platform? Use Rust: `cargo install statica-cli --locked`.

## Library / embed

Programmatic builds: depend on Rust [`statica`](https://crates.io/crates/statica) (`statica::build`). A JS `@statica/core` port may come later.

## License

MIT
