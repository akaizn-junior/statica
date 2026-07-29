# bench-50k-pages

**50000** funnel items, paginated **1 per page** → `blog/1/` … `blog/50000/` (+ `blog/` index + home) = **50002** HTML pages on disk.

```bash
statica build examples/bench-50k-pages
statica serve examples/bench-50k-pages
# → http://127.0.0.1:4350/blog/
```

Approx (release, Apple Silicon): run locally; `.dist` generated under the example.

Nav uses first / prev / next / last only (full page-number lists are omitted when `total_pages > 200` to keep memory O(pages)).
