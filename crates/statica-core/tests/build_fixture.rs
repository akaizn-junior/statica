use std::path::PathBuf;

use statica_core::{build, BuildOptions};

#[test]
fn builds_blog_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/blog");
    let root = root.canonicalize().expect("examples/blog");
    let out = root.join("dist-test");
    let mut opts = BuildOptions::new(&root);
    opts.out_dir = out.clone();
    opts.clean = true;
    opts.i18n = statica_core::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };
    opts.forms = statica_core::FormsOptions {
        enabled: true,
        provider: statica_core::FormProvider::Formspree,
        endpoint: "https://formspree.io/f/{id}".into(),
        ids: [("contact".into(), "example".into())].into(),
    };
    opts.pagination = vec![statica_core::PaginationRule {
        route: "blog/[page]".into(),
        page_size: 2,
        limit: 0,
        offset: 0,
        sort_by: "published_at".into(),
        sort_desc: true,
        max_pages: 0,
        index: true,
    }];

    let report = build(&opts).expect("build");
    assert!(report.pages_written >= 10, "pages={}", report.pages_written);

    let listing = std::fs::read_to_string(out.join("blog/index.html")).unwrap();
    assert!(listing.contains("statica as SSG") || listing.contains("Typed funnels"));
    assert!(listing.contains(r#"href="/posts/statica-ssg/""#) || listing.contains("/posts/"));
    assert!(listing.contains("Page 1 of 3") || listing.contains(">1<"));

    let page3 = std::fs::read_to_string(out.join("blog/3/index.html")).unwrap();
    assert!(page3.contains("Hello world"));

    let page2 = std::fs::read_to_string(out.join("blog/2/index.html")).unwrap();
    assert!(page2.contains("/blog/1/") || page2.contains("/blog/3/"));

    let post = std::fs::read_to_string(out.join("posts/hello-world/index.html")).unwrap();
    assert!(post.contains("<title>Hello world</title>") || post.contains("Hello world"));
    assert!(post.contains("data-s=\"post-card-") || post.contains("data-s='post-card-"));

    let home = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(home.contains("Read the blog") || home.contains("class=\"btn\""));
    assert!(home.contains("fonts.googleapis.com"));

    let about_en = std::fs::read_to_string(out.join("en/about/index.html")).unwrap();
    assert!(about_en.contains("https://formspree.io/f/example"));
    assert!(about_en.contains("lang=\"en\""));

    let about_pt = std::fs::read_to_string(out.join("pt/about/index.html")).unwrap();
    assert!(about_pt.contains("HTML com superpoderes") || about_pt.contains(">Sobre<"));
    assert!(about_pt.contains("lang=\"pt\""));

    let _ = std::fs::remove_dir_all(out);
}

#[test]
fn builds_blog_from_markdown_content() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("content/posts")).unwrap();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::write(
        dir.join("content/posts/hello-world.md"),
        r#"---
slug: hello-world
headline: Hello world
published_at: 2026-07-01
summary: First post from markdown.
---

Build stamps this into **static HTML**.
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/posts/second-post.md"),
        r#"---
slug: second-post
headline: Second post
published_at: 2026-07-10
summary: Another markdown post.
---

More **content** here.
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="posts">
  <head>
    <script type="statica/data" src="../../content/posts" id="posts"></script>
    <title><slot name="headline"></slot></title>
  </head>
  <body>
    <h1><slot name="headline"></slot></h1>
    <div><slot name="html"></slot></div>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let post = std::fs::read_to_string(dir.join("dist/posts/hello-world/index.html")).unwrap();
    assert!(post.contains("<title>Hello world</title>") || post.contains("Hello world"));
    assert!(post.contains("<strong>static HTML</strong>"));

    let post2 = std::fs::read_to_string(dir.join("dist/posts/second-post/index.html")).unwrap();
    assert!(post2.contains("Second post"));
    assert!(post2.contains("<strong>content</strong>"));
}

#[test]
fn builds_blog_from_markdown_glob() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("content/posts")).unwrap();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::write(
        dir.join("content/posts/hello-world.md"),
        r#"---
slug: hello-world
headline: Hello world
published_at: 2026-07-01
summary: From glob.
---

Glob **works**.
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="posts">
  <head>
    <script type="statica/data" src="../../content/posts/*.md" id="posts"></script>
    <title><slot name="headline"></slot></title>
  </head>
  <body>
    <h1><slot name="headline"></slot></h1>
    <div><slot name="html"></slot></div>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let post = std::fs::read_to_string(dir.join("dist/posts/hello-world/index.html")).unwrap();
    assert!(post.contains("Hello world"));
    assert!(post.contains("<strong>works</strong>"));
}

