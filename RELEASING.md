# Releasing gitt

```bash
scripts/release.sh 0.2.0
```

That's the whole thing. It bumps the version, tags it, waits for CI to build and publish the macOS
universal binary, and points the Homebrew tap at the result. Nothing to configure and no access token
to create — it runs from your Mac with the `gh` login you already have.

Then anyone (including you) picks it up with:

```bash
brew update && brew upgrade gitt
```

## What it actually does

1. **Refuses to start** if you're not on `main`, the tree is dirty, or `main` and `origin/main` differ.
2. Runs `cargo test --locked`.
3. Bumps `version` in `Cargo.toml` (and `Cargo.lock`), commits `Release v0.2.0`.
4. Tags `v0.2.0` and pushes both.
5. Watches [`release.yml`](.github/workflows/release.yml), which builds both darwin targets, `lipo`s
   them into one universal binary, and publishes the GitHub release with the tarball attached.
6. Downloads that tarball, checksums it, renders
   [`packaging/homebrew/gitt.rb.tmpl`](packaging/homebrew/gitt.rb.tmpl), and commits the result to
   `danielkag/homebrew-gitt` as `Formula/gitt.rb`.

Every step checks whether it has already happened, so if the build fails or your network drops you can
fix the problem and run the same command again — it picks up where it stopped.

## Why the tap update isn't in CI

Updating the tap means pushing to a *second* repository. A GitHub Actions run only has rights to the
repo it lives in, so doing it from CI would mean creating a personal access token and storing it as a
secret — real setup, and a credential to rotate — purely to lend the bot an identity you already have
on your laptop. Doing that one push locally is simpler and needs nothing. CI keeps the job it's good
at: building the binary in a clean, reproducible environment.

## One-time setup (already done)

For reference, if this ever needs recreating:

```bash
gh repo create danielkag/gitt --public
gh repo create danielkag/homebrew-gitt --public   # the `homebrew-` prefix is what `brew tap` expects
```

## Verifying a release by hand

```bash
brew update && brew upgrade gitt
gitt --version
brew test gitt                              # --version, and the "not a git repository" exit path
brew audit --formula danielkag/gitt/gitt    # should be silent
```

Installing from a third-party tap needs a one-time `brew trust danielkag/gitt` on Homebrew 6+.

## Building the artifact locally

Rarely needed — CI does this — but if you want to check the binary before tagging (use the `rustup`
toolchain; a Homebrew-installed `rustc` has no cross-target std):

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin
mkdir -p dist && lipo -create -output dist/gitt \
  target/aarch64-apple-darwin/release/gitt target/x86_64-apple-darwin/release/gitt
lipo -info dist/gitt && dist/gitt --version
```
