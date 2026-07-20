# statica documentation

**Powered HTML**

| Doc | Contents |
| --- | -------- |
| [../README.md](../README.md) | Overview, install (cargo / `@statica/cli`), CLI SPECs, authoring summary |
| [guide.md](guide.md) | Full authoring + config reference |
| [man/](man/) | Unix man pages (regenerated from clap on `cargo build -p statica`) |

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
