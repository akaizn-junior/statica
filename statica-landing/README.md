# statica landing page

Product Hunt launch landing page for statica, built with statica itself.

The site follows statica's authoring model:

- `index.html` is valid HTML and the root route.
- `content/` contains build-time data funnels.
- `ui/` contains reusable build-time fragments.
- daisyUI v5 provides the component styling layer through plain CSS classes.
- `styles.css` only adds layout and launch-specific polish around daisyUI components.

## Project layout

```text
statica-landing/
├── statica.toml
├── index.html
├── styles.css
├── content/
│   ├── features.json
│   └── installs.json
├── ui/
│   ├── feature-card.html
│   └── install-card.html
└── .github/workflows/pages.yml
```

## Run locally

Install statica using one of the supported methods, then from this directory:

```bash
statica watch
```

For a production build:

```bash
statica .
```

The generated site is emitted to `.dist/`.

## GitHub Pages

Deploy the contents of `.dist/` with GitHub Pages. If this is hosted somewhere other than `https://akaizn-junior.github.io/statica/`, update `site_url` in `statica.toml` and the `og:url` value in `index.html`.

## Before Product Hunt launch

Add the final Product Hunt launch URL and a 1200×630 `og:image` once those assets exist. Do not add a placeholder Product Hunt badge that points nowhere.

## Automatic deployment

The included GitHub Pages workflow installs statica, runs `statica .`, uploads `.dist/`, and deploys that generated output through GitHub Pages. In the repository Settings → Pages, set the source to **GitHub Actions**.
