# statica guide

**statica** — **Powered HTML**

Write valid HTML. statica resolves fragments, runs local JS funnels, expands collections / pagination, scopes components, transforms CSS to browser-ready output, and emits a static site.

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
| **Funnel** | Build-time local JS value literals via `<script type="statica/data" src id>` |
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
- **Collection** — `[param]` + `<html data-bind="id">` or `data-bind="{…}">` over a JS array; one output per item using `item[param]` as the folder name.
- **Pagination** — `[page]` + `[[pagination]]`; declare chunk fields in `<html data-bind="{page, items, …}">`.

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
- **Final output** — when `[minify]` / `--minify` is on, every `.html`, `.css`, and `.js` under `out_dir` is minified (HTML pass also shrinks inline `<style>` and `<script>`).

## Data (funnel)

```html
<script type="statica/data" src="../content/posts.json" id="posts"></script>
```

Collection/pagination pages use the same bind rules as fragments:

- **Named** — `data-bind="posts"` → `${posts.headline}`, `<slot name="posts.headline">` (also selects the funnel id)
- **Destructure** — `data-bind="{headline, html}"` → `<slot name="headline">`, `${headline}` (funnel id from the lone `<script id>` on the page, or `data-bind="id"`)

Look up funnel arrays on static pages with `data-each="posts"` on fragment mounts.

### Collection page

```html
<!doctype html>
<html lang="en" data-bind="{headline, html}">
  <head>
    <script type="statica/data" src="../../content/posts.json" id="posts"></script>
    <title><slot name="headline"></slot></title>
  </head>
  <body>
    <h1><slot name="headline"></slot></h1>
  </body>
</html>
```

Or bind the whole item: `data-bind="posts"` with `<slot name="posts.headline">` / `${posts.slug}`.

### Paginated listing

Template at `blog/[page]/index.html`:

```html
<html lang="en" data-bind="{page, total_pages, items, prev_href, next_href, first_href, last_href, pages}">
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
<template id="post-card" data-bind="{slug, headline}">
  <h2>
    <a href="/posts/${slug}/"><slot name="headline"></slot></a>
  </h2>
</template>
```

- **Content** → `<slot name="field">` (never put slots in attributes).
- **Attributes** → `${field}` template strings against names declared in `data-bind`.
- **Props** → `<template data-bind="button">` binds only `button` (use `${button.variant}`). To expose fields directly, destructure: `data-bind="{variant, href}"` (nested objects allowed: `data-bind="{tag, summary: { foo, bar }}"`). No magic flattening — every `${…}` / named slot must be declared.
- Bad: `data-bind="button"` with `${variant}` → build fails (`variant` is not bound).
- Good: `data-bind="{variant, href}"` with `${variant}`, or `data-bind="button"` with `${button.variant}`.

Loops:

```html
<template id="post-list" data-bind="posts">
  <ul>
    <slot id="post-card" data-each="."></slot>
  </ul>
</template>
```

Fragments may import their own data and nest other fragments. Paths are relative to the fragment file. Nearest data scope wins.

### `<select>` with slotted options

Browsers reject `<slot>` inside `<select>`; statica’s **pre** pass rewrites those mounts before parsing, then expands them like any other fragment loop:

```html
<link rel="statica/fragment" type="text/html" href="./ui/select-option.html" id="select-option" />
<select name="${name}" required>
  <slot id="select-option" data-each="items"></slot>
</select>
```

```html
<template id="select-option" data-bind="{value, label}">
  <option value="${value}"><slot name="label"></slot></option>
</template>
```

Works inside `<optgroup>` too. Emitted HTML is a normal `<select>` with `<option>` children.

## Forms

Mark forms with the `statica` attribute. At build time statica wires `action` and `method="POST"` from `[forms]` in `statica.toml` — provider-agnostic HTML, mapping in config only. No client JS is injected; the browser submits normally.

```html
<form name="contact" statica>
  <input type="email" name="email" required />
  <button type="submit">Send</button>
</form>
```

```toml
[forms]
enabled = true
provider = "formspree"   # or "custom"
endpoint = "https://formspree.io/f/{id}"

[forms.ids]
contact = "xyzabc"
```

Lookup uses the form's `name` (or `id` if `name` is missing) as the key into `[forms.ids]`. Emitted HTML:

```html
<form name="contact" action="https://formspree.io/f/xyzabc" method="POST">
  …
</form>
```

For `provider = "custom"`, `endpoint` is the POST URL for every statica form (no `{id}`).

Build-time env (optional) — set in `statica.toml`, `.env`, or `.dev.vars` (process env always wins):

```toml
[env]
FORMS_CONTACT_ID = "xyzabc"
FORMS_ENDPOINT = "https://formspree.io/f/{id}"
```

Priority: `[env]` in config → `.env` → `.dev.vars`. Set `load_files = false` under `[env]` to skip dotenv files.

## Aliases

Path aliases are defined in `statica.toml`. The symbol defaults to `@`. Reference them with regular path syntax: `@Name/tail`. Local paths go in `[aliases.paths]`; URLs in `[aliases.urls]` only.

```toml
[aliases]
symbol = "@"

[aliases.urls]
Google = "https://fonts.googleapis.com/css2"

[aliases.paths]
fonts = "./assets/fonts"
static = "./static"
```

