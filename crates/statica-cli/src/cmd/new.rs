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
        &root.join("ui/button.html"),
        r#"<template id="button">
  <style>
    .btn {
      background: #0f172a;
      color: #fff;
      border: 0;
      border-radius: 0.5rem;
      padding: 0.5rem 1rem;
      cursor: pointer;
    }
    .btn[data-pressed="true"] {
      background: #0369a1;
    }
  </style>
  <button class="btn" type="button"><slot>Click me</slot></button>
  <script type="module">
    document.querySelector(".btn")?.addEventListener("click", () => {
      document.querySelector(".btn").dataset.pressed = "true";
    });
  </script>
</template>
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
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>statica starter</title>
    <link rel="statica/fragment" type="text/html" href="./ui/button.html" id="button" />
    <style>
      :root {
        color-scheme: light;
        font-family:
          Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
          "Segoe UI", sans-serif;
        background: #f8fafc;
        color: #0f172a;
      }
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
      }
      main {
        width: min(100% - 2rem, 960px);
        padding: 4rem 0;
      }
      .hero {
        display: grid;
        gap: 2rem;
      }
      .eyebrow {
        color: #047857;
        font-size: 0.8rem;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
      }
      h1 {
        margin: 0.5rem 0 0;
        max-width: 720px;
        font-size: clamp(2.5rem, 8vw, 5.5rem);
        line-height: 0.95;
      }
      .lead {
        max-width: 620px;
        color: #475569;
        font-size: 1.125rem;
        line-height: 1.7;
      }
      .actions {
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
      }
      .link-card {
        border: 1px solid #e2e8f0;
        border-radius: 0.75rem;
        padding: 1rem;
        color: inherit;
        text-decoration: none;
        background: #fff;
      }
      .link-card strong {
        display: block;
        margin-bottom: 0.35rem;
      }
      .link-card span {
        color: #64748b;
      }
      .grid {
        display: grid;
        gap: 1rem;
      }
      .terminal {
        border-radius: 0.75rem;
        background: #0f172a;
        color: #d1fae5;
        padding: 1rem;
        overflow-x: auto;
      }
      code {
        font-family: "SFMono-Regular", Consolas, monospace;
      }
      @media (width >= 760px) {
        .hero {
          grid-template-columns: 1.1fr 0.9fr;
          align-items: center;
        }
        .grid {
          grid-template-columns: repeat(3, 1fr);
        }
      }
    </style>
  </head>
  <body>
    <main>
      <section class="hero">
        <div>
          <p class="eyebrow">statica starter</p>
          <h1>Build your site with just HTML.</h1>
          <p class="lead">
            Edit <code>index.html</code>, add content in <code>content/</code>,
            compose fragments from <code>ui/</code>, and ship plain static files.
          </p>
          <div class="actions">
            <slot id="button">Try scoped JS</slot>
            <a href="/blog/">Browse posts</a>
          </div>
        </div>
        <pre class="terminal"><code>statica
statica watch
open .website/index.html</code></pre>
      </section>

      <section class="grid" aria-label="Next steps">
        <a class="link-card" href="https://github.com/statica/statica/blob/main/docs/guide.md">
          <strong>Read the guide</strong>
          <span>Learn pages, fragments, data funnels, i18n, and deployable output.</span>
        </a>
        <a class="link-card" href="https://github.com/statica/statica">
          <strong>Open the docs</strong>
          <span>Find install notes, examples, and release details for statica.</span>
        </a>
        <a class="link-card" href="/blog/">
          <strong>See generated content</strong>
          <span>Two Markdown posts are already wired through a reusable fragment.</span>
        </a>
      </section>
    </main>
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
- Fragment scripts are scoped by default; `document.querySelector(...)` searches the mounted fragment instance.
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
        opts.out_dir = root.join(".website");
        build(&opts).unwrap();

        let blog = fs::read_to_string(root.join(".website/blog/index.html")).unwrap();
        assert!(blog.contains("Hello world"));
        assert!(blog.contains("Funnel to pages"));
        let home = fs::read_to_string(root.join(".website/index.html")).unwrap();
        assert!(home.contains("Read the guide"));
        assert!(home.contains("https://github.com/statica/statica/blob/main/docs/guide.md"));

        let _ = fs::remove_dir_all(root);
    }
}
