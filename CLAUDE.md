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

Cargo workspace with 4 crates:

- **muralis-core** — Shared library. Traits, models, config, DB, IPC, cache, paths.
- **muralis-cli** — CLI binary (clap). Sends IPC requests to daemon.
- **muralis-daemon** — Background service. Runs display engine, IPC server, system tray, workspace listener as tokio tasks communicating via mpsc channels.
- **muralis-gui** — Iced (Elm architecture) GUI. Tabs for favorites + each wallpaper source.

### Key Traits

- `WallpaperBackend` (backend/mod.rs) — Wallpaper setters: `HyprpaperBackend`, `SwwwBackend`. Factory: `create_backend()`.
- `WallpaperSource` (sources/mod.rs) — Async API clients: `WallhavenClient`, `UnsplashClient`, `PexelsClient`, `FeedClient`. Factory: `create_sources()`.

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
