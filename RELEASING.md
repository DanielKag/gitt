# Releasing gitt

`gitt` ships as **one macOS universal binary** (arm64 + x86_64) per release, installed through a
Homebrew tap. Everything below the one-time setup is automated by
[`.github/workflows/release.yml`](.github/workflows/release.yml).

## One-time setup

1. **Push the code** to `github.com/danielkag/gitt` (public). CI runs on macOS: `fmt`, `clippy`,
   unit + e2e tests.

2. **Create the tap repo** — Homebrew resolves `danielkag/gitt` to the repo `homebrew-gitt`, so the
   name matters:

   ```bash
   gh repo create danielkag/homebrew-gitt --public \
     --description "Homebrew tap for gitt" --add-readme
   ```

3. **Let releases bump the tap.** Create a fine-grained PAT with **Contents: read and write** scoped
   to `danielkag/homebrew-gitt` only, then add it to the main repo:

   ```bash
   gh secret set TAP_TOKEN --repo danielkag/gitt
   ```

   Without this secret the release still publishes; only the formula bump is skipped (see
   [manual bump](#bumping-the-formula-by-hand)).

## Cutting a release

```bash
# 1. bump `version` in Cargo.toml (and Cargo.lock: `cargo check` refreshes it)
# 2. tag it — the workflow refuses a tag that disagrees with Cargo.toml
git commit -am "Release v0.2.0"
git tag v0.2.0
git push --follow-tags
```

The workflow then:

1. verifies the tag matches `Cargo.toml`,
2. runs the full test suite on macOS,
3. builds both darwin targets and `lipo`s them into `dist/gitt`,
4. packages `gitt-<version>-macos-universal.tar.gz` (binary + README + LICENSE),
5. creates the GitHub release with generated notes and the tarball attached,
6. renders [`packaging/homebrew/gitt.rb.tmpl`](packaging/homebrew/gitt.rb.tmpl) with the version and
   sha256 and commits it to the tap as `Formula/gitt.rb`.

Users get it with:

```bash
brew tap danielkag/gitt
brew trust danielkag/gitt          # Homebrew 6+ gate on third-party taps
brew install gitt                  # or `brew upgrade gitt`
```

You can also re-run a published tag from the Actions tab via **workflow_dispatch**.

## Bumping the formula by hand

If `TAP_TOKEN` isn't set, take the `sha256` from the release job summary and commit this to
`danielkag/homebrew-gitt` as `Formula/gitt.rb` — it's `packaging/homebrew/gitt.rb.tmpl` with
`@VERSION@`, `@ARCHIVE@`, and `@SHA256@` filled in.

Verify before announcing:

```bash
brew uninstall gitt 2>/dev/null
brew untap danielkag/gitt 2>/dev/null
brew tap danielkag/gitt
brew trust danielkag/gitt
brew install --verbose gitt
brew test gitt         # asserts --version and the "not a git repository" exit path
brew audit --formula danielkag/gitt/gitt   # must be clean before announcing
gitt log
```

## Local rehearsal

The build and packaging steps run fine on a Mac (note: use the `rustup` toolchain — a Homebrew-installed
`rustc` has no cross-target std):

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin
mkdir -p dist && lipo -create -output dist/gitt \
  target/aarch64-apple-darwin/release/gitt target/x86_64-apple-darwin/release/gitt
lipo -info dist/gitt && dist/gitt --version
```
