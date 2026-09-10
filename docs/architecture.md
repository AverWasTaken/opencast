# Architecture

`src/main.rs` creates the native eframe/egui window. `src/ui.rs` owns UI state, keyboard actions, configuration and two background workers. `src/index.rs` implements metadata scanning, the disk snapshot and ranked search; `src/calculator.rs` implements expression evaluation and dimension-aware conversions.

The scanner publishes an immutable `Arc<Snapshot>` through a short-held `RwLock`. Searches clone that pointer and release the lock before evaluating a query. Search requests are coalesced, and the UI rejects responses for an outdated query. Scanning never holds the snapshot lock while walking directories or writing the cache. Results are bounded to 40, with exact names first, then prefixes, substrings, path tokens and fuzzy names. Modification time breaks ranking ties.

The UI thread handles no directory walking or search. Configuration writes are small synchronous operations; a native folder dialog on Windows is modal. File opening delegates directly to the operating system through the `open` crate, without constructing shell commands. Calculator input is never executed. The Windows shell reads icon resources to resolve native icons.

The scanner refreshes at startup and 60 seconds after each scan; a manual rebuild wakes it early. A rebuild requested while scanning is queued. Permission failures are skipped. Cached data is used only when its roots match the current configuration. A corrupt cache is rebuilt. The cache replacement removes the previous file before renaming on Windows; interruption in that window loses only the cache.

Future work: filesystem notifications with full-scan recovery, code signing and automatic updates, full accessibility review, and visual checks on a real Windows desktop. Keep extension execution separate from the trusted search/calculator core when that feature is introduced.

The Windows renderer uses wgpu with Direct3D 12, including software adapter fallback; Linux development uses OpenGL. Windows acrylic is applied through the native window handle, with a tinted opaque fallback if unsupported. The result list uses 44-pixel row spacing below a 60-pixel search header. Actions contain search modes and settings, keeping the main palette free of tabs and branding.

`src/native.rs` owns the Windows integration. A named mutex prevents duplicate processes; subsequent invocations signal a hidden resident window, which runs on a dedicated Win32 message thread. That thread owns the tray icon and `RegisterHotKey` registrations. It can show the main HWND directly even while the renderer is asleep. Escape, activation and focus loss are handled by the UI; Quit is distinct from dismissing the launcher.

Shortcuts persist as modifiers plus a Windows virtual-key code. The recorder briefly suspends the active hotkey so its keys reach the input field, restores it after capture, then registers the candidate before releasing the old registration. A failed registration or config write retains/rolls back to the old shortcut. A tray menu provides a recovery path if a persisted shortcut is occupied on startup.

An independent COM-initialized icon worker calls `SHGetFileInfoW` on actual file paths. HICONs are rendered against black and white DIBs to reconstruct transparency, then destroyed; all GDI allocations are cleaned up. Requests are bounded and texture caching is capped at 512 entries. Application shortcuts from user and shared Start menu folders join the same metadata index.
