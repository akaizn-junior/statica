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

- Home page with `$`-scoped button fragment
- Paginated listing (`blog/[page]/` → `blog/1/`, `blog/2/`, … via `[[pagination]]`)
- Collection pages (`posts/[slug]/`) from `content/posts.json`
- Nested fragment with its own data (`related-posts`)
- Sitemap + RSS (`site_url` + `[sitemap]` / `[rss]` in `statica.toml`)
