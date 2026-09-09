# OpenCast

A small, open-source Windows launcher for finding files and doing quick calculations. Written entirely in Rust, with a native GPU-rendered interface. Inspired by Raycast; an independent project with its own code and assets.

![OpenCast file search](docs/file-search.png)

![OpenCast actions](docs/actions.png)

The compact palette uses a single search row, inline file types, and a searchable actions panel. Windows uses Segoe UI and native acrylic blur where supported; the screenshots show the tinted Linux fallback.

## Install on Windows

1. Open [Releases](https://github.com/AverWasTaken/opencast/releases).
2. Download `OpenCast-0.1.0-windows-x64-setup.exe` and run it.
3. Launch **OpenCast** from Start or the desktop shortcut. The Start menu shortcut also registers **Ctrl+Alt+O** with Windows.

The installer targets Windows 10/11 x64, installs for your user without administrator rights, and includes an uninstaller in Windows Settings → Apps. No Rust, terminal, browser runtime, or account is needed. A portable executable is also included in releases. Initial builds are unsigned, so Windows may show a publisher/SmartScreen warning.

Documents, Downloads and Desktop are indexed on first launch when available. Use **Settings → Add folder** to include other locations. Uninstalling preserves your local settings and index; delete `%LOCALAPPDATA%\OpenCast\OpenCast\data` if you also want to remove them.

## What works

- Filename and path search, with exact, prefix, substring, multiple-word and fuzzy filename matching.
- Background indexing; cached startup; automatic refresh every 60 seconds and a manual rebuild action.
- Recent files when the search field is empty; open files in their default applications, open containing folders, and copy paths.
- Offline arithmetic with parentheses, powers, constants and functions: `(24 + 8) / 2`, `2^8`, `sqrt(144)`.
- Unit conversions: `10 km to mi`, `72 f to c`, `5 kg to lb`, `90 min to h`, `1 GiB to MiB`. Expressions also work: `(2 + 3) kg to g`.
- Windows folder picker, per-user installation, app icon and Start menu shortcut.

| Shortcut | Action |
| --- | --- |
| ↑ / ↓ | Select a file |
| Enter | Open selected file or copy calculation |
| Ctrl+Shift+C | Copy selected path or calculation |
| Ctrl+K | Actions |
| Ctrl+, | Search settings |
| Escape | Dismiss a panel, clear the query, then close the app |

Click a file to select it; double-click to open. Choose **Actions → Search files only** to search for a filename that looks like a calculation. Calculator errors are shown inline. On macOS source builds, Command replaces Ctrl.

## Supported units

| Dimension | Units |
| --- | --- |
| Length | mm, cm, m, km, in, ft, yd, mi |
| Mass | mg, g, kg, oz, lb |
| Time | ms, s, min, h, d |
| Volume | ml, l, gal (US liquid gallon) |
| Temperature | c / °c, f / °f, k |
| Data | B, KB, MB, GB, TB (decimal); KiB, MiB, GiB (binary) |
| Angle | deg, rad |

Use `to` or `in` between units. Units are case-insensitive; `b` means bytes, not bits. Arithmetic uses floating-point numbers and displays up to eight decimal places. Currency, compound units, natural-language dates and percentages are outside this MVP.

## Local by design

OpenCast reads directory entries and file metadata, never file contents. There is no telemetry or network service. The local index contains file names and paths; it is not encrypted. Hidden entries, symlinks, AppData, `.git`, `node_modules`, and `target` are excluded. Gitignored files are included. Unreadable entries are skipped and counted in the index status tooltip.

Search uses an in-memory snapshot with a bounded top-40 ranking heap. Scanning and searching run on separate threads; an existing snapshot remains searchable during refresh. Updates become visible when the scan completes. The JSON cache is disposable and regenerated if missing or invalid.

## Development

Install the current stable [Rust toolchain](https://rustup.rs/). Windows development also needs Visual Studio Build Tools with **Desktop development with C++**.

```sh
cargo run --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --locked --release
```

Linux source builds need a desktop session, OpenGL and X11/Wayland runtime libraries (including `libxkbcommon-x11-0` for X11). Windows is the supported distribution target; macOS and Linux installers are not provided.

Build the Windows installer using [Inno Setup 6](https://jrsoftware.org/isinfo.php):

```powershell
cargo build --locked --release
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" packaging/windows/opencast.iss
```

The `Build and test` workflow checks Linux and Windows and uploads a Windows installer artifact. Pushing a version tag matching `Cargo.toml` (e.g. `v0.1.0`) runs the release workflow and publishes a prerelease with installer, portable executable and SHA-256 checksums.

## MVP boundaries

This is a file launcher and calculator, not an extension platform yet. It does not index file contents, launch installed apps by name, watch changes instantly, run in the tray, enforce a single instance, or update itself. Ctrl+Alt+O is a Windows shortcut that launches the program, not a resident show/hide hotkey. Initial indexing time and memory use depend on the selected folders. Very large trees and network drives can take longer to refresh.

## Contributing

Small, focused pull requests are welcome. Include tests for changes to search ranking, indexing or calculator semantics, and a screenshot for interface changes. Run the checks above before opening a PR. See [the architecture notes](docs/architecture.md) for the main modules.

MIT licensed. See [LICENSE](LICENSE).
