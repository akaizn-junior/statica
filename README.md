# statica

**Just HTML.** A blazingly fast static site generator that builds on just HTML

Full reference: [docs/guide.md](docs/guide.md) · Man pages: [docs/man/](docs/man/)

## Install

**Rust (crates.io):**

```bash
cargo install statica-cli --locked
```

**JavaScript (npm):**

```bash
npm i -D @statica/cli
```

**Homebrew:**

```bash
brew tap akaizn-junior/statica
brew install statica
```

Prebuilt macOS and Linux binaries from GitHub releases. See [homebrew/README.md](homebrew/README.md) for tap setup.

```bash
npx statica build .
# package.json scripts: "build": "statica build ."
```

From this repo (dev):

```bash
cargo install --path crates/statica-cli --force
```

## Quick start

```bash
statica new my-site
cd my-site
statica                 # build cwd (finds statica.toml walking up)
statica watch           # watch + serve
```

```bash
statica examples/blog
cd examples/blog/content && statica   # still finds ../statica.toml
statica -h
statica -v
```

## CLI

```text
statica [PATH]              build (default)
statica build [PATH]        build
statica serve [PATH]        preview out_dir with 404 fallback
statica watch [PATH]        watch + rebuild + serve
statica new <NAME>          scaffold
statica -h / --help
statica -v / --version
```

**Project location:** `PATH` (default `.`) → resolve against process **cwd** → walk up for `statica.toml` → site root is that dir, or `project` / `--project` under it.

Nested config tables use compact SPECs (CLI wins over the file):

```bash
statica build --rss 'title=Blog,limit=20,collections=posts'
statica build --sitemap 'filename=sitemap.xml,urls_per_file=50000'
statica build --process 'css=true,js=false,images=true'
statica build --minify 'html=true,css=true,js=true'
statica build --process --minify
statica build --pagination 'page_size=10,sort_desc=true,index=true'
statica build --i18n 'locales=en|pt,default=en'
statica build --render-mode serial
statica build --report-json report.json
statica watch --preview host=127.0.0.1,port=9000
```

### Man pages

```bash
man docs/man/statica.1
man docs/man/statica-build.1
man docs/man/statica-serve.1
man docs/man/statica-watch.1
man docs/man/statica-new.1
```

## Config (`statica.toml`)

Optional. Missing file → defaults. See [docs/guide.md](docs/guide.md) for the full reference.

```toml
project = ""                 # relative to this file; empty = here
out_dir = ".dist"
site_url = ""                # needed for sitemap / RSS

[process]
enabled = false
css = true
js = true
images = true

[sitemap]
enabled = false
urls_per_file = 50000

[[pagination]]
page_size = 10
sort_by = "published_at"
sort_desc = true
index = true

[rss]
enabled = false
limit = 50

[performance]
render_mode = "auto"
render_threads = 0

[preview]
host = "0.0.0.0"
port = 4321

[i18n]
enabled = false
locales = ["en"]
```

| Asset kind | Tool |
| ---------- | ---- |
| CSS | lightningcss (nesting, modern syntax → browser-ready; minify with `--process` or `--minify`) |
| JS | oxc |
| HTML | minify-html (final pass with `--minify`) |
| Images | oxipng + image |
| Fonts | copied as-is |

Inline `<style>` (pages + fragments) is always transformed. Linked `.css` under `asset_dirs` is transformed when `[process].css` is on. Enable `[minify]` / `--minify` for a final pass on emitted HTML, CSS, and JS (including inline `<style>` / `<script>`).

Set `[performance].render_mode` to `auto`, `serial`, or `parallel`. `serial` avoids rayon for page rendering; `parallel` always uses rayon; `auto` uses statica's default page-render profile. Use `render_threads = 0` for the default worker count, or set `--render-threads N` to cap parallel page rendering.

Use `--report-json [PATH]` to write the build report as JSON for benchmarks, CI, and integrations. Omit `PATH` or pass `-` to write JSON to stdout; pass a file path to update that file. In `watch`, the report is written after the initial build and each rebuild.

`statica watch` performs conservative incremental rebuilds. Direct edits to an existing page `index.html` re-emit only that page route when global post-processing is off. Changes to shared inputs such as data, fragments, assets, config-driven processing, deleted files, or minified builds fall back to a full rebuild.

## Authoring

statica source is valid HTML. It uses normal `<template>`, `<slot>`, and `<link>` elements as build-time authoring primitives, so keep them where HTML allows them.

```text
index.html                 → .dist/index.html
404/index.html             → .dist/404/index.html
posts/[slug]/index.html    → .dist/posts/{item.slug}/index.html
blog/[page]/index.html     → .dist/blog/1/, blog/2/, …  ([[pagination]])
```

If the site does not define `404.html` or `404/index.html`, statica writes a default `.dist/404/index.html`. Custom 404 pages are normal source pages and always win. `statica serve` returns the 404 page with HTTP status `404` for missing paths.

```html
<link rel="statica/data" href="../content/posts.json" id="posts" />
<link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />
<link rel="statica/font" href="@Google/?family=Outfit:wght@100..900&display=swap" />
<slot id="post-card"></slot>
```

- Scalar page text → `data-t="${item.field}"`, or literal text with `data-t="Plain text"`
- Attributes → `${item.slug}` / `${page.pagination.next_href}` / `${i18n.locale}`
- Fragment mount: `<slot id="fragment-id"></slot>` passes the current item/context
- Fragment loop: `<slot id="fragment-id" data-each="items"></slot>` passes each item
- Page `data-bind` declares canonical roots such as `{item}`, `{page}`, `{data}`, or `{i18n}` before use
- Data link IDs are directly available by `id`; they cannot be named `data`, `item`, `page`, or `i18n`
- Collection: linked data + `[slug]`; current record is `item`
- Pagination: linked data + `[page]`; page chunk is `page.pagination`
- Fragments never receive canonical page context; pass values through the mount context or link fragment-local data

Keep content funnels build-time. Production output should not fetch site data at runtime unless you are intentionally adding unrelated client behavior.

## Crate layout

- `crates/statica-cli` — CLI (cwd/project resolve, config, SPECs, watch/serve, man pages)
- `crates/statica` — discover → funnel → bind → scope → emit
- `examples/blog` — dogfood fixture
- `docs/` — guide + man pages

## License

MIT

## Author

(c) 2026 Simão Nziaka
