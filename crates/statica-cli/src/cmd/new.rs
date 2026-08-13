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
    eprintln!("  {}", style::dim(format!("statica build {name}")));
    Ok(())
}

fn scaffold(root: &Path, name: &str) -> Result<()> {
    if root.exists() {
        bail!("path already exists: {}", root.display());
    }

    for dir in [
        root.join("content").join("i18n"),
        root.join("[locale]"),
        root.join("public"),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }

    write(
        &root.join(CONFIG_FILE),
        &format!(
            "{}\n[i18n]\nenabled = true\ndefault = \"en\"\nlocales = [\"en\", \"fr\", \"pt\"]\ndir = \"content/i18n\"\n",
            StaticaConfig::default_toml()
        ),
    )?;
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("logo/statica-badge-green.png"),
        root.join("public/statica-logo.png"),
    )
    .with_context(|| format!("write {}", root.join("public/statica-logo.png").display()))?;

    write(
        &root.join("content/i18n/en.json"),
        r#"{
  "home": {
    "title": "statica starter",
    "guide": "Guide",
    "github": "Star on GitHub",
    "copyright": "© 2026 statica. Just HTML."
  }
}
"#,
    )?;
    write(
        &root.join("content/i18n/fr.json"),
        r#"{
  "home": {
    "title": "starter statica",
    "guide": "Guide",
    "github": "Suivre sur GitHub",
    "copyright": "© 2026 statica. Juste HTML."
  }
}
"#,
    )?;
    write(
        &root.join("content/i18n/pt.json"),
        r#"{
  "home": {
    "title": "starter statica",
    "guide": "Guia",
    "github": "Marcar no GitHub",
    "copyright": "© 2026 statica. Apenas HTML."
  }
}
"#,
    )?;
    write(
        &root.join("[locale]/index.html"),
        r##"<!doctype html>
<html lang="${i18n.locale}" data-bind="{i18n}">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <link rel="icon" href="../public/statica-logo.png" type="image/png" />
    <title data-t="${i18n.home.title}">statica starter</title>
    <style>
      :root {
        color-scheme: light;
        font-family:
          Karla, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
          "Segoe UI", sans-serif;
        background: #f8fafc;
        color: #0f172a;
      }
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        background-image:
          linear-gradient(color-mix(in oklab, currentColor 4%, transparent) 1px, transparent 1px),
          linear-gradient(90deg, color-mix(in oklab, currentColor 4%, transparent) 1px, transparent 1px);
        background-size: 28px 28px;
      }
      main {
        display: grid;
        justify-items: center;
        gap: 1.5rem;
        width: min(100% - 2rem, 34rem);
        text-align: center;
      }
      .logo {
        display: grid;
        place-items: center;
        gap: 1rem;
      }
      .logo-mark {
        width: clamp(6rem, 18vw, 9rem);
        height: clamp(6rem, 18vw, 9rem);
        object-fit: contain;
        filter: drop-shadow(0 1rem 1.8rem color-mix(in oklab, #089868 22%, transparent));
      }
      h1 {
        margin: 0;
        font-size: clamp(2rem, 8vw, 4.5rem);
        line-height: 1;
        letter-spacing: -0.06em;
        font-weight: 950;
      }
      nav {
        display: flex;
        flex-wrap: wrap;
        justify-content: center;
        gap: 0.75rem;
      }
      a {
        color: #089868;
        font-weight: 700;
        text-decoration-thickness: 0.08em;
        text-underline-offset: 0.25em;
      }
      .locales {
        gap: 0.5rem;
        font-size: 0.9rem;
      }
      footer {
        color: #64748b;
        font-size: 0.9rem;
      }
    </style>
  </head>
  <body>
    <main>
      <div class="logo" aria-label="statica">
        <img class="logo-mark" src="../public/statica-logo.png" alt="statica logo" />
        <h1 data-t="${i18n.home.title}">statica starter</h1>
      </div>

      <nav aria-label="Project links">
        <a href="https://github.com/akaizn-junior/statica/blob/main/docs/guide.md" data-t="${i18n.home.guide}">Guide</a>
        <a href="https://github.com/akaizn-junior/statica" data-t="${i18n.home.github}">Star on GitHub</a>
      </nav>

      <nav class="locales" aria-label="Languages">
        <a href="../en/">English</a>
        <a href="../fr/">Français</a>
        <a href="../pt/">Português</a>
      </nav>

      <footer data-t="${i18n.home.copyright}">© 2026 statica. Just HTML.</footer>
    </main>
  </body>
</html>
"##,
    )?;
    write(
        &root.join("README.md"),
        &format!(
            r#"# {name}

**Just HTML.** A blazingly fast static site generator that builds on just HTML

Install the CLI with Rust (`cargo install statica-cli --locked`) or npm (`npm i -D @statica/cli`), then:

```bash
statica
statica build
```

Settings live in `statica.toml` (optional; defaults apply if missing).

- Pages are every `**/index.html` (folder = route).
- `[locale]/index.html` emits localized pages for English, French, and Portuguese.
- Translation catalogs live in `content/i18n/{{locale}}.json`.
- Pages bind canonical roots such as `{{i18n}}` before use.
- Scalar text goes in `data-t="${{i18n.home.title}}"`.
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
    use statica::{build, BuildOptions, I18nOptions};

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
        opts.i18n = I18nOptions {
            enabled: true,
            default_locale: "en".into(),
            locales: vec!["en".into(), "fr".into(), "pt".into()],
            dir: "content/i18n".into(),
            ..Default::default()
        };
        build(&opts).unwrap();

        let home = fs::read_to_string(root.join(".website/en/index.html")).unwrap();
        assert!(home.contains("statica starter"));
        assert!(home.contains("../public/statica-logo.png"));
        assert!(home.contains(r#"<link rel="icon" href="../public/statica-logo.png" type="image/png""#));
        assert!(home.contains("https://github.com/akaizn-junior/statica/blob/main/docs/guide.md"));
        assert!(home.contains("Star on GitHub"));
        assert!(root.join(".website/public/statica-logo.png").exists());
        let fr = fs::read_to_string(root.join(".website/fr/index.html")).unwrap();
        assert!(fr.contains("starter statica"));
        let pt = fs::read_to_string(root.join(".website/pt/index.html")).unwrap();
        assert!(pt.contains("Apenas HTML"));

        let _ = fs::remove_dir_all(root);
    }
}
