# statica guide

**statica** — **Powered HTML**

Write valid HTML. statica resolves fragments, runs local JSON funnels, expands collections / pagination, scopes components, transforms CSS to browser-ready output, and emits a static site.

> Always lowercase **statica**.

## Install

| Ecosystem | CLI | Library |
| --------- | --- | ------- |
| Rust | `cargo install statica --locked` | `statica-core` → `statica_core::build` |
| JavaScript | `npm i -D @statica/cli` → `statica …` / `npx statica …` | Rust crate only (npm `@statica/core` later) |

`@statica/cli` is the npm port of the Rust CLI: prebuilt binary + launcher via optional platform packages. No postinstall downloads, no JS exports — use in scripts and `npx` only.

```bash
npm i -D @statica/cli
npx statica build .
```

## Concepts

| Concept | Role |
| ------- | ---- |
| **Funnel** | Build-time local JSON via `<script type="statica/data" src id>` |
| **Pages** | Every `folder/index.html` — folder path is the route |

Flow: **Funnel → Pages → static HTML** (default out dir: `.dist`)

## Project location

When you run the installed `statica` binary:

1. `PATH` (default `.`) is resolved against the **process cwd** (never relative to the binary).
2. statica walks **up** looking for `statica.toml`.
3. Site root = that config directory, or `project` / `--project` under it.

```bash
cd my-site && statica                 # cwd is the project
cd my-site/content && statica         # still finds ../statica.toml
statica /abs/path/to/site             # explicit start path
statica --project apps/docs           # monorepo: site under the toml dir
```

```toml
# statica.toml at the repo root
project = "apps/docs"   # empty = this directory
```

## Pages and routing

```text
index.html                 → .dist/index.html
about/index.html           → .dist/about/index.html
posts/[slug]/index.html    → .dist/posts/{item.slug}/index.html
blog/[page]/index.html     → .dist/blog/1/, blog/2/, …   (with [[pagination]])
```

- **Static page** — no `[param]` in the path; one output.
- **Collection** — `[param]` + `<html data-bind="id">` over a JSON array; one output per item using `item[param]` as the folder name.
- **Pagination** — `[page]` (or any single param) + `[[pagination]]` in config; array is chunked into page objects.

## CSS

statica transforms CSS with **lightningcss** so you can write modern CSS in `<style>` tags and linked stylesheets; the build emits browser-ready CSS. No PostCSS config.

Supported authoring (compiled when needed): nesting, custom media, range `@media`, modern colors, logical properties, and related features. Fragment styles are scoped (`[data-s="…"]`) after nesting is flattened.

```html
<style>
  .card {
    padding: 1rem;
    & h2 { font-weight: 600; }
  }
  @media (width >= 40rem) {
    .card { padding: 1.5rem; }
  }
</style>
```

- **`<style>`** in pages and fragments — always transformed on emit.
- **`.css` under `asset_dirs`** — transformed + minified when `[process].css` / `--process` is on; otherwise copied as-is.

## Data (funnel)

```html
<script type="statica/data" src="../content/posts.json" id="posts"></script>
```

Look up with `data-bind="posts"` / `data-each="posts"`. Types are inferred from JSON. Missing fields render empty.

### Collection page

```html
<!doctype html>
<html lang="en" data-bind="posts">
  <head>
    <script type="statica/data" src="../../content/posts.json" id="posts"></script>
    <title><slot name="headline"></slot></title>
  </head>
  <body>
    <h1><slot name="headline"></slot></h1>
  </body>
</html>
```

### Paginated listing

Template at `blog/[page]/index.html`:

```html
<html lang="en" data-bind="posts">
  <body>
    <p>Page <slot name="page"></slot> of <slot name="total_pages"></slot></p>
    <slot id="post-list" data-bind="items"></slot>
    <a href="${prev_href}">Previous</a>
    <a href="${next_href}">Next</a>
  </body>
</html>
```

```toml
[[pagination]]
route = "blog/[page]"
page_size = 10
limit = 0
offset = 0
sort_by = "published_at"
sort_desc = true
max_pages = 0
index = true            # also write page 1 at blog/
```

Page context fields: `items`, `page`, `page_number`, `total_pages`, `total_items`, `source_total`, `per_page`, `limit`, `offset`, `has_prev`, `has_next`, `prev`, `next`, `path`, `href`, `prev_href`, `next_href`, `first_href`, `last_href`, `pages` (array of `{ page, href, current }`).

Pipeline: **sort → offset → limit → chunk → max_pages**.

Do not put `[page]` and `[slug]` in the same folder tree (e.g. avoid both `posts/[slug]` and `posts/[page]`).

## Fragments

Three-part `id` contract:

