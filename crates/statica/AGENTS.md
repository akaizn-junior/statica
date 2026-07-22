# AGENTS.md — statica CLI

Rust CLI binary. Read [../../AGENTS.md](../../AGENTS.md) for project-wide context.

## Responsibility

The `statica` crate is the user-facing CLI:

- Resolve project root (walk up for `statica.toml`, honor `project` / `--project`)
- Load config from TOML, `.env`, `.dev.vars`, and CLI SPEC flags
- Map everything to `statica_core::BuildOptions`
- Watch, serve, scaffold (`new`), man page generation

Core pipeline code belongs in `statica-core`, not here.

## Module map

| Module | Purpose |
| ------ | ------- |
| `main.rs` | Entry, subcommand dispatch |
| `cli.rs` | clap definitions + long help text |
| `cli_config.rs` | SPEC flag parsing (`--rss 'title=Blog,limit=20'`) |
| `config.rs` | `statica.toml` load/map (~1300 lines, includes unit tests) |
| `env.rs` | `.env` / `.dev.vars` loading |
| `style.rs` | Terminal colors (owo-colors, TTY/`NO_COLOR`) |
| `cmd/build.rs` | Build command |
| `cmd/serve.rs` | Preview server (axum + tower-http) |
| `cmd/watch.rs` | File watcher + rebuild + serve |
| `cmd/new.rs` | Project scaffold |
| `build.rs` | Man page generation via clap_mangen |

## Conventions

### Config

- Config file constant: `CONFIG_FILE` = `"statica.toml"` in `config.rs`
- Serde structs: `#[serde(default, deny_unknown_fields)]`
- CLI SPEC strings override TOML; document new flags in clap help and `docs/guide.md`

### Errors

Use `anyhow::Result` with `.context("…")` for path and operation context. Map `statica_core::Error` at the boundary.

### Man pages

Regenerated on every `cargo build -p statica` into `docs/man/`. Update clap doc comments in `cli.rs` when changing CLI behavior — do not hand-edit `.1` files.

### Async

Only `serve` and `watch` use tokio/axum. Keep the rest synchronous.

### Tests

Unit tests co-located in `config.rs`, `env.rs`, `cmd/util.rs`. Test SPEC parsing and config mapping, not the pipeline (that's core's job).

### Adding a CLI flag

1. Add to clap in `cli.rs` with help text
2. Parse in `cli_config.rs` if SPEC-style
3. Map to `BuildOptions` field in `config.rs` or command handler
4. Update `docs/guide.md`
5. Rebuild to regenerate man pages
