# examples/blog

Dogfood fixture for **statica**. **Just HTML.** A blazingly fast static site generator that builds on just HTML

```bash
# from repo root
statica build examples/blog
statica examples/blog

# or from inside the project (builds, watches, and serves)
cd examples/blog && statica
```

Demonstrates:

- Markdown funnel content (`content/posts/*.md` via `<link rel="statica/data" href="…/posts/*.md">`)
- Home page with a default-scoped button fragment script
- Paginated listing (`blog/[page]/` → `blog/1/`, `blog/2/`, … via `[[pagination]]`)
- Collection pages (`posts/[slug]/`) from the posts Markdown glob
- Related posts composed in Markdown data and rendered with `data-each="item.related"`
- Path aliases + the Google Fonts recipe (`@Google/…` via `<link rel="statica/font">`)
- Static forms (`<form statica>` + `[forms]` in `statica.toml`)
- i18n (`[locale]/about/` + `data-t` catalogs in `content/i18n/`)
- Sitemap + RSS (`site_url` + `[sitemap]` / `[rss]` in `statica.toml`)
- Public assets copied from `public/` (e.g. `site.css`)
