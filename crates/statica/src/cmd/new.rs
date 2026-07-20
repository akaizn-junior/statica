use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::{StaticaConfig, CONFIG_FILE};
use crate::style;

pub fn run(name: &str) -> Result<()> {
    let root = PathBuf::from(name);
    if root.exists() {
        bail!("path already exists: {}", root.display());
    }

    for dir in [
        root.join("content"),
        root.join("ui"),
        root.join("blog"),
        root.join("posts").join("[slug]"),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }

    write(&root.join(CONFIG_FILE), &StaticaConfig::default_toml())?;

    write(
        &root.join("content/posts.json"),
        r#"[
  {
    "slug": "hello-world",
    "headline": "Hello world",
    "published_at": "2026-07-01",
    "summary": "First post from the funnel.",
    "html": "<p>Build stamps this into static HTML.</p>"
  },
  {
    "slug": "funnel-to-pages",
    "headline": "Funnel to pages",
    "published_at": "2026-07-10",
    "summary": "Item count drives page count.",
    "html": "<p>Two posts in → two folders out.</p>"
  }
]
"#,
    )?;
    write(
        &root.join("ui/post-card.html"),
        r#"<template id="post-card" data-bind="{slug, headline, summary}">
  <style>
    .card { border-top: 1px solid #e2e8f0; padding: 1rem 0; }
    .card__title { font-weight: 600; }
    .card__title a { color: inherit; text-decoration: none; }
  </style>
  <li class="card">
    <h2 class="card__title">
      <a href="/posts/${slug}/"><slot name="headline"></slot></a>
    </h2>
    <p><slot name="summary"></slot></p>
  </li>
</template>
"#,
    )?;
    write(
        &root.join("ui/post-list.html"),
        r#"<link rel="statica/fragment" type="text/html" href="./post-card.html" id="post-card" />
<template id="post-list" data-bind="posts">
  <ul class="posts">
    <slot id="post-card" data-each="."></slot>
  </ul>
</template>
"#,
    )?;
    write(
        &root.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>statica site</title>
  </head>
  <body>
    <h1>statica</h1>
    <p>Funnel → pages.</p>
    <p><a href="/blog/">Browse posts</a></p>
  </body>
</html>
"#,
    )?;
    write(
        &root.join("blog/index.html"),
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Blog</title>
    <link rel="statica/fragment" type="text/html" href="../ui/post-list.html" id="post-list" />
    <link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />
    <script type="statica/data" src="../content/posts.json" id="posts"></script>
  </head>
  <body>
    <h1>All posts</h1>
    <slot id="post-list" data-bind="posts"></slot>
  </body>
</html>
"#,
    )?;
    write(
        &root.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="posts">
  <head>
    <meta charset="utf-8" />
    <title><slot name="headline"></slot></title>
    <script type="statica/data" src="../../content/posts.json" id="posts"></script>
  </head>
  <body>
    <article>
      <h1><slot name="headline"></slot></h1>
      <time><slot name="published_at"></slot></time>
      <p><slot name="summary"></slot></p>
      <div><slot name="html"></slot></div>
    </article>
    <p><a href="/blog/">← All posts</a></p>
  </body>
</html>
"#,
    )?;
    write(
        &root.join("README.md"),
        &format!(
            r#"# {name}

A **statica** site — Powered HTML.

Install the CLI with Rust (`cargo install statica --locked`) or npm (`npm i -D @statica/cli`), then:

```bash
statica
statica watch
```

Settings live in `statica.toml` (optional; defaults apply if missing).

- Pages are every `**/index.html` (folder = route).
- Data via `<script type="statica/data" src id>`.
- Fragments via `<link rel="statica/fragment" href id>` + `<template id>` + `<slot id>`.
- Attributes use `${{field}}` declared via fragment `data-bind` (`name` or `{{a, b}}`); content uses `<slot name="field">`.
"#
        ),
    )?;

    eprintln!(
        "{} {}",
        style::success("created"),
        style::bold(root.display().to_string())
    );
    eprintln!("  {}", style::dim(format!("cd {name} && statica")));
    eprintln!("  {}", style::dim(format!("statica watch {name}")));
    Ok(())
}

fn write(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}
