# bench-5k-pages

**5000** funnel items, paginated **1 per page** → `blog/1/` … `blog/5000/` (+ `blog/` index + home) = **5002** HTML pages on disk.

```bash
cargo build -p statica --release
./target/release/statica build examples/bench-5k-pages
./target/release/statica serve examples/bench-5k-pages
# → http://127.0.0.1:4350/blog/
```

Approx (release, Apple Silicon): ~7 s cold build, ~50 MiB RSS, `.dist` generated under the example.

Nav uses first / prev / next / last only (full page-number lists are omitted when `total_pages > 200` to keep memory O(pages)).