| Authoring | Resolves to |
|-----------|-------------|
| `@Google/?family=Outfit:wght@100..900&display=swap` | `https://fonts.googleapis.com/css2?family=Outfit:wght@100..900&display=swap` |
| `@fonts/outfit.css` | `./assets/fonts/outfit.css` |
| `@static/app.js` | `./static/app.js` |
| `./app.js` (no alias) | unchanged |

Aliases resolve at build time on `href`, `src`, `poster`, and `action` — including `statica/data` `src`, `statica/fragment` `href`, and emitted HTML.

## Fonts

Declare fonts with `<link rel="statica/font">`. statica expands them into regular HTML5 `<link rel="stylesheet">` tags (plus preconnect hints for Google Fonts when applicable).

```html
<link rel="statica/font" href="@Google/?family=Outfit:wght@100..900&display=swap" id="outfit-font" />
```

Emits:

```html
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="" />
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Outfit:wght@100..900&amp;display=swap" id="outfit-font" />
```

Local fonts — point at a CSS file with your own `@font-face` rules (plain path or alias):

```html
<link rel="statica/font" href="./fonts/outfit.css" />
<link rel="statica/font" href="@fonts/outfit.css" />
```

Both expand to `<link rel="stylesheet" href="…">`. Font files are copied via `asset_dirs` as usual.

## Web manifest

Enable with `manifest = true` in statica.toml or `--manifest` on the CLI.

statica scaffolds `public/manifest.webmanifest` when it is missing (edit that file directly), copies it via `asset_dirs`, and injects PWA head tags into every page:

```html
<link rel="manifest" href="/manifest.webmanifest" />
<meta name="theme-color" content="…" />
<link rel="apple-touch-icon" href="…" />
```

Tags are skipped when a page already declares them. Theme color and the apple touch icon are read from the manifest JSON.

```toml
manifest = true
```

```bash
statica build --manifest
```

## Internationalization (`[i18n]`)

Author **one page template** with a `[locale]` route segment. statica expands it once per entry in `[i18n].locales`:

```text
[locale]/about/index.html   → .dist/en/about/, .dist/pt/about/, …
[locale]/index.html         → .dist/en/, .dist/pt/, …
[locale]/posts/[slug]/…     → locale × collection items
```

Translation catalogs live in `content/i18n/{locale}.json`:

```toml
[i18n]
enabled = true
default = "en"
locales = ["en", "pt"]   # expanded for every [locale]/… template
dir = "content/i18n"
fallback = ""
```

Mark translatable text with `data-t` (inner text is the fallback when a key is missing):

```html
<!-- [locale]/about/index.html -->
<span data-t="label">hello</span>
<title data-t="page.title">About</title>
<a href="/${locale}/">home</a>
```

At build time statica replaces element content from the catalog, sets `<html lang="…">`, strips `data-t`, and supports `${locale}` in attributes. Use `${…}` only in **attributes** — not in text nodes.

### Locale-aware data

Funnel sources can include `${locale}` in `src` to load per-locale content files at build time:

```html
<!-- [locale]/posts/[slug]/index.html -->
<script type="statica/data" src="../../../content/posts.${locale}.json" id="posts"></script>
```

With `content/posts.en.json` and `content/posts.pt.json`, each locale expansion loads its own file. Array keys in `content/i18n/{locale}.json` still override funnel data for that locale (useful when content lives in the catalog instead of separate files).

Fragments inherit the parent page locale automatically — no extra attributes on `<slot>` or fragment templates. `data-t` in a fragment translates when the page is emitted under `[locale]/…`; on pages without a locale route, `data-t` is stripped and the inner fallback text is kept.

When the paginated/collection data source is shared across locales (no `${locale}` in its funnel `src`, no per-locale catalog array override), statica sorts and chunks **once**, then renders each locale — so 100 locales do not repeat sort/chunk work.

CLI:

```bash
statica build --i18n 'locales=en|pt,default=en'
statica build --no-i18n
```

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
statica build --pagination 'route=blog/[page],page_size=10,sort_desc=true,index=true'
statica build --i18n 'locales=en|pt,default=en'
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

[aliases]
symbol = "@"

[aliases.urls]
Google = "https://fonts.googleapis.com/css2"
# fonts = "./assets/fonts"

[aliases.paths]
# fonts = "./assets/fonts"

[process]
enabled = false
css = true                     # lightningcss: modern → browser-ready + minify (asset .css)
js = true                      # oxc
images = true                  # oxipng + image
fonts = false                  # copied as-is

[minify]
enabled = false
html = true                    # .html + inline <style>/<script> when css/js on
css = true                     # .css under out_dir
js = true                      # .js under out_dir

[sitemap]
enabled = false
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

[i18n]
enabled = false
default = "en"
locales = ["en"]
dir = "content/i18n"
fallback = ""
```

Sitemap pagination (many URLs) is separate from UI pagination: when URL count exceeds `urls_per_file`, statica writes part files and a sitemap index at `filename`.

## Crate layout

| Crate | Role |
| ----- | ---- |
| `crates/statica` | CLI: cwd/project resolve, `statica.toml`, SPEC flags, watch/serve/scaffold, man pages |
| `crates/statica-core` | Pipeline: discover → funnel → bind → scope → emit (+ feeds, pagination, assets) |
| `examples/blog` | Dogfood fixture (Markdown funnel, pagination, RSS, sitemap, fonts, forms, i18n) |
| `examples/bench-1k` | Stress fixture (~1k collection pages) |

## License

MIT
