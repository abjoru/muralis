# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build                          # build all crates
cargo build -p muralis-cli           # build single crate
cargo test                           # run all tests
cargo test -p muralis-core           # test single crate
cargo test -p muralis-core config    # run tests matching "config"
cargo clippy --workspace             # lint all crates
cargo fmt --all -- --check           # check formatting
cargo fmt --all                      # apply formatting
cargo run -p muralis-cli -- status   # run CLI
cargo run -p muralis-daemon          # run daemon
cargo run -p muralis-gui             # run GUI
```

## Architecture

Cargo workspace with 8 crates:

- **muralis-core** — Shared library. Traits (`WallpaperSource`, `WallpaperBackend`), models, config, DB, IPC, cache, paths, `SourceRegistry`.
- **muralis-cli** — CLI binary (clap). Sends IPC requests to daemon.
- **muralis-daemon** — Background service. Runs display engine, IPC server, system tray, workspace listener as tokio tasks communicating via mpsc channels.
- **muralis-gui** — Iced (Elm architecture) GUI. Dynamic tabs built from `SourceRegistry`.
- **muralis-source-wallhaven** — Wallhaven API source plugin.
- **muralis-source-unsplash** — Unsplash API source plugin.
- **muralis-source-pexels** — Pexels API source plugin.
- **muralis-source-feed** — RSS/Atom feed source plugin (one instance per configured feed).

### Plugin Architecture

Sources are independent crates implementing `WallpaperSource` trait. Each exports:
```rust
pub fn create_sources(table: &toml::Table) -> Vec<Box<dyn WallpaperSource>>
```
The GUI builds a `SourceRegistry` at startup by calling each crate's `create_sources()`. Tabs and search are fully dynamic — no hardcoded source names in the GUI.

**Adding a new source:**
1. Create `muralis-source-foo/` with `WallpaperSource` impl
2. Add to workspace members in root `Cargo.toml`
3. Add dep in `muralis-gui/Cargo.toml`
4. Add 1 line in `build_registry()` in `muralis-gui/src/app.rs`
5. Add `[sources.foo]` section to user's config.toml

### Key Types

- `SourceType` (models.rs) — String newtype for source identification. Stored as TEXT in SQLite.
- `WallpaperSource` (sources/mod.rs) — Trait with `name()` (display), `source_type()` (DB key), `search()`, `download()`.
- `SourceRegistry` (sources/mod.rs) — Holds `Vec<Box<dyn WallpaperSource>>`, lookup by name.
- `WallpaperBackend` (backend/mod.rs) — Wallpaper setters: `HyprpaperBackend`, `SwwwBackend`. Factory: `create_backend()`.
- `Config.sources` — `toml::Table` (raw TOML). Each source crate deserializes its own section.

### Daemon Task Model

4 concurrent tokio tasks spawned in main.rs:
1. **Tray** — ksni/SNI D-Bus system tray
2. **Workspace listener** — Hyprland event stream → engine
3. **IPC server** — Unix socket, JSON newline-delimited protocol
4. **Display engine** — Main loop with `tokio::select!`, handles rotation/scheduling/mode transitions

### IPC Protocol

Unix socket at `/tmp/muralis-{uid}.sock`. `IpcRequest`/`IpcResponse` enums serialized as JSON, newline-delimited. Client in `ipc.rs`, server in daemon's `ipc.rs`.

### Display Modes

`DisplayMode` enum: `Static`, `Random`, `Sequential`, `Workspace` (per-Hyprland-workspace), `Schedule` (time-of-day).

### Data Flow

Search results → `WallpaperPreview` (transient). Favoriting downloads full image → SHA-256 hash for dedup → saved to wallpapers dir → metadata in SQLite DB → `Wallpaper` (persistent).

## Key Design Decisions

- Wayland-only (hyprpaper/swww backends)
- SHA-256 content hashing for cross-source dedup
- SQLite with WAL mode, foreign keys enabled
- XDG Base Directory compliant (`dirs` crate)
- Async-first with tokio throughout all binaries
- Config: `~/.config/muralis/config.toml`
- DB: `~/.local/share/muralis/muralis.db`
- Cache: `~/.cache/muralis/{thumbnails,previews}/`