#[test]
fn duplicate_slug_errors() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::write(
        dir.join("content.json"),
        r#"[{"slug":"a","headline":"A"},{"slug":"a","headline":"B"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="posts">
  <head>
    <script type="statica/data" src="../../content.json" id="posts"></script>
    <title><slot name="headline"></slot></title>
  </head>
  <body><h1><slot name="headline"></slot></h1></body>
</html>"#,
    )
    .unwrap();

    let opts = BuildOptions::new(&dir);
    let err = build(&opts).unwrap_err().to_string();
    assert!(err.contains("duplicate") || err.contains("Duplicate"), "{err}");
}

#[test]
fn parses_with_html5ever_not_regex() {
    let doc = statica_core::parse::parse_document(
        r#"<!doctype html><html><body><h1 id="x">Hi</h1></body></html>"#,
    )
    .unwrap();
    assert!(doc.doctype.is_some());
    let els = doc.find(|e| e.attr("id") == Some("x"));
    assert_eq!(els.len(), 1);
}

#[test]
fn select_slot_expands_to_options() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("countries.json"),
        r#"[
  {"value": "us", "label": "United States"},
  {"value": "pt", "label": "Portugal"}
]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/select-option.html"),
        r#"<template id="select-option" data-bind="{value, label}">
  <option value="${value}"><slot name="label"></slot></option>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head>
    <script type="statica/data" src="./countries.json" id="countries"></script>
  </head>
  <body>
    <link rel="statica/fragment" type="text/html" href="./ui/select-option.html" id="select-option" />
    <select name="country" required>
      <slot id="select-option" data-each="countries"></slot>
    </select>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains("name=\"country\"") && html.contains("required"));
    assert!(html.contains(r#"<option value="us""#) && html.contains("United States"));
    assert!(html.contains(r#"<option value="pt""#) && html.contains("Portugal"));
    assert!(!html.contains("<slot"));
}

#[test]
fn statica_form_wires_formspree_action() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <body>
    <form name="contact" statica>
      <input type="email" name="email" required />
      <button type="submit">Send</button>
    </form>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.forms = statica_core::FormsOptions {
        enabled: true,
        provider: statica_core::FormProvider::Formspree,
        endpoint: "https://formspree.io/f/{id}".into(),
        ids: [("contact".into(), "xyzabc".into())].into(),
    };
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains("https://formspree.io/f/xyzabc"));
    assert!(html.contains("method=\"POST\"") || html.contains("method='POST'"));
    assert!(!html.contains("statica"));
}

#[test]
fn font_link_expands_in_build() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head>
    <link rel="statica/font" href="@Google/?family=Outfit:wght@400;700&display=swap" />
  </head>
  <body><p>Hi</p></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains("fonts.googleapis.com/css2?family=Outfit:wght@400;700"));
    assert!(!html.contains("statica/font"));
}

#[test]
fn i18n_expands_locale_param_from_config() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/about")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(
        dir.join("content/i18n/en.json"),
        r#"{"title": "About us", "label": "Contact"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/i18n/pt.json"),
        r#"{"title": "Sobre nós", "label": "Contactar"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/about/index.html"),
        r#"<!doctype html>
<html lang="en">
  <head><title data-t="title">About</title></head>
  <body>
    <h1 data-t="title">About</h1>
    <span data-t="label">hello</span>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica_core::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };

    build(&opts).expect("build");

    let en = std::fs::read_to_string(dir.join("dist/en/about/index.html")).unwrap();
    assert!(en.contains("<title>About us</title>"));
    assert!(en.contains("lang=\"en\""));
    assert!(en.contains("<span>Contact</span>"));
    assert!(!en.contains("data-t"));

    let pt = std::fs::read_to_string(dir.join("dist/pt/about/index.html")).unwrap();
    assert!(pt.contains("<title>Sobre nós</title>"));
    assert!(pt.contains("lang=\"pt\""));
    assert!(pt.contains("<span>Contactar</span>"));

    assert!(!dir.join("dist/[locale]").exists());
}

