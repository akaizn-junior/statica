# Homebrew tap

Release CI publishes the `statica` formula to `{owner}/homebrew-statica`.

Users install with:

```bash
brew tap akaizn-junior/statica
brew install statica
```

## One-time setup

1. Create a public GitHub repository named `homebrew-statica` under the same owner as this repo (can be empty).
2. Add a repository secret `HOMEBREW_TAP_TOKEN` on **this** repo — a PAT with `contents: write` on the tap repository.

## What CI does

On each release (`release.yml`):

1. **`github-release`** — uploads binaries to GitHub Releases
2. **`homebrew`** — runs `scripts/publish-homebrew-tap.sh`, which:
   - generates `Formula/statica.rb` via `scripts/update-homebrew-formula.mjs`
   - initializes the tap repo if empty, otherwise clones and updates it
   - pushes to `homebrew-statica`

Homebrew runs in a separate job from npm/crates.io, so a failed npm publish does not block the tap update.

To seed or retry the tap for an existing tag, re-run the **Release** workflow manually with that tag (e.g. `v0.12.1`).
