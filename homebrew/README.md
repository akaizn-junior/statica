# Homebrew tap

Release CI publishes the `statica` formula to `{owner}/homebrew-statica`.

Users install with:

```bash
brew tap akaizn-junior/statica
brew install statica
```

## One-time setup

1. Create a public GitHub repository named `homebrew-statica` under the same owner as this repo.
2. Add an initial commit with `Formula/statica.rb` (copy from this directory after a local formula generation, or run the release once).
3. Add a repository secret `HOMEBREW_TAP_TOKEN` on **this** repo — a fine-grained or classic PAT with `contents: write` on the tap repository.

On each release, `release.yml` regenerates the formula (version, URLs, SHA256 checksums for macOS and Linux binaries) and pushes to the tap.

Regenerate locally:

```bash
node scripts/update-homebrew-formula.mjs \
  --version 0.12.0 \
  --repo akaizn-junior/statica \
  --assets-dir ./release-assets \
  --output ./homebrew/statica.rb
```