#[test]
fn i18n_loads_locale_specific_funnel_data() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/posts/[slug]")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(
        dir.join("content/i18n/en.json"),
        r#"{"title": "Posts"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/i18n/pt.json"),
        r#"{"title": "Artigos"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/posts.en.json"),
        r#"[{"slug":"hello","headline":"Hello world"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/posts.pt.json"),
        r#"[{"slug":"ola","headline":"Olá mundo"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="posts">
  <head>
    <script type="statica/data" src="../../../content/posts.${locale}.json" id="posts"></script>
    <title data-t="title">Posts</title>
  </head>
  <body>
    <h1><slot name="headline"></slot></h1>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica_core::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };

    build(&opts).expect("build");

    let en = std::fs::read_to_string(dir.join("dist/en/posts/hello/index.html")).unwrap();
    assert!(en.contains("<title>Posts</title>"));
    assert!(en.contains("<h1>Hello world</h1>"));
    assert!(en.contains("lang=\"en\""));

    let pt = std::fs::read_to_string(dir.join("dist/pt/posts/ola/index.html")).unwrap();
    assert!(pt.contains("<title>Artigos</title>"));
    assert!(pt.contains("<h1>Olá mundo</h1>"));
    assert!(pt.contains("lang=\"pt\""));
}

#[test]
fn i18n_fragment_inherits_parent_locale_for_data_t() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/about")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(
        dir.join("content/i18n/en.json"),
        r#"{"cta": "Contact us"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/i18n/pt.json"),
        r#"{"cta": "Contacte-nos"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/button.html"),
        r#"<template id="button">
  <button type="button"><span data-t="cta">Contact</span></button>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/about/index.html"),
        r#"<!doctype html>
<html lang="en">
  <body>
    <link rel="statica/fragment" type="text/html" href="../../ui/button.html" id="button" />
    <slot id="button"></slot>
  </body>
</html>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <body>
    <link rel="statica/fragment" type="text/html" href="./ui/button.html" id="button" />
    <slot id="button"></slot>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica_core::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };

    build(&opts).expect("build");

    let en = std::fs::read_to_string(dir.join("dist/en/about/index.html")).unwrap();
    assert!(en.contains("Contact us"));
    assert!(!en.contains("data-t"));

    let pt = std::fs::read_to_string(dir.join("dist/pt/about/index.html")).unwrap();
    assert!(pt.contains("Contacte-nos"));
    assert!(!pt.contains("data-t"));

    let home = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(home.contains("Contact"));
    assert!(!home.contains("data-t"));
    assert!(!home.contains("Contact us"));
}

#[test]
fn i18n_pagination_chunks_once_for_shared_data() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/blog/[page]")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(
        dir.join("content/posts.json"),
        r#"[
  {"slug":"a","headline":"Alpha","published_at":"2026-07-03"},
  {"slug":"b","headline":"Beta","published_at":"2026-07-02"},
  {"slug":"c","headline":"Gamma","published_at":"2026-07-01"}
]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/i18n/en.json"),
        r#"{"blog_title": "Blog"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/i18n/pt.json"),
        r#"{"blog_title": "Blogue"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/blog/[page]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="posts">
  <head>
    <script type="statica/data" src="../../../content/posts.json" id="posts"></script>
    <title data-t="blog_title">Blog</title>
  </head>
  <body>
    <h1 data-t="blog_title">Blog</h1>
    <p>Page <slot name="page"></slot> of <slot name="total_pages"></slot></p>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica_core::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };
    opts.pagination = vec![statica_core::PaginationRule {
        route: "[locale]/blog/[page]".into(),
        page_size: 2,
        sort_by: "published_at".into(),
        sort_desc: true,
        ..Default::default()
    }];

    build(&opts).expect("build");

    let en_p1 = std::fs::read_to_string(dir.join("dist/en/blog/1/index.html")).unwrap();
    assert!(en_p1.contains("<title>Blog</title>"));
    assert!(en_p1.contains("Page 1 of 2"));

    let pt_p2 = std::fs::read_to_string(dir.join("dist/pt/blog/2/index.html")).unwrap();
    assert!(pt_p2.contains("<title>Blogue</title>"));
    assert!(pt_p2.contains("Page 2 of 2"));

    assert!(dir.join("dist/en/blog/2/index.html").exists());
    assert!(dir.join("dist/pt/blog/1/index.html").exists());
}

#[test]
fn minifies_final_html_output() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head>
    <title>  Minify test  </title>
    <style>
      body {
        margin: 0;
        & p { padding: 1rem; }
      }
    </style>
    <script>
      const value = 1;
      console.log(value);
    </script>
  </head>
  <body>
    <p>  Hello  </p>
  </body>
</html>
"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.minify = statica_core::MinifyOptions {
        enabled: true,
        ..statica_core::MinifyOptions::default()
    };

    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains("Hello"));
    assert!(html.contains("console"));
    assert!(
        html.len() < 350,
        "expected minified HTML, got {} bytes: {html}",
        html.len()
    );
}

fn tempfile_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "statica-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
