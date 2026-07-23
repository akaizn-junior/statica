#!/usr/bin/env bash
# Push Formula/statica.rb to the homebrew-statica tap repository.
set -euo pipefail

VERSION="${1:?usage: publish-homebrew-tap.sh VERSION REPO [ASSETS_DIR]}"
REPO="${2:?usage: publish-homebrew-tap.sh VERSION REPO [ASSETS_DIR]}"
ASSETS_DIR="${3:-./release-assets}"
TAP="${REPO%%/*}/homebrew-statica"
FORMULA="./homebrew/statica.rb"
TAP_DIR="./homebrew-tap"

if [[ -z "${HOMEBREW_TAP_TOKEN:-}" ]]; then
  echo "HOMEBREW_TAP_TOKEN not set; skipping Homebrew tap update"
  exit 0
fi

node scripts/update-homebrew-formula.mjs \
  --version "${VERSION}" \
  --repo "${REPO}" \
  --assets-dir "${ASSETS_DIR}" \
  --output "${FORMULA}"

TAP_URL="https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/${TAP}.git"
rm -rf "${TAP_DIR}"

if git clone --depth 1 "${TAP_URL}" "${TAP_DIR}" 2>/dev/null; then
  :
else
  mkdir -p "${TAP_DIR}"
  git -C "${TAP_DIR}" init -b main
  git -C "${TAP_DIR}" remote add origin "${TAP_URL}"
fi

mkdir -p "${TAP_DIR}/Formula"
cp "${FORMULA}" "${TAP_DIR}/Formula/statica.rb"

if [[ ! -f "${TAP_DIR}/README.md" ]]; then
  cat > "${TAP_DIR}/README.md" <<EOF
# homebrew-statica

Homebrew tap for [statica](https://github.com/${REPO}).

\`\`\`bash
brew tap ${REPO%%/*}/statica
brew install statica
\`\`\`
EOF
  git -C "${TAP_DIR}" add README.md
fi

git -C "${TAP_DIR}" config user.name "github-actions[bot]"
git -C "${TAP_DIR}" config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git -C "${TAP_DIR}" add Formula/statica.rb

if git -C "${TAP_DIR}" diff --staged --quiet; then
  echo "Homebrew formula unchanged"
  exit 0
fi

git -C "${TAP_DIR}" commit -m "statica ${VERSION}"
git -C "${TAP_DIR}" push -u origin HEAD

echo "Published statica ${VERSION} to ${TAP}"
