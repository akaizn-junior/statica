use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::{StaticaConfig, CONFIG_FILE};
use crate::style;

pub fn run(name: &str) -> Result<()> {
    let root = PathBuf::from(name);
    scaffold(&root, name)?;

    eprintln!(
        "{} {}",
        style::success("created"),
        style::bold(root.display().to_string())
    );
    eprintln!("  {}", style::dim(format!("cd {name} && statica")));
    eprintln!("  {}", style::dim(format!("statica watch {name}")));
    Ok(())
}

fn scaffold(root: &Path, name: &str) -> Result<()> {
    if root.exists() {
        bail!("path already exists: {}", root.display());
    }

    for dir in [
        root.join("content").join("posts"),
        root.join("ui"),
        root.join("blog"),
        root.join("posts").join("[slug]"),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }

    write(&root.join(CONFIG_FILE), &StaticaConfig::default_toml())?;

    write(
        &root.join("content/posts/hello-world.md"),
        r#"---
slug: hello-world
headline: Hello world
published_at: 2026-07-01
summary: First post from the funnel.
---

Build stamps this into static HTML.
"#,
    )?;
    write(
        &root.join("content/posts/funnel-to-pages.md"),
        r#"---
slug: funnel-to-pages
headline: Funnel to pages
published_at: 2026-07-10
summary: Item count drives page count.
---

Two posts in → two folders out.
"#,
    )?;
    write(
        &root.join("ui/post-card.html"),
        r#"<template id="post-card" data-bind="post">
  <style>
    .card { border-top: 1px solid #e2e8f0; padding: 1rem 0; }
    .card__title { font-weight: 600; }
    .card__title a { color: inherit; text-decoration: none; }
  </style>
  <li class="card">
    <h2 class="card__title">
      <a href="/posts/${post.slug}/" data-t="${post.headline}">Post</a>
    </h2>
    <p data-t="${post.summary}"></p>
  </li>
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
    <link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />
    <link rel="statica/data" href="../content/posts/*.md" id="posts" />
  </head>
  <body>
    <h1>All posts</h1>
    <ul class="posts">
      <slot id="post-card" data-each="posts"></slot>
    </ul>
  </body>
</html>
"#,
    )?;
    write(
        &root.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <meta charset="utf-8" />
    <title data-t="${item.headline}">Post</title>
    <link rel="statica/data" href="../../content/posts/*.md" id="posts" />
  </head>
  <body>
    <article>
      <h1 data-t="${item.headline}">Post</h1>
      <time data-t="${item.published_at}"></time>
      <p data-t="${item.summary}"></p>
      <div><slot name="item.html"></slot></div>
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

**Just HTML.** A blazingly fast static site generator that builds on just HTML

Install the CLI with Rust (`cargo install statica-cli --locked`) or npm (`npm i -D @statica/cli`), then:

```bash
statica
statica watch
```

Settings live in `statica.toml` (optional; defaults apply if missing).

- Pages are every `**/index.html` (folder = route).
- Data via `<link rel="statica/data" href id>`.
- Fragments via `<link rel="statica/fragment" href id>` + `<template id>` + `<slot id>`.
- Pages bind canonical roots such as `{{item}}`, `{{data}}`, `{{page}}`, or `{{i18n}}` before use.
- Attributes and scalar text use dotted paths like `${{item.slug}}`; scalar text goes in `data-t="${{item.headline}}"`.
"#
        ),
    )?;

    Ok(())
}

fn write(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use statica::{build, BuildOptions};

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "statica-new-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn scaffold_builds_without_authoring_errors() {
        let root = temp_dir();
        scaffold(&root, "starter").unwrap();

        let mut opts = BuildOptions::new(&root);
        opts.out_dir = root.join(".dist");
        build(&opts).unwrap();

        let blog = fs::read_to_string(root.join(".dist/blog/index.html")).unwrap();
        assert!(blog.contains("Hello world"));
        assert!(blog.contains("Funnel to pages"));

        let _ = fs::remove_dir_all(root);
    }
}
