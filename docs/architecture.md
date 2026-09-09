# Architecture

`src/main.rs` creates the native eframe/egui window. `src/ui.rs` owns UI state, keyboard actions, configuration and two background workers. `src/index.rs` implements metadata scanning, the disk snapshot and ranked search; `src/calculator.rs` implements expression evaluation and dimension-aware conversions.

The scanner publishes an immutable `Arc<Snapshot>` through a short-held `RwLock`. Searches clone that pointer and release the lock before evaluating a query. Search requests are coalesced, and the UI rejects responses for an outdated query. Scanning never holds the snapshot lock while walking directories or writing the cache. Results are bounded to 40, with exact names first, then prefixes, substrings, path tokens and fuzzy names. Modification time breaks ranking ties.

The UI thread handles no directory walking or search. Configuration writes are small synchronous operations; a native folder dialog on Windows is modal. File opening delegates directly to the operating system through the `open` crate, without constructing shell commands. File contents and calculator input are never executed.

The scanner refreshes at startup and 60 seconds after each scan; a manual rebuild wakes it early. A rebuild requested while scanning is queued. Permission failures are skipped. Cached data is used only when its roots match the current configuration. A corrupt cache is rebuilt. The cache replacement removes the previous file before renaming on Windows; interruption in that window loses only the cache.

Future work: filesystem notifications with full-scan recovery, single-instance activation and resident global hotkey, code signing and automatic updates, full accessibility review, and visual checks on a real Windows desktop. Keep extension execution separate from the trusted search/calculator core when that feature is introduced.

The Windows renderer uses wgpu with Direct3D 12, including software adapter fallback; Linux development uses OpenGL. Windows acrylic is applied through the native window handle, with a tinted opaque fallback if unsupported. The result list uses 44-pixel row spacing below a 60-pixel search header. Actions contain search modes and settings, keeping the main palette free of tabs and branding.
