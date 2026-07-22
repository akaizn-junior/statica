# examples/blog

Dogfood fixture for **statica** — Powered HTML.

```bash
# from repo root
statica build examples/blog
statica watch examples/blog

# or from inside the project (walks up for statica.toml)
cd examples/blog && statica
```

Demonstrates:

- Markdown funnel content (`content/posts/*.md` via `<script type="statica/data" src="…/posts">`)
- Home page with `$`-scoped button fragment
- Paginated listing (`blog/[page]/` → `blog/1/`, `blog/2/`, … via `[[pagination]]`)
- Collection pages (`posts/[slug]/`) from the posts directory
- Nested fragment with its own data (`related-posts`)
- Path aliases + Google Fonts (`@Google/…` via `<link rel="statica/font">`)
- Static forms (`<form statica>` + `[forms]` in `statica.toml`)
- i18n (`[locale]/about/` + `data-t` catalogs in `content/i18n/`)
- Sitemap + RSS (`site_url` + `[sitemap]` / `[rss]` in `statica.toml`)
- Public assets copied from `public/` (e.g. `site.css`)