```html
<link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />
<slot id="post-card" data-bind="."></slot>
```

```html
<template id="post-card" data-bind="post">
  <h2>
    <a href="/posts/${slug}/"><slot name="headline"></slot></a>
  </h2>
</template>
```

- **Content** → `<slot name="field">` (never put slots in attributes).
- **Attributes** → `${field}` template strings against the bind context.
- **Props** → `<template data-bind="button">` names the prop (any JS type). Object props are destructured (`const { variant, href } = button`), so `${variant}` and `${button.variant}` both work. `null` / missing → empty.

Loops:

```html
<template id="post-list" data-bind="posts">
  <ul>
    <slot id="post-card" data-each="."></slot>
  </ul>
</template>
```

Fragments may import their own data and nest other fragments. Paths are relative to the fragment file. Nearest data scope wins.

## Actions (`$`)

```html
<template id="button">
  <button class="btn" type="button"><slot>Go</slot></button>
  <script type="module">
    $.querySelector(".btn")?.addEventListener("click", () => {
      $.host.dataset.pressed = "true";
    });
  </script>
</template>
```

`$` scopes selectors to the fragment instance (`data-s="id-hash"`). Production builds inline the helper — no separate runtime file required in `.dist`.

## CLI

```bash
statica [PATH]              # build (default)
statica build [PATH]
statica serve [PATH]        # preview out_dir (no rebuild)
statica watch [PATH]        # watch + rebuild + serve
statica new <NAME>
statica -h / --help
statica -v / --version
```

Nested config tables use compact `key=value` **SPECs** (CLI wins over `statica.toml`):

```bash
statica build --rss 'title=Blog,limit=20,collections=posts'
statica build --sitemap 'filename=sitemap.xml,urls_per_file=50000'
statica build --process 'css=true,js=false,images=true'
statica build --emit strip_data=false
statica build --pagination 'route=blog/[page],page_size=10,sort_desc=true,index=true'
statica watch --preview host=127.0.0.1,port=9000
# short aliases: -p / --port, --host
```

- Bare `--rss` / `--sitemap` / `--process` enable that feature.
- `--no-rss` / `--no-sitemap` / `--no-process` disable.
- Inside `--rss`, list values use `|` (`collections=posts|notes`) because `,` separates keys.

Preview uses **axum** + **tower-http** `ServeDir` (indexes, gzip, index fallback). Default host `0.0.0.0` prints Local + Network URLs.

### Man pages

Regenerated from clap on every `cargo build -p statica` into `docs/man/`:

```bash
man docs/man/statica.1
man docs/man/statica-build.1
man docs/man/statica-serve.1
man docs/man/statica-watch.1
man docs/man/statica-new.1
```

## Config (`statica.toml`)

Optional. Missing file → defaults. Owned by the **CLI**; core only receives mapped [`BuildOptions`].

```toml
project = ""                   # site root relative to this file
out_dir = ".dist"
clean = true
copy_assets = true
asset_dirs = ["public", "assets", "static"]
ignore_dirs = [".dist", "dist", "target", ".git"]
site_url = ""                  # needed for sitemap / RSS

[emit]
strip_data = true
strip_fragments = true
strip_html_data_bind = true
dedupe_helpers = true
dedupe_styles = true

[process]
enabled = false
css = true                     # lightningcss: modern → browser-ready + minify (asset .css)
js = true                      # oxc
images = true                  # oxipng + image
fonts = false                  # copied as-is

[sitemap]
enabled = true
filename = "sitemap.xml"
urls_per_file = 50000          # over → sitemap-1.xml… + index

[[pagination]]
route = "blog/[page]"
page_size = 10
limit = 0
offset = 0
sort_by = "published_at"
sort_desc = true
max_pages = 0
index = true

[rss]
enabled = false
filename = "rss.xml"
title = ""
description = ""
language = "en"
limit = 50
title_field = "headline"
description_field = "summary"
date_field = "published_at"
collections = []               # empty = all collections

[preview]                      # alias: [watch]
host = "0.0.0.0"
port = 4321
debounce_ms = 80
poll_interval_secs = 2
```

Sitemap pagination (many URLs) is separate from UI pagination: when URL count exceeds `urls_per_file`, statica writes part files and a sitemap index at `filename`.

## Crate layout

| Crate | Role |
| ----- | ---- |
| `crates/statica` | CLI: cwd/project resolve, `statica.toml`, SPEC flags, watch/serve/scaffold, man pages |
| `crates/statica-core` | Pipeline: discover → funnel → bind → scope → emit (+ feeds, pagination, assets) |
| `examples/blog` | Dogfood fixture (pagination, RSS, sitemap) |
| `examples/bench-1k` | Stress fixture (~1k collection pages) |

## License

MIT
