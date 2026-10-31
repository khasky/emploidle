# Emploidle

Cross-platform anti-idle activity simulator. Choose allowed windows and press Start to run reading and reviewing scenarios. Local text previews and a temporary practice document support more varied activity without editing external documents or opening messages.

## What it does when running

- **Scenarios** stay with a window for 90–240 seconds, with reading pauses, occasional longer breaks and a gradually changing pace. Window memory returns to previously selected sources.
- **Cursor movement** follows curved paths with acceleration, deceleration and small corrections. Where accessibility exposes text rows, hovering targets those rows.
- **External reading** never sends navigation or editing keys. It only hovers over the already foreground window. External scrolling cannot guarantee restoration when the user scrolls or the document changes concurrently, so scrolling is confined to local previews and the practice document.
- **Local preview** captures up to 3,000 characters exposed by an allowed source and displays them in a read-only Emploidle window. When the browser exposes its address bar, the domain identifies Notion, Jira, GitHub/GitLab, Gmail, Google Calendar and Slack/Discord views. This classification does not navigate the original service or enumerate its documents, tickets, repositories, messages or channels.
- **Practice document** supports clicks, caret movement and selection through native input. Its own page animates typing, occasional corrected typos and gradual deletion, restoring the original text, selection and scroll position. It can use a short passage captured from a selected source. Clipboard contents are not changed. Text remains local and is discarded when the document closes.
- **User control** interrupts actions when you use the keyboard or mouse and resumes after 8 seconds without physical input. Ctrl + Alt + Escape stops activity globally (Control + Option + Escape on macOS).

Choose windows under Activity windows, assign Reading or Local preview, optionally enable Practice document, then press Start. Start hides the main window to the tray. Opening it while activity is running shows Paused; closing or minimizing it to the tray resumes activity. Stop remains available while paused.

Refresh the list after opening or closing an application. Settings remember the selected application and exact window title; native window IDs are resolved again after launch. If the source is unavailable or a window loses focus unexpectedly, activity stops.

External tabs, comments, drafts, messages and IDE workspace navigation are not opened or edited. Erasing temporary input cannot undo server autosave or read markers, so typing and selection are confined to local documents. Only currently accessible content can be previewed; this is not a browser extension or a service API integration.

Exit from the tray menu → Quit. The tray menu also has Start / Stop / Show window.

## Schedule and startup

Schedule uses the system's local time. The default 00:00–00:00 disables scheduling; other start and finish times define a daily shift, including shifts across midnight. Emploidle must remain open in the tray for its schedule to run. A manual Stop suppresses the rest of the current shift; the next shift can start automatically. Manual Start outside a shift runs until Stop or the next scheduled finish.

Start with system is off by default. When enabled, Emploidle launches in the background at user login through HKCU Run on Windows, a LaunchAgent on macOS or XDG autostart on Linux. Windows registration uses the original installed executable, not its temporary renamed process copy. Settings and source bindings are stored in the application's config directory; captured document text is not saved there.

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

Text snapshots currently require Windows. macOS and Linux support hovering and the practice document.

- **macOS** — enable **Accessibility** and **Input Monitoring** for the application under System Settings → Privacy & Security. Window selection requires Accessibility; Start requests Input Monitoring when needed. Secure Input can restrict monitoring in protected applications.
- **Linux** — requires an X11 session with XInput 2 and XTEST devices. Wayland is unsupported because global window control and physical input monitoring cannot be guaranteed through XWayland.
- **Windows** — source capture uses the built-in Windows PowerShell UI Automation provider. Some applications do not expose suitable text. Windows can refuse to focus an application or protected applications can reject simulated input; the app reports this and stops.

## Icons

The application and tray icon comes from [`app-icon.svg`](app-icon.svg), a 1024×1024 vector source. Generate icons with `pnpm tauri icon app-icon.svg --output src-tauri/target/app-icons`, then copy `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.ico`, and `icon.icns` into `src-tauri/icons/`.

## Process name

Each launch generates a random 15-character process name, also used for the window title and tray tooltip. The text inside the window keeps the `Emploidle` name.

- **Windows** — the launcher briefly runs as `emploidle.exe`, starts a copy with a random `.exe` filename under the temporary directory's `emploidle-processes` folder, then exits. The temporary copy omits the version resource so Task Manager uses its random filename; the original executable retains its metadata. Arguments and the working directory are preserved. Unlocked copies from earlier runs are removed on the next launch. The final copy remains until a later launch or temporary-file cleanup. Both debug and release builds run without a console window. The interface uses WebView2, whose browser, renderer and utility processes appear under the application in Task Manager.
- **Linux** — changes the kernel process name through `/proc/self/comm`, visible in tools such as `ps -o comm`. The executable path and command line retain their original names.
- **macOS** — changes the kernel process name through `kern.procname`, preserving the application bundle and its signature. Views based on bundle metadata or command-line arguments may still show `Emploidle`.

If setting the random name fails, the application exits before starting its activity worker or window.
