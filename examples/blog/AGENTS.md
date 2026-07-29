# AGENTS.md — examples/blog

Dogfood statica site. Canonical reference for authoring. Read [../../AGENTS.md](../../AGENTS.md) for full rules.

## Purpose

This directory demonstrates every major statica feature: Markdown funnel, collections, pagination, fragments, fonts, forms, i18n, RSS, sitemap. Integration tests in `crates/statica/tests/build_fixture.rs` build this fixture.

When unsure how to author something, **copy from here** before inventing patterns.

## Build

```bash
statica examples/blog
# or from this directory:
statica
```

Output: `.dist/` (gitignored). Tests use `dist-test/`.

## Layout

```
examples/blog/
├── statica.toml          # pagination, i18n, forms, rss, sitemap
├── index.html            # home page
├── site.css              # global styles
├── content/
│   ├── posts/            # Markdown files loaded by glob
│   └── i18n/en.json, pt.json
├── ui/                   # fragment templates
│   ├── post-card.html
│   ├── post-list.html
│   ├── button.html
│   └── …
├── posts/[slug]/index.html       # collection route
├── blog/[page]/index.html        # pagination route
├── [locale]/about/index.html     # i18n route
└── public/               # static assets
```

## Patterns to copy

### Funnel from Markdown glob

```html
<link rel="statica/data" href="../../content/posts/*.md" id="posts" />
```

Points at an explicit `.md` glob with YAML front matter — see `content/posts/*.md`.

### Collection page

[`posts/[slug]/index.html`](posts/[slug]/index.html):

- Page context roots such as `data`, `item`, `page`, and `i18n` are declared on `<html data-bind>` before use
- Data links in `<head>`
- `data-t="${item.headline}"` for scalar item text
- Fragment imports with relative paths from the page file

### Pagination page

[`blog/[page]/index.html`](blog/[page]/index.html):

- Same explicit canonical page context, usually `{page}` for pagination metadata
- `<slot id="post-list">` passes the pagination context into the fragment
- `${page.pagination.prev_href}`, `${page.pagination.next_href}`, `data-each="page.pagination.pages"` for nav

Pagination config in [`statica.toml`](statica.toml):

```toml
[[pagination]]
route = "blog/[page]"
page_size = 2
sort_by = "published_at"
sort_desc = true
index = true
```

### Fragment with scoped CSS

[`ui/post-card.html`](ui/post-card.html):

- `<template id="post-card" data-bind="{slug, headline, …}">`
- Modern nested CSS in `<style>`
- `${slug}` in attributes, `data-t="${headline}"` for scalar text

### i18n

- Template at `[locale]/about/index.html`
- Catalogs in `content/i18n/{locale}.json`
- `data-t="${i18n.about.title}"`-style paths on translatable elements
- The page binds `{i18n}` on `<html>` before using catalog values
- Config: `[i18n] enabled = true, locales = ["en", "pt"]`

## When editing this fixture

- Keep it minimal — one clear example per feature
- Run `cargo test -p statica builds_blog_fixture` after changes
- Update `docs/guide.md` if you introduce a new authoring pattern
- Do not add JavaScript frameworks or build tools — statica IS the build tool
