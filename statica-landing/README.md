# statica landing page

Product Hunt launch landing page for statica, built with statica itself and styled with daisyUI.

## Development

```bash
statica watch .
```

`watch` builds, rebuilds on changes, and serves `.website`. `statica serve .` only previews an already-built `.website` directory.

## i18n

The site uses statica's native `[locale]` routing and canonical `i18n` binding:

```text
[locale]/index.html
content/i18n/en.json
content/i18n/pt.json
ui/feature-card.html
ui/install-card.html
```

The localized page binds only `{i18n}`. It has no `statica/data` funnels, so `[locale]` cannot be confused with a collection route when multiple page data sources are present.

Translated feature and install-card records live directly in each locale catalog under `feature_items` and `install_items`. The page passes each record to its reusable fragment with:

```html
<slot id="feature-card" data-each="i18n.feature_items"></slot>
<slot id="install-card" data-each="i18n.install_items"></slot>
```

Fragments therefore receive only their explicit item context; they do not depend on inheriting the page's canonical `i18n` root.

The install cards use default-scoped fragment scripts. Their copy buttons call `document.querySelector(".copy-command")`, and statica scopes that lookup to the mounted `install-card` instance during the build.

The root `index.html` redirects to `./en/`. Locale-switch links are relative siblings (`../en/`, `../pt/`), so the site works locally and under GitHub Pages' `/statica/` project subpath.

## Build

```bash
statica .
```

Expected output includes:

```text
.website/index.html
.website/en/index.html
.website/pt/index.html
.website/public/styles.css
```

## Deployment

The repository GitHub Actions workflow builds with statica and publishes `statica-landing/.website` to GitHub Pages. The landing source links CSS relatively as `../public/styles.css`, so styles work both locally and under the `/statica/` GitHub Pages project path.
