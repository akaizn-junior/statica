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

- Scaffold-style statica branding with the real logo asset copied from `public/statica-logo.png`
- daisyUI-friendly page structure and form controls, with small custom CSS in `public/site.css`
- Shared page shell in `layouts/base.html` mounted by route pages with `<link rel="statica/layout">`
- Markdown funnel content (`content/posts/*.md` via `<link rel="statica/data" href="…/posts/*.md">`)
- Home page with a default-scoped button fragment script
- Paginated listing (`blog/[page]/` → `blog/1/`, `blog/2/`, … via `[[pagination]]`)
- Collection pages (`posts/[slug]/`) from the posts Markdown glob
- Related posts composed in Markdown data and rendered with `data-each="item.related"`
- Path aliases + the Google Fonts recipe (`@Google/…` via `<link rel="statica/font">`)
- Static forms (`<form statica>` + `[forms]` in `statica.toml`)
- i18n (`[locale]/about/` + `data-t` catalogs in `content/i18n/`)
- Sitemap + RSS (`site_url` + `[sitemap]` / `[rss]` in `statica.toml`)
- Generated site search (`<input type="statica/search">` + `/search.json`)
- Public assets copied from `public/` (e.g. `site.css`, `statica-logo.png`)
