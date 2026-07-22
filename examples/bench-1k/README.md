# bench-1k

Stress fixture: **1000** collection posts + home + blog index → **1002** pages.

```bash
# from repo root (release binary recommended)
statica build examples/bench-1k

# or after a local build
./target/release/statica build examples/bench-1k
```

Regenerate posts:

```bash
python3 - <<'PY'
import json
from pathlib import Path
root = Path("examples/bench-1k")
posts = [{
    "slug": f"post-{i:04d}",
    "headline": f"Post {i:04d}",
    "published_at": f"2026-{(i % 12) + 1:02d}-{(i % 28) + 1:02d}",
    "summary": f"Summary for benchmark post number {i}.",
    "html": f"<p>Body of post {i}. Lorem ipsum dolor sit amet.</p>",
} for i in range(1, 1001)]
(root / "content" / "posts.json").write_text(json.dumps(posts, indent=2) + "\n")
print(len(posts))
PY
```

## Results (Apple Silicon, release, 2026-07-17)

| Metric | Value |
| ------ | ----- |
| Pages | 1002 (1000 posts + home + blog) |
| Funnel JSON | ~244 KiB |
| Output `.dist` | ~3.9 MiB |
| Binary startup (`statica -v`) | ~0 ms wall |
| Cold build (empty `.dist`, `clean=true`) | ~180–210 ms wall / ~160–200 ms reported |
| Warm full rebuild | ~160–250 ms |
| Hot rebuild (`clean=false`) | ~90 ms wall / ~86 ms reported |
| Per page (cold, wall) | ~0.17–0.21 ms |
| Per page (cold, reported) | ~0.16–0.20 ms |
| Throughput (cold) | ~5k pages/s |
| Peak RSS (`time -l`) | ~12 MiB |

First-ever process launch after a cold binary load can be slower (~1.5 s once) while the OS maps the release binary; subsequent runs stay in the ~200 ms band.

Reported time is statica’s internal `BuildReport.duration_ms` (discover → emit). Wall time includes process startup and stderr logging.
