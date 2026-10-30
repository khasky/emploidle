# Emploidle

Cross-platform anti-idle activity simulator. A single Start/Stop button; while running it imitates human "working" activity — paced scrolling ("reading"), switching between applications and tabs, and small mouse movements — so the machine stays awake and marked active.

## What it does when running

- **Reading scroll** — scrolls down in small steps at a human pace with occasional pauses, then jumps back to the top.
- **Switch application** — Alt+Tab (Cmd+Tab on macOS) to bring another window forward.
- **Switch tab** — Ctrl+Tab through a few tabs of the focused browser / editor.
- **Micro-moves** — small cursor movements.

Actions and pauses are randomized so the rhythm is not mechanical.

The window hides to the tray when closed; it keeps running there. Exit only via the tray menu → Quit. The tray menu also has Start / Stop / Show window.

## Build & run

Requires the [Rust toolchain](https://rustup.rs), Node.js 22 or newer, and pnpm 9.15.4 (pinned in `package.json`). Install the project's tooling, including the Tauri CLI and commit hook:

```sh
pnpm install
```

Platform prerequisites (Linux only): WebKitGTK and friends — see the [Tauri Linux setup](https://tauri.app/start/prerequisites/).

From the project root:

```sh
pnpm dev:desktop      # run
pnpm build:desktop    # produce a bundled binary/installer
```

The frontend is static files under `src/`. Node.js runs the development and release tooling.

## Versioning and releases

The first release is **1.0.0**. `pnpm release` derives versions and [CHANGELOG.md](CHANGELOG.md) from Conventional Commits, synchronizing `package.json`, Tauri's config and both Cargo files using [.versionrc.json](.versionrc.json).

Run `pnpm check:versions` to check version parity, or `pnpm release --dry-run` to preview a release after committing the initial application. A pushed `v<version>` tag builds installers for Windows, Linux and macOS and prepares a draft GitHub Release. See [the release guide](docs/releasing.md) for the first-release command and publishing.

Commit messages and pull request titles follow [Conventional Commits](CONTRIBUTING.md#commit-messages), using the style from Git Account Manager.

## Platform notes

- **macOS** — on first Start the OS prompts for the **Accessibility** permission (System Settings → Privacy & Security → Accessibility). Input simulation does nothing until it is granted.
- **Linux** — works on X11 and on Wayland compositors that allow input simulation; on a restricted Wayland session the input backend may be unavailable (logged to stderr, the app stays running).
- **Windows** — no extra setup.

## Icons

`src-tauri/icons/` holds placeholder icons. To replace them, add an `app-icon.svg` (1024×1024) and run `pnpm tauri icon app-icon.svg`.

## Process name

Each launch generates a random 15-character process name, also used for the window title and tray tooltip. The text inside the window keeps the `Emploidle` name.

- **Windows** — the launcher briefly runs as `emploidle.exe`, starts a copy with a random `.exe` filename under the temporary directory's `emploidle-processes` folder, then exits. The temporary copy omits the version resource so Task Manager uses its random filename; the original executable retains its metadata. Arguments and the working directory are preserved. Unlocked copies from earlier runs are removed on the next launch. The final copy remains until a later launch or temporary-file cleanup. Both debug and release builds run without a console window. The interface uses WebView2, whose browser, renderer and utility processes appear under the application in Task Manager.
- **Linux** — changes the kernel process name through `/proc/self/comm`, visible in tools such as `ps -o comm`. The executable path and command line retain their original names.
- **macOS** — changes the kernel process name through `kern.procname`, preserving the application bundle and its signature. Views based on bundle metadata or command-line arguments may still show `Emploidle`.

If setting the random name fails, the application exits before starting its activity worker or window.
