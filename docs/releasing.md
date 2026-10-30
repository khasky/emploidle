# Releasing Emploidle

The release flow uses the same tools as Git Account Manager: pnpm, Husky, commitlint, `commit-and-tag-version`, `.versionrc.json`, Cargo updaters and JavaScript release-note extraction. The first version is `1.0.0`, tagged `v1.0.0`.

## Setup and checks

Install Node.js 22 or newer, the Rust toolchain and pnpm 9.15.4. The package manager version is pinned in `package.json`.

```sh
pnpm install
pnpm check:versions
pnpm lint
pnpm lint:workflow
pnpm test
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

`pnpm install` runs `prepare: husky`, installing the commit-message hook. Commitlint checks the Conventional Commit header, optional scope, line lengths, breaking-change explanation and revert reference. CI repeats message validation and checks squash-merge pull request titles.

## First release

The repository needs an initial commit before the release tool can read its history. Review the working tree and commit the initial application using a Conventional Commit:

```sh
git status --short
git add .
git commit -m "feat: introduce Emploidle"
pnpm release --first-release --dry-run
```

The changelog starts as a seed header. The JavaScript `prechangelog` hook clears a header-only seed so `commit-and-tag-version` can write one changelog header and generate the first release's features from the initial commit. The prepared application version stays at `1.0.0` with `--first-release`.

After reviewing the dry run:

```sh
pnpm release --first-release
pnpm check:versions
git push --follow-tags origin main
```

The release command writes `CHANGELOG.md`, creates `chore(release): 1.0.0`, and creates the annotated tag `v1.0.0`. It performs the commit and tag itself, so run it only when ready to create them. Use `--first-release` only once. The development tooling setup does not run this command automatically.

## Later releases

```sh
pnpm release --dry-run
pnpm release
pnpm check:versions
git push --follow-tags origin main
```

The bump comes from Conventional Commits since the last tag: incompatible changes bump major, `feat` bumps minor, and other changes bump patch. `.versionrc.json` updates these files together:

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`, through `scripts/cargo-toml-updater.cjs`
- `src-tauri/Cargo.lock`, through `scripts/cargo-lock-updater.cjs`

The Cargo updaters change only Emploidle's package version. Dependency versions remain locked. The Rust package version also supplies the version shown in the application window.

To select a version explicitly:

```sh
pnpm release --release-as minor
pnpm release --release-as 1.1.0
```

## GitHub Actions

[Build & Release](../.github/workflows/build.yml) checks JavaScript tooling, version parity, commit messages, Rust lints and tests. Pull request titles have their own Conventional Commit check. Every third-party action is pinned to a commit.

A pushed `v*` tag builds Windows x64, Linux x64, macOS Apple Silicon and macOS Intel installers. Build jobs use `pnpm tauri`. After every platform succeeds, `finalize-release` runs JavaScript scripts to rename the assets and extract the exact version's notes from `CHANGELOG.md`.

Assets use `Emploidle-<version>-<win|mac|linux>-<x64|arm64>[-setup].<ext>`. macOS application archives use `.app.tar.gz`. There is no updater manifest because this application has no automatic updater.

Use **Actions → Build & Release → Run workflow** on `main` to rehearse platform builds. A manual run stores installers as workflow artifacts and creates no release. After a tag build and finalization succeed, review the draft under **Releases** and choose **Publish release**.

The workflow uses the automatic `GITHUB_TOKEN`; no signing secrets are required. Windows installers have no publisher certificate, and macOS builds are not notarized. A failed platform build leaves an incomplete draft; publish it only after all required jobs succeed. Correct an already published release with a new patch version instead of moving its tag.

Release-note extraction can also be run locally:

```sh
node scripts/release-notes.mjs 1.0.0 src-tauri/target/release-notes.md
```

## Local Windows installers

```sh
pnpm build:desktop --ci --bundles nsis,msi -- --locked
```

The executable is `src-tauri/target/release/emploidle.exe`. Installers are under `src-tauri/target/release/bundle/nsis/` and `src-tauri/target/release/bundle/msi/`. The running application uses a random process name; release asset names carry the product and version.
