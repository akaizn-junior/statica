# statica

**Powered HTML**

statica turns valid HTML into a static site: fragments, local JSON funnels, collections, pagination, scoped components, and browser-ready CSS — then emits files.

> Always lowercase **statica**.

Full reference: [docs/guide.md](docs/guide.md) · Man pages: [docs/man/](docs/man/)

## Two cores

| Concept | Role |
| ------- | ---- |
| **Funnel** | Build-time local JSON via `<script type="statica/data" src id>` |
| **Pages** | Every `**/index.html` — folder path is the route (incl. `[slug]` / `[page]`) |

Flow: **Funnel → Pages → static HTML**

## Install

**Rust (crates.io):**

```bash
cargo install statica --locked
```

Library API: depend on [`statica-core`](https://crates.io/crates/statica-core) and call `statica_core::build`.

**JavaScript (npm):**

```bash
npm i -D @statica/cli
```

Same CLI binary as `cargo install statica`. Optional platform packages — no postinstall scripts.

```bash
npx statica build .
# package.json scripts: "build": "statica build ."
```

From this repo (dev):

```bash
cargo install --path crates/statica --force
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

Prefer the installed binary (`statica …`) over `cargo run`.

## CLI

```text
statica [PATH]              build (default)
statica build [PATH]        build
statica serve [PATH]        preview out_dir (axum + tower-http)
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
statica build --emit strip_data=false
statica build --pagination 'route=blog/[page],page_size=10,sort_desc=true,index=true'
statica watch --preview host=127.0.0.1,port=9000
```

### Man pages

Generated from clap on every `cargo build -p statica`:

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

[emit]
strip_data = true

[process]
enabled = false
css = true
js = true
images = true

[sitemap]
enabled = false
urls_per_file = 50000

[[pagination]]
route = "blog/[page]"
page_size = 10
sort_by = "published_at"
sort_desc = true
index = true

[rss]
enabled = false
limit = 50

[preview]
host = "0.0.0.0"
port = 4321
```

| Asset kind | Tool |
| ---------- | ---- |
| CSS | lightningcss (nesting, modern syntax → browser-ready; minify with `--process`) |
| JS | oxc |
| Images | oxipng + image |
| Fonts | copied as-is |

Inline `<style>` (pages + fragments) is always transformed. Linked `.css` under `asset_dirs` is transformed when `[process].css` is on.

## Authoring

```text
index.html                 → .dist/index.html
posts/[slug]/index.html    → .dist/posts/{item.slug}/index.html
blog/[page]/index.html     → .dist/blog/1/, blog/2/, …  ([[pagination]])
```

```html
<script type="statica/data" src="../content/posts.json" id="posts"></script>
<link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />
<slot id="post-card" data-bind="."></slot>
```

- Content → `<slot name="field">` (field must be declared in the fragment `data-bind`)
- Attributes → `${field}` (same — no magic vars; use `data-bind="{a, b}"` or `${prop.field}`)
- Collection: `<html data-bind="posts">` + `[slug]`
- Pagination: `<html data-bind="posts">` + `[page]` + `data-bind="items"` for the chunk

## Crate layout

- `crates/statica` — CLI (cwd/project resolve, config, SPECs, watch/serve, man pages)
- `crates/statica-core` — discover → funnel → bind → scope → emit
- `examples/blog` — dogfood fixture
- `docs/` — guide + man pages

## License

MIT
