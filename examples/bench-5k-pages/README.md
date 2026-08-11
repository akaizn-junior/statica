# bench-5k-pages

**5000** funnel items, paginated **10 per page** → `blog/1/` … `blog/500/`, plus nested readable post pages at `blog/[page]/[slug]/` (+ `blog/` index + home) = **5502** HTML pages on disk.

```bash
statica build examples/bench-5k-pages
statica serve examples/bench-5k-pages
# → http://127.0.0.1:4350/blog/
```

Approx (release, Apple Silicon): ~7 s cold build, ~50 MiB RSS, `.website` generated under the example.

The pages use the shared scaffold-style statica logo from `public/statica-logo.png`; `copy_assets = true` keeps the visual fixture self-contained.

Nav uses first / prev / next / last only (full page-number lists are omitted when `total_pages > 200` to keep memory O(pages)).
