# Contributing to Emploidle

## Commit messages

Use Conventional Commits, following the style used in Git Account Manager:

Run `pnpm install` during setup. Its `prepare` script installs Husky's `commit-msg` hook, which runs `pnpm exec commitlint --edit "$1"` using [commitlint.config.js](commitlint.config.js). CI checks commit messages and pull request titles as well.

```text
type(scope): subject
```

Scope is optional. Write the subject in English, start it with a lowercase imperative verb, and omit the trailing period. Keep one concern per commit and per pull request. Pull request titles use the same format because a squash merge uses the title as its commit message.

```text
feat(ui): display the application version
fix(platform): avoid a console window on Windows
docs: describe the release process
chore(release): prepare 1.0.0
```

Allowed types are `feat`, `fix`, `perf`, `refactor`, `docs`, `style`, `test`, `build`, `ci`, `chore` and `revert`. Use `feat` for new behavior, `fix` for a correction, `perf` for a performance improvement, and `refactor` for changes that preserve behavior. `chore` is housekeeping and `style` is code formatting; user-visible changes use `feat` or `fix`.

Choose the scope from the area changed. Suggested scopes for this project are `activity`, `build`, `ci`, `contributing`, `deps`, `docs`, `platform`, `process`, `readme`, `release`, `scripts`, `security`, `tauri`, `tray` and `ui`. Reuse an existing scope for an existing area; introduce a new one when the area is new. Commitlint warns on an unknown scope and permits it.

Limit the header and each body line to 100 characters. Put a blank line between the header, body and footers. Footer lines have no length limit so that URLs and commit references remain intact.

A `!` after the type or scope marks an incompatible change and requires a `BREAKING CHANGE: <what breaks>` footer:

```text
feat(ui)!: change the tray controls

BREAKING CHANGE: the tray menu replaces the previous control layout.
```

A `revert` message requires a `Refs: <sha>` footer identifying the reverted commit, using its 7–40 character hexadecimal Git hash:

```text
revert(ui): restore the previous tray controls

Refs: 79a8186
```

`pnpm release` uses `commit-and-tag-version` to derive the next version from the commits since the last tag: breaking changes bump major, features bump minor, and other changes bump patch. It synchronizes `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`, writes the changelog, creates the release commit and tags it. `feat`, `fix`, `perf`, `refactor` and `revert` appear in the changelog; other types are hidden. See [the release guide](docs/releasing.md).

## Checks

```sh
pnpm check:versions
pnpm lint
pnpm lint:workflow
pnpm test
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

On Windows, build a debug executable with `pnpm tauri build --debug --no-bundle --ci -- --locked`, then run `pnpm test:windows` to check the actual process, window title, metadata and WebView2 browser. JavaScript tooling uses Vitest; Rust tests remain in Cargo.
