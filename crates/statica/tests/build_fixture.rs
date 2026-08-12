use std::path::PathBuf;

use statica::{build, rebuild_paths, BuildOptions, MinifyOptions};

fn add_google_font_alias(opts: &mut BuildOptions) {
    opts.aliases.urls.insert(
        "Google".into(),
        statica::UrlAlias::new("https://fonts.googleapis.com/css2"),
    );
}

#[test]
fn builds_blog_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/blog");
    let root = root.canonicalize().expect("examples/blog");
    let out = root.join("dist-test");
    let mut opts = BuildOptions::new(&root);
    opts.out_dir = out.clone();
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };
    opts.forms = statica::FormsOptions {
        enabled: true,
        provider: statica::FormProvider::Formspree,
        endpoint: "https://formspree.io/f/{id}".into(),
        ids: [("contact".into(), "example".into())].into(),
    };
    opts.manifest = true;
    opts.pagination = vec![statica::PaginationRule {
        page_size: 2,
        sort_by: "published_at".into(),
        sort_desc: true,
        index: true,
        ..Default::default()
    }];
    add_google_font_alias(&mut opts);

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
    assert!(
        post.contains("<title>Hello world</title>") || post.contains("Hello world"),
        "{post}"
    );
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
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content/posts/*.md" id="posts" />
    <title>Post</title>
  </head>
  <body>
    <h1><slot name="item.headline"></slot></h1>
    <div><slot name="item.html"></slot></div>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let post = std::fs::read_to_string(dir.join("dist/posts/hello-world/index.html")).unwrap();
    assert!(
        post.contains("<title>Hello world</title>") || post.contains("Hello world"),
        "{post}"
    );
    assert!(post.contains("<strong>static HTML</strong>"));

    let post2 = std::fs::read_to_string(dir.join("dist/posts/second-post/index.html")).unwrap();
    assert!(post2.contains("Second post"));
    assert!(post2.contains("<strong>content</strong>"));
}

#[test]
fn rebuild_paths_reemits_only_changed_static_page() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("about")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><html><body><h1>Home v1</h1></body></html>",
    )
    .unwrap();
    std::fs::write(
        dir.join("about/index.html"),
        "<!doctype html><html><body><h1>About v1</h1></body></html>",
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    build(&opts).expect("initial build");

    std::fs::write(
        dir.join("about/index.html"),
        "<!doctype html><html><body><h1>About v2</h1></body></html>",
    )
    .unwrap();
    opts.clean = false;
    let report = rebuild_paths(&opts, &[dir.join("about/index.html")]).expect("rebuild");

    assert_eq!(report.pages_written, 1);
    assert_eq!(report.routes.len(), 1);
    assert_eq!(report.routes[0].route, "about");
    let home = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    let about = std::fs::read_to_string(dir.join("dist/about/index.html")).unwrap();
    assert!(home.contains("Home v1"));
    assert!(about.contains("About v2"));
}

#[test]
fn rebuild_paths_uses_full_build_for_shared_data_changes() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("content")).unwrap();
    std::fs::write(dir.join("content/site.json"), r#"{"headline":"First"}"#).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html data-bind="{data}">
  <head><link rel="statica/data" href="content/site.json" id="site" /></head>
  <body><h1 data-t="${data.site.headline}">Fallback</h1></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    build(&opts).expect("initial build");

    std::fs::write(dir.join("content/site.json"), r#"{"headline":"Second"}"#).unwrap();
    opts.clean = false;
    let report = rebuild_paths(&opts, &[dir.join("content/site.json")]).expect("rebuild");

    assert_eq!(report.pages_written, 1);
    assert_eq!(report.routes.len(), 1);
    let home = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(home.contains("Second"));
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
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content/posts/*.md" id="posts" />
    <title>Post</title>
  </head>
  <body>
    <h1><slot name="item.headline"></slot></h1>
    <div><slot name="item.html"></slot></div>
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
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content.json" id="posts" />
    <title>Post</title>
  </head>
  <body><h1><slot name="item.headline"></slot></h1></body>
</html>"#,
    )
    .unwrap();

    let opts = BuildOptions::new(&dir);
    let err = build(&opts).unwrap_err().to_string();
    assert!(
        err.contains("duplicate") || err.contains("Duplicate"),
        "{err}"
    );
}

#[test]
fn page_data_bind_can_narrow_to_item_context() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::write(
        dir.join("content.json"),
        r#"[{"slug":"a","headline":"Hello","html":"<p>Hi</p>"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content.json" id="posts" />
  </head>
  <body>
    <h1><slot name="item.headline"></slot></h1>
    <div><slot name="item.html"></slot></div>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/posts/a/index.html")).unwrap();
    assert!(html.contains("<h1>Hello</h1>"));
    assert!(html.contains("<p>Hi</p>"));
}

#[test]
fn fragment_mount_receives_current_item_without_data_bind() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::create_dir_all(dir.join("content")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("content.json"),
        r#"[{"slug":"a","headline":"Hello"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/post-card.html"),
        r#"<template id="post-card" data-bind="{slug, headline}">
  <article><a href="/posts/${slug}/"><slot name="headline"></slot></a></article>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content.json" id="posts" />
    <link rel="statica/fragment" type="text/html" href="../../ui/post-card.html" id="post-card" />
  </head>
  <body><slot id="post-card"></slot></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/posts/a/index.html")).unwrap();
    assert!(html.contains(r#"href="/posts/a/""#));
    assert!(html.contains(">Hello</a>"));
}

#[test]
fn looped_fragments_keep_css_and_script_scoped_per_instance() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("items.json"),
        r#"[{"label":"One"},{"label":"Two"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/card.html"),
        r#"<template id="card" data-bind="{label}">
  <style>
    #title { letter-spacing: 0; }
    .card { color: red; }
    .card strong { font-weight: 700; }
  </style>
  <article class="card">
    <strong id="title" data-t="${label}">Label</strong>
  </article>
  <script type="module">
    document.querySelector(".card").setAttribute("data-ready", "true");
    document.getElementById("title").setAttribute("data-title-ready", "true");
  </script>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head>
    <link rel="statica/data" href="./items.json" id="items" />
    <link rel="statica/fragment" type="text/html" href="./ui/card.html" id="card" />
  </head>
  <body><slot id="card" data-each="items"></slot></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.minify = MinifyOptions {
        enabled: true,
        ..MinifyOptions::default()
    };
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert_eq!(html.matches("<article").count(), 2, "{html}");
    assert_eq!(html.matches("<style>").count(), 1, "{html}");
    assert_eq!(html.matches("data-s-scope=").count(), 2, "{html}");
    assert_eq!(html.matches("run: runScoped").count(), 1, "{html}");
    assert_eq!(html.matches("function (document)").count(), 2, "{html}");
    assert_eq!(
        html.matches("__statica.run(document.currentScript").count(),
        2,
        "{html}"
    );
    assert_eq!(
        html.matches("</article><script data-s-scope=").count(),
        2,
        "{html}"
    );
    assert!(
        !html.contains("<script>const button=document.querySelector"),
        "{html}"
    );
    assert!(
        html.contains(".card[data-s=\"card-") || html.contains(".card[data-s=card-"),
        "{html}"
    );
    assert!(
        html.contains("#title[data-s=\"card-") || html.contains("#title[data-s=card-"),
        "{html}"
    );
    assert!(!html.contains("data-s-id"), "{html}");
    assert!(html.contains("var root = host || document;"), "{html}");
}

#[test]
fn data_bind_on_fragment_mount_errors() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("content.json"),
        r#"[{"slug":"a","headline":"Hello"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/post-card.html"),
        r#"<template id="post-card" data-bind="{slug, headline}">
  <article><slot name="headline"></slot></article>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content.json" id="posts" />
    <link rel="statica/fragment" type="text/html" href="../../ui/post-card.html" id="post-card" />
  </head>
  <body><slot id="post-card" data-bind="."></slot></body>
</html>"#,
    )
    .unwrap();

    let opts = BuildOptions::new(&dir);
    let err = build(&opts).unwrap_err().to_string();
    assert!(err.contains("data-bind is only valid on <html> and fragment <template>"));
}

#[test]
fn page_undeclared_bind_field_errors() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::write(
        dir.join("content.json"),
        r#"[{"slug":"a","headline":"A","summary":"S"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content.json" id="posts" />
    <title>Post</title>
  </head>
  <body>
    <h1><slot name="item.headline"></slot></h1>
    <p><slot name="summary"></slot></p>
  </body>
</html>"#,
    )
    .unwrap();

    let err = build(&BuildOptions::new(&dir)).unwrap_err().to_string();
    assert!(err.contains("`summary` is not bound"), "{err}");
}

#[test]
fn page_undeclared_attr_field_errors() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::write(dir.join("content.json"), r#"[{"slug":"a","headline":"A"}]"#).unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content.json" id="posts" />
  </head>
  <body><a href="/posts/${slug}/"><slot name="item.headline"></slot></a></body>
</html>"#,
    )
    .unwrap();

    let err = build(&BuildOptions::new(&dir)).unwrap_err().to_string();
    assert!(err.contains("`slug` is not bound"), "{err}");
}

#[test]
fn page_declared_data_source_is_bound_for_attrs() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("site.json"),
        r#"{"canonical":"https://example.com/"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="./site.json" id="site" />
    <link rel="canonical" href="${site.canonical}" />
  </head>
  <body></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains(r#"href="https://example.com/""#));
}

#[test]
fn page_bound_data_precedes_data_source_id() {
    let dir = tempfile_dir();
    std::fs::write(dir.join("route.json"), r#""From data id""#).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{page: {route}}">
  <head>
    <link rel="statica/data" href="./route.json" id="route" />
    <meta name="route" content="${route}" />
  </head>
  <body></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains(r#"content="""#), "{html}");
    assert!(!html.contains("From data id"), "{html}");
}

#[test]
fn data_link_id_cannot_use_canonical_page_root() {
    for reserved in ["data", "item", "page", "i18n"] {
        let dir = tempfile_dir();
        std::fs::write(dir.join("source.json"), "{}").unwrap();
        std::fs::write(
            dir.join("index.html"),
            format!(
                r#"<!doctype html>
<html lang="en">
  <head><link rel="statica/data" href="./source.json" id="{reserved}" /></head>
  <body></body>
</html>"#
            ),
        )
        .unwrap();

        let err = build(&BuildOptions::new(&dir)).unwrap_err().to_string();
        assert!(
            err.contains("conflicts with canonical page context"),
            "{err}"
        );
        assert!(err.contains("rename this data source"), "{err}");
    }
}

#[test]
fn fragment_bound_data_precedes_linked_data_id() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("ui/label.json"),
        r#"{"text":"From fragment data"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts.json"),
        r#"[{"slug":"a","label":{"text":"From bound item"}}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/badge.html"),
        r#"<link rel="statica/data" href="./label.json" id="label" />
<template id="badge" data-bind="{label}">
  <span data-t="${label.text}">Fallback</span>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="./posts.json" id="posts" />
  </head>
  <body>
    <link rel="statica/fragment" type="text/html" href="./ui/badge.html" id="badge" />
    <slot id="badge" data-each="posts"></slot>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains(">From bound item</span>"));
}

#[test]
fn page_linked_each_source_is_bound() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(dir.join("posts.json"), r#"[{"slug":"a","headline":"A"}]"#).unwrap();
    std::fs::write(
        dir.join("ui/post-card.html"),
        r#"<template id="post-card" data-bind="{slug, headline}">
  <article><a href="/posts/${slug}/"><slot name="headline"></slot></a></article>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{data}">
  <head>
    <link rel="statica/data" href="./posts.json" id="posts" />
    <link rel="statica/fragment" type="text/html" href="./ui/post-card.html" id="post-card" />
  </head>
  <body><slot id="post-card" data-each="posts"></slot></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains(r#"href="/posts/a/""#));
    assert!(html.contains(">A</a>"));
}

#[test]
fn page_undeclared_each_source_errors() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(dir.join("posts.json"), r#"[{"slug":"a","headline":"A"}]"#).unwrap();
    std::fs::write(
        dir.join("ui/post-card.html"),
        r#"<template id="post-card" data-bind="{slug, headline}">
  <article><a href="/posts/${slug}/"><slot name="headline"></slot></a></article>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="./posts.json" id="posts" />
    <link rel="statica/fragment" type="text/html" href="./ui/post-card.html" id="post-card" />
  </head>
  <body><slot id="post-card" data-each="articles"></slot></body>
</html>"#,
    )
    .unwrap();

    let err = build(&BuildOptions::new(&dir)).unwrap_err().to_string();
    assert!(err.contains("`articles` is not bound"), "{err}");
}

#[test]
fn parses_with_html5ever_not_regex() {
    let doc = statica::parse::parse_document(
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
<html lang="en" data-bind="{data}">
  <head>
    <link rel="statica/data" href="./countries.json" id="countries" />
  </head>
  <body>
    <link rel="statica/fragment" type="text/html" href="./ui/select-option.html" id="select-option" />
    <select name="country" required>
      <slot id="select-option" data-each="data.countries"></slot>
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
<html lang="en" data-bind="{data}">
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
    opts.forms = statica::FormsOptions {
        enabled: true,
        provider: statica::FormProvider::Formspree,
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
<html lang="en" data-bind="{item}">
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
    add_google_font_alias(&mut opts);
    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains("fonts.googleapis.com/css2?family=Outfit:wght@400;700"));
    assert!(!html.contains("statica/font"));
}

#[test]
fn manifest_scaffolds_and_injects_tags() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <meta charset="utf-8" />
  </head>
  <body><p>Hi</p></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.manifest = true;
    build(&opts).expect("build");

    assert!(dir.join("public/manifest.webmanifest").exists());

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains(r#"<link rel="manifest" href="/manifest.webmanifest""#));
    assert!(html.contains("theme-color"));

    let manifest = std::fs::read_to_string(dir.join("dist/public/manifest.webmanifest")).unwrap();
    assert!(manifest.contains("\"name\": \"My Site\""));
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
<html lang="en" data-bind="{i18n}">
  <head><title data-t="${i18n.title}">About</title></head>
  <body>
    <h1 data-t="${i18n.title}">About</h1>
    <span data-t="${i18n.label}">hello</span>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
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
fn i18n_catalog_keys_must_use_canonical_i18n_root() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/about")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(
        dir.join("content/i18n/en.json"),
        r#"{"about": {"title": "About"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/about/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <body><h1 data-t="${about.title}">About</h1></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into()],
        ..Default::default()
    };

    let err = build(&opts).unwrap_err().to_string();
    assert!(
        err.contains("`about` is not bound"),
        "unexpected error: {err}"
    );
}

#[test]
fn data_t_binds_page_context_text_without_i18n() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::create_dir_all(dir.join("content")).unwrap();
    std::fs::write(
        dir.join("content/posts.json"),
        r#"[{"slug":"hello","headline":"Hello from data","summary":"Plain HTML stays plain"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content/posts.json" id="posts">
    <title data-t="${item.headline}">Fallback title</title>
  </head>
  <body>
    <h1 data-t="${item.headline}">Fallback heading</h1>
    <p data-t="${item.summary}">Fallback summary</p>
    <p>${item.headline}</p>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;

    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/posts/hello/index.html")).unwrap();
    assert!(html.contains("<title>Hello from data</title>"));
    assert!(html.contains("<h1>Hello from data</h1>"));
    assert!(html.contains("<p>Plain HTML stays plain</p>"));
    assert!(html.contains("<p>${item.headline}</p>"));
    assert!(!html.contains("data-t"));
}

#[test]
fn i18n_emits_root_redirect_to_default_locale() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(dir.join("content/i18n/en.json"), r#"{"title": "Home"}"#).unwrap();
    std::fs::write(dir.join("content/i18n/pt.json"), r#"{"title": "Início"}"#).unwrap();
    std::fs::write(
        dir.join("[locale]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{i18n}">
  <head><title data-t="${i18n.title}">Home</title></head>
  <body><h1 data-t="${i18n.title}">Home</h1></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };

    build(&opts).expect("build");

    let root = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(root.contains(r#"<meta http-equiv="refresh" content="0; url=/en/""#));
    assert!(root.contains(r#"location.replace("/en/" + location.hash)"#));
    assert!(root.contains(r#"<a href="/en/">Continue to site</a>"#));

    let en = std::fs::read_to_string(dir.join("dist/en/index.html")).unwrap();
    assert!(en.contains("<title>Home</title>"));
    assert!(en.contains("lang=\"en\""));

    let pt = std::fs::read_to_string(dir.join("dist/pt/index.html")).unwrap();
    assert!(pt.contains("<title>Início</title>"));
    assert!(pt.contains("lang=\"pt\""));
}

#[test]
fn i18n_skips_root_redirect_when_author_has_root_page() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(dir.join("content/i18n/en.json"), r#"{"title": "Home"}"#).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html><html><body><p>Landing</p></body></html>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/index.html"),
        r#"<!doctype html><html data-bind="{i18n}"><body><h1 data-t="${i18n.title}">Home</h1></body></html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into()],
        ..Default::default()
    };

    build(&opts).expect("build");

    let root = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(root.contains("Landing"));
    assert!(!root.contains("http-equiv=\"refresh\""));
}

#[test]
fn i18n_loads_locale_specific_funnel_data() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/posts/[slug]")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(dir.join("content/i18n/en.json"), r#"{"title": "Posts"}"#).unwrap();
    std::fs::write(dir.join("content/i18n/pt.json"), r#"{"title": "Artigos"}"#).unwrap();
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
<html lang="en" data-bind="{item, i18n}">
  <head>
    <link rel="statica/data" href="../../../content/posts.${i18n.locale}.json" id="posts" />
    <title data-t="${i18n.title}">Posts</title>
  </head>
  <body>
    <h1><slot name="item.headline"></slot></h1>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
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
fn i18n_locale_data_expands_token_before_glob_loading() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::create_dir_all(dir.join("content/features.en")).unwrap();
    std::fs::create_dir_all(dir.join("content/features.pt")).unwrap();
    std::fs::create_dir_all(dir.join("content/installs.en")).unwrap();
    std::fs::create_dir_all(dir.join("content/installs.pt")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(dir.join("content/i18n/en.json"), r#"{"title": "Home"}"#).unwrap();
    std::fs::write(dir.join("content/i18n/pt.json"), r#"{"title": "Início"}"#).unwrap();
    std::fs::write(
        dir.join("content/features.en/fast.json"),
        r#"{"label":"Fast"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/features.pt/rapido.json"),
        r#"{"label":"Rápido"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/installs.en/npm.json"),
        r#"{"label":"npm install statica"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/installs.pt/install.json"),
        r#"{"label":"instalar statica"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/item.html"),
        r#"<template id="item" data-bind="{label}">
  <li data-t="${label}">Item</li>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{i18n}">
  <head>
    <link rel="statica/fragment" type="text/html" href="../ui/item.html" id="item" />
    <link rel="statica/data" href="../content/features.${i18n.locale}/*.json" id="features" />
    <link rel="statica/data" href="../content/installs.${i18n.locale}/*.json" id="installs" />
    <title data-t="${i18n.title}">Home</title>
  </head>
  <body>
    <h1 data-t="${i18n.title}">Home</h1>
    <a href="/${i18n.locale}/">Locale</a>
    <ul><slot id="item" data-each="features"></slot></ul>
    <ol><slot id="item" data-each="installs"></slot></ol>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };

    build(&opts).expect("build");

    let en = std::fs::read_to_string(dir.join("dist/en/index.html")).unwrap();
    assert!(en.contains("<title>Home</title>"));
    assert!(en.contains("href=\"/en/\""));
    assert!(en.contains("Fast"));
    assert!(en.contains("npm install statica"));

    let pt = std::fs::read_to_string(dir.join("dist/pt/index.html")).unwrap();
    assert!(pt.contains("<title>Início</title>"));
    assert!(pt.contains("href=\"/pt/\""));
    assert!(pt.contains("Rápido"));
    assert!(pt.contains("instalar statica"));
}

#[test]
fn page_dynamic_data_href_requires_defined_context_path() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(dir.join("content/i18n/en.json"), "{}").unwrap();
    std::fs::write(dir.join("content/posts.en.json"), "[]").unwrap();
    std::fs::write(
        dir.join("[locale]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{i18n}">
  <head>
    <link rel="statica/data" href="../content/posts.${locale}.json" id="posts" />
    <title>Home</title>
  </head>
  <body></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into()],
        ..Default::default()
    };

    let err = build(&opts).unwrap_err().to_string();
    assert!(err.contains("`${locale}` is not defined"), "{err}");
}

#[test]
fn fragment_dynamic_data_href_can_use_bound_locale() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::create_dir_all(dir.join("content")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("posts.json"),
        r#"[{"slug":"hello","locale":"en"},{"slug":"ola","locale":"pt"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/messages.en.json"),
        r#"[{"label":"Hello"}]"#,
    )
    .unwrap();
    std::fs::write(dir.join("content/messages.pt.json"), r#"[{"label":"Olá"}]"#).unwrap();
    std::fs::write(
        dir.join("ui/message-list.html"),
        r#"<template id="message-list" data-bind="{locale}">
  <link rel="statica/data" href="../content/messages.${locale}.json" id="messages" />
  <ul>
    <slot id="message-item" data-each="messages"></slot>
  </ul>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/message-item.html"),
        r#"<template id="message-item" data-bind="{label}">
  <li data-t="${label}">Message</li>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../posts.json" id="posts" />
    <link rel="statica/fragment" type="text/html" href="../../ui/message-list.html" id="message-list" />
    <link rel="statica/fragment" type="text/html" href="../../ui/message-item.html" id="message-item" />
    <title>Home</title>
  </head>
  <body>
    <slot id="message-list"></slot>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;

    build(&opts).expect("build");

    let en = std::fs::read_to_string(dir.join("dist/posts/hello/index.html")).unwrap();
    assert!(en.contains("Hello"), "{en}");

    let pt = std::fs::read_to_string(dir.join("dist/posts/ola/index.html")).unwrap();
    assert!(pt.contains("Olá"), "{pt}");
}

#[test]
fn fragment_sibling_dynamic_data_href_cannot_use_template_bind() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("posts/[slug]")).unwrap();
    std::fs::create_dir_all(dir.join("content")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("posts.json"),
        r#"[{"slug":"hello","locale":"en"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/messages.en.json"),
        r#"[{"label":"Hello"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/message-list.html"),
        r#"<link rel="statica/data" href="../content/messages.${locale}.json" id="messages" />
<template id="message-list" data-bind="{locale}">
  <ul>
    <slot id="message-item" data-each="messages"></slot>
  </ul>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/message-item.html"),
        r#"<template id="message-item" data-bind="{label}">
  <li data-t="${label}">Message</li>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("posts/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../posts.json" id="posts" />
    <link rel="statica/fragment" type="text/html" href="../../ui/message-list.html" id="message-list" />
    <link rel="statica/fragment" type="text/html" href="../../ui/message-item.html" id="message-item" />
    <title>Home</title>
  </head>
  <body>
    <slot id="message-list"></slot>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;

    let err = build(&opts).unwrap_err().to_string();
    assert!(err.contains("`${locale}` is not defined"), "{err}");
}

#[test]
fn i18n_fragment_cannot_use_canonical_context_without_bound_data() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/about")).unwrap();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(dir.join("content/i18n/en.json"), r#"{"cta": "Contact us"}"#).unwrap();
    std::fs::write(
        dir.join("content/i18n/pt.json"),
        r#"{"cta": "Contacte-nos"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ui/button.html"),
        r#"<template id="button">
  <button type="button"><span data-t="${i18n.cta}">Contact</span></button>
</template>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/about/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{i18n}">
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
<html lang="en" data-bind="{i18n, page}">
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
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };

    let err = build(&opts).unwrap_err().to_string();
    assert!(err.contains("`i18n` is not bound"), "{err}");
}

#[test]
fn i18n_translates_a11y_attributes() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("[locale]/about")).unwrap();
    std::fs::create_dir_all(dir.join("content/i18n")).unwrap();
    std::fs::write(
        dir.join("content/i18n/en.json"),
        r#"{
  "skip": "Skip to main content",
  "photo": { "alt": "Sunset over the hills" },
  "form": { "email_placeholder": "Your email address" }
}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("content/i18n/pt.json"),
        r#"{
  "skip": "Saltar para o conteúdo principal",
  "photo": { "alt": "Pôr do sol sobre as colinas" },
  "form": { "email_placeholder": "O seu endereço de email" }
}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("[locale]/about/index.html"),
        r##"<!doctype html>
<html lang="en" data-bind="{i18n}">
  <body>
    <a href="#main" aria-label="Skip to main content" data-t-aria-label="${i18n.skip}"></a>
    <img src="/sunset.jpg" alt="Sunset over the hills" data-t-alt="${i18n.photo.alt}" />
    <input type="email" placeholder="Your email address" data-t-placeholder="${i18n.form.email_placeholder}" />
  </body>
</html>"##,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };

    build(&opts).expect("build");

    let en = std::fs::read_to_string(dir.join("dist/en/about/index.html")).unwrap();
    assert!(en.contains("aria-label=\"Skip to main content\""));
    assert!(en.contains("alt=\"Sunset over the hills\""));
    assert!(en.contains("placeholder=\"Your email address\""));
    assert!(!en.contains("data-t-"));

    let pt = std::fs::read_to_string(dir.join("dist/pt/about/index.html")).unwrap();
    assert!(pt.contains("aria-label=\"Saltar para o conteúdo principal\""));
    assert!(pt.contains("alt=\"Pôr do sol sobre as colinas\""));
    assert!(pt.contains("placeholder=\"O seu endereço de email\""));
    assert!(!pt.contains("data-t-"));
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
<html lang="en" data-bind="{i18n, page}">
  <head>
    <link rel="statica/data" href="../../../content/posts.json" id="posts" />
    <title data-t="${i18n.blog_title}">Blog</title>
  </head>
  <body>
    <h1 data-t="${i18n.blog_title}">Blog</h1>
    <p>Page <slot name="page.pagination.page"></slot> of <slot name="page.pagination.total_pages"></slot></p>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.i18n = statica::I18nOptions {
        enabled: true,
        default_locale: "en".into(),
        locales: vec!["en".into(), "pt".into()],
        ..Default::default()
    };
    opts.pagination = vec![statica::PaginationRule {
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
fn pagination_root_expands_listing_and_nested_item_pages() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("blog/[page]/[slug]")).unwrap();
    std::fs::write(
        dir.join("content.json"),
        r#"[
  {"slug":"alpha","headline":"Alpha"},
  {"slug":"beta","headline":"Beta"},
  {"slug":"gamma","headline":"Gamma"}
]"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("blog/[page]")).unwrap();
    std::fs::write(
        dir.join("blog/[page]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{page}">
  <head>
    <link rel="statica/data" href="../../content.json" id="posts" />
    <link rel="statica/fragment" type="text/html" href="./post-link.html" id="post-link" />
    <title data-t="Page ${page.pagination.page}">Page</title>
  </head>
  <body>
    <p data-t="${page.pagination.href}">Href</p>
    <slot id="post-link" data-each="page.pagination.items"></slot>
  </body>
</html>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("blog/[page]/[slug]/index.html"),
        r#"<!doctype html>
<html lang="en" data-bind="{item, page}">
  <head>
    <link rel="statica/data" href="../../../content.json" id="posts" />
    <title data-t="${item.headline}">Post</title>
  </head>
  <body>
    <h1 data-t="${item.headline}">Post</h1>
    <p data-t="${page.pagination.href}">Page href</p>
    <p data-t="${page.params.page}">Page param</p>
    <p data-t="${page.params.slug}">Slug param</p>
  </body>
</html>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("blog/[page]/post-link.html"),
        r#"<template id="post-link" data-bind="{slug, headline}">
  <a href="./${slug}/" data-t="${headline}">Post</a>
</template>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.clean = true;
    opts.pagination = vec![statica::PaginationRule {
        page_size: 2,
        index: true,
        ..Default::default()
    }];

    build(&opts).expect("build");

    let listing = std::fs::read_to_string(dir.join("dist/blog/1/index.html")).unwrap();
    assert!(listing.contains("/blog/1/"));

    let alpha = std::fs::read_to_string(dir.join("dist/blog/1/alpha/index.html")).unwrap();
    assert!(alpha.contains("<title>Alpha</title>"));
    assert!(alpha.contains("/blog/1/"));
    assert!(alpha.contains(">1</p>"));
    assert!(alpha.contains(">alpha</p>"));

    let gamma = std::fs::read_to_string(dir.join("dist/blog/2/gamma/index.html")).unwrap();
    assert!(gamma.contains("<title>Gamma</title>"));

    assert!(dir.join("dist/blog/index.html").exists());
    assert!(!dir.join("dist/blog/[slug]/index.html").exists());
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
    opts.minify = statica::MinifyOptions {
        enabled: true,
        ..statica::MinifyOptions::default()
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

#[test]
fn responsive_images_wrap_img_in_picture() {
    let dir = tempfile_dir();
    std::fs::create_dir_all(dir.join("assets")).unwrap();

    let mut img = image::RgbImage::new(800, 600);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([x as u8, y as u8, 100]);
    }
    let dyn_img = image::DynamicImage::ImageRgb8(img);
    let mut buf = Vec::new();
    dyn_img
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    std::fs::write(dir.join("assets/hero.jpg"), &buf).unwrap();

    std::fs::write(
        dir.join("index.html"),
        r##"<!doctype html>
<html lang="en">
  <body>
    <img src="/assets/hero.jpg" alt="Hero" data-s-img-sizes="(max-width: 768px) 100vw, 50vw" />
  </body>
</html>"##,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.process = statica::AssetProcessOptions {
        enabled: true,
        images: true,
        css: false,
        js: false,
        fonts: false,
        image: statica::ImageProcessOptions {
            widths: vec![400, 800],
            ..statica::ImageProcessOptions::default()
        },
    };

    build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(html.contains("<picture>"), "expected picture: {html}");
    assert!(html.contains("type=\"image/webp\""));
    assert!(html.contains("srcset=\"/assets/hero-400w.webp 400w"));
    assert!(html.contains("sizes=\"(max-width: 768px) 100vw, 50vw\""));
    assert!(html.contains("loading=\"lazy\""));
    assert!(html.contains("width=\"800\""));
    assert!(html.contains("height=\"600\""));
    assert!(!html.contains("data-s-img"));
    assert!(!html.contains("data-statica"));
    assert!(dir.join("dist/assets/hero-400w.webp").exists());
    assert!(dir.join("dist/assets/hero-800w.jpg").exists());
}

#[test]
fn emits_default_404_when_author_omits_one() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head><title>Home</title></head>
  <body><h1>Home</h1></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");

    let report = build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/404/index.html")).unwrap();
    assert!(html.contains("<title>404 Not Found</title>"));
    assert!(html.contains("The page you are looking for does not exist."));
    assert_eq!(report.pages_written, 1);
    assert!(!report.outputs.contains(&dir.join("dist/404/index.html")));
}

#[test]
fn preserves_authored_404_page() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head><title>Home</title></head>
  <body><h1>Home</h1></body>
</html>"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("404")).unwrap();
    std::fs::write(
        dir.join("404/index.html"),
        r#"<!doctype html>
<html lang="en">
  <head><title>Custom Missing</title></head>
  <body><h1>Custom Missing</h1></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");

    let report = build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/404/index.html")).unwrap();
    assert!(html.contains("<title>Custom Missing</title>"));
    assert!(!html.contains("The page you are looking for does not exist."));
    assert_eq!(report.pages_written, 2);
}

#[test]
fn statica_search_input_emits_modal_runtime_and_index() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head><title>Home Search</title></head>
  <body>
    <input type="statica/search" placeholder="Find things" data-limit="7" />
    <main><h1>Home</h1><p>Needle content lives here.</p></main>
    <script>const hidden = "not searchable";</script>
  </body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");

    let report = build(&opts).expect("build");

    let html = std::fs::read_to_string(dir.join("dist/index.html")).unwrap();
    assert!(!html.contains(r#"type="statica/search""#));
    assert!(html.contains("statica-search-modal"));
    assert!(html.contains("data-limit=\"7\""));
    assert!(dir.join("dist/statica/search.js").exists());
    assert!(dir.join("dist/statica/search.css").exists());

    let index = std::fs::read_to_string(dir.join("dist/search.json")).unwrap();
    assert!(index.contains("\"url\": \"/\""));
    assert!(index.contains("Home Search"));
    assert!(index.contains("Needle content lives here."));
    assert!(!index.contains("not searchable"));
    assert!(report.phases.iter().any(|phase| phase.name == "search"));
}

#[test]
fn search_config_emits_index_without_search_input() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head><title>Docs</title></head>
  <body><main><p>Config enabled search content.</p></main></body>
</html>"#,
    )
    .unwrap();

    let mut opts = BuildOptions::new(&dir);
    opts.out_dir = dir.join("dist");
    opts.search = statica::SearchOptions {
        enabled: true,
        output: "assets/site-search.json".into(),
    };

    build(&opts).expect("build");

    let index = std::fs::read_to_string(dir.join("dist/assets/site-search.json")).unwrap();
    assert!(index.contains("Config enabled search content."));
    assert!(!dir.join("dist/statica/search.js").exists());
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
