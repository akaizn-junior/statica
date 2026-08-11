# DOME

**Just HTML.** A blazingly fast static site generator that builds on just HTML

Install the CLI with Rust (`cargo install statica-cli --locked`) or npm (`npm i -D @statica/cli`), then:

```bash
statica
statica watch
```

Settings live in `statica.toml` (optional; defaults apply if missing).

- Pages are every `**/index.html` (folder = route).
- Data via `<link rel="statica/data" href id>`.
- Fragments via `<link rel="statica/fragment" href id>` + `<template id>` + `<slot id>`.
- Pages bind canonical roots such as `{item}`, `{data}`, `{page}`, or `{i18n}` before use.
- Attributes and scalar text use dotted paths like `${item.slug}`; scalar text goes in `data-t="${item.headline}"`.
