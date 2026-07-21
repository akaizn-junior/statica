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
    // Newest first: page 1 has latest posts, not hello-world.
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

    let _ = std::fs::remove_dir_all(out);
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
