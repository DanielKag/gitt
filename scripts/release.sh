#!/usr/bin/env bash
#
# Cut a gitt release, end to end:
#
#   scripts/release.sh 0.2.0
#
# Bumps the version, tags it, waits for CI to build and publish the macOS universal binary, then points
# the Homebrew tap at it. Runs from your Mac using the `gh` login you already have — no access token to
# create, nothing to configure. Safe to re-run: every step checks whether it has already happened.
set -euo pipefail

REPO="danielkag/gitt"
TAP="danielkag/homebrew-gitt"

die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

VERSION="${1:-}"
[ -n "$VERSION" ] || die "usage: scripts/release.sh <version>   (e.g. scripts/release.sh 0.2.0)"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must look like 1.2.3, got '$VERSION'"
TAG="v$VERSION"

cd "$(dirname "$0")/.."
command -v gh >/dev/null || die "the GitHub CLI (gh) is required: brew install gh"
gh auth status >/dev/null 2>&1 || die "not logged in to GitHub: run 'gh auth login'"

# --- preflight: never release from a dirty tree or a stale branch --------------------------------
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || die "release from main, not $(git rev-parse --abbrev-ref HEAD)"
git diff --quiet && git diff --cached --quiet || die "working tree is dirty — commit or stash first"
git fetch --quiet origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || die "local main and origin/main differ — push or pull first"

step "Running the test suite"
cargo test --locked

# --- 1. bump the version (skipped if it is already there) ---------------------------------------
CURRENT=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [ "$CURRENT" = "$VERSION" ]; then
  step "Cargo.toml is already at $VERSION"
else
  step "Bumping $CURRENT → $VERSION"
  # Only the [package] version, which is the first `version =` line in the file.
  sed -i '' "1,/^version = /s/^version = \"$CURRENT\"/version = \"$VERSION\"/" Cargo.toml
  cargo check --quiet          # refreshes Cargo.lock so --locked builds agree
  git commit -qam "Release $TAG"
fi

# --- 2. tag and push ----------------------------------------------------------------------------
if git rev-parse "$TAG" >/dev/null 2>&1; then
  step "Tag $TAG already exists locally"
else
  step "Tagging $TAG"
  git tag "$TAG"
fi
step "Pushing main and $TAG"
git push --follow-tags

# --- 3. wait for the build ----------------------------------------------------------------------
ASSET="gitt-$VERSION-macos-universal.tar.gz"
if gh release view "$TAG" --repo "$REPO" --json assets --jq '.assets[].name' 2>/dev/null | grep -qx "$ASSET"; then
  step "Release $TAG is already published"
else
  step "Waiting for the release build (a few minutes)"
  sleep 10  # give GitHub a moment to register the run for this tag
  RUN=$(gh run list --repo "$REPO" --workflow Release --limit 1 --json databaseId --jq '.[0].databaseId')
  [ -n "$RUN" ] || die "no Release workflow run appeared for $TAG — check $REPO's Actions tab"
  gh run watch "$RUN" --repo "$REPO" --exit-status \
    || die "the release build failed: gh run view $RUN --repo $REPO --log-failed"
fi

# --- 4. point the Homebrew tap at the new binary ------------------------------------------------
step "Updating the Homebrew tap"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

gh release download "$TAG" --repo "$REPO" --pattern '*.tar.gz' --dir "$WORK" --clobber
SHA=$(shasum -a 256 "$WORK/$ASSET" | cut -d' ' -f1)
echo "sha256: $SHA"

git clone --quiet "git@github.com:$TAP.git" "$WORK/tap"
mkdir -p "$WORK/tap/Formula"
sed -e "s|@VERSION@|$VERSION|g" -e "s|@SHA256@|$SHA|g" -e "s|@ARCHIVE@|$ASSET|g" \
  packaging/homebrew/gitt.rb.tmpl > "$WORK/tap/Formula/gitt.rb"

git -C "$WORK/tap" add Formula/gitt.rb
if git -C "$WORK/tap" diff --cached --quiet; then
  echo "the formula is already at $VERSION"
else
  git -C "$WORK/tap" commit -qm "gitt $VERSION"
  git -C "$WORK/tap" push --quiet
  echo "tap updated"
fi

printf '\n\033[32m✓ gitt %s released\033[0m\n' "$VERSION"
echo "  https://github.com/$REPO/releases/tag/$TAG"
echo
echo "Anyone (including you) upgrades with:"
echo "  brew update && brew upgrade gitt"
