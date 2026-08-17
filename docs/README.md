# statica documentation

**Just HTML.** A blazingly fast static site generator for valid HTML.

| Doc | Contents |
| --- | -------- |
| [../README.md](../README.md) | Install, quick start, and getting a new site going |
| [guide.md](guide.md) | Direct authoring and config reference |
| [man/](man/) | Unix man pages (regenerated from clap on `cargo build -p statica-cli`) |

When CLI behavior changes, update clap help in `crates/statica-cli/src/cli.rs`, refresh the relevant README/guide text, then regenerate these man pages with `cargo build -p statica-cli`.

```bash
man docs/man/statica.1
man docs/man/statica-build.1
man docs/man/statica-serve.1
man docs/man/statica-watch.1
man docs/man/statica-new.1
```

Install system-wide (optional):

```bash
cp docs/man/*.1 /usr/local/share/man/man1/
```
