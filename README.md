<p align="center">
  <img src="assets/muralis.svg" alt="Muralis Logo" width="300">
</p>

<h1 align="center">Muralis</h1>

<p align="center">Wallpaper manager for Hyprland with multi-source search, favorites, and display modes</p>

## Status

![Version](https://img.shields.io/badge/version-0.1.8-blue)
![Status](https://img.shields.io/badge/status-beta-yellow)

**Beta Release** - Core functionality complete and stable. All major features implemented and tested.

## Features

- **Multi-Source Search**: Wallhaven, Unsplash, Pexels, and RSS/Atom feeds
- **Plugin Architecture**: Add new sources by implementing a single trait
- **Display Modes**: Static, Random, Sequential, Workspace-aware, Scheduled
- **Favorites System**: SHA-256 deduplication, SQLite metadata, persistent library
- **GUI**: Qt6/QML browser with source chips, feed dropdown, thumbnail grid, preview drawer
- **Daemon**: Background service with IPC and workspace listener
- **CLI**: Full daemon control via Unix socket
- **Wayland-Native**: hyprpaper and swww backends with transitions

## Installation

### Prerequisites

- **Wayland compositor**: Hyprland
- **Wallpaper backend**: [swww](https://github.com/LGFae/swww) or [hyprpaper](https://github.com/hyprwm/hyprpaper)
- **SQLite**: `sqlite` (Arch) or `libsqlite3-dev` (Debian/Ubuntu)
- **Qt6**: `qt6-base qt6-declarative` (Arch)

### Option 1: AUR Package (Arch Linux)

```bash
paru -S muralis
```

### Option 2: Building from Source

```bash
git clone https://github.com/abjoru/muralis
cd muralis

make
sudo make install DESTDIR=/
```

### Hyprland Autostart

Add to your Hyprland config:

```conf
exec-once = swww-daemon        # or hyprpaper
exec-once = muralis-daemon
```

## Usage

### Daemon

```bash
muralis-daemon

# With debug logging
RUST_LOG=debug muralis-daemon
```

The daemon spawns 3 concurrent tasks:
- IPC server (Unix socket)
- Display engine (wallpaper rotation/scheduling)
- Workspace listener (Hyprland events)

### CLI

```bash
muralis status              # Show daemon status
muralis next                # Next wallpaper
muralis prev                # Previous wallpaper
muralis set <id>            # Set specific wallpaper
muralis mode random         # Switch display mode
muralis pause               # Pause rotation
muralis resume              # Resume rotation
muralis reload              # Reload config
muralis favorites list      # List all favorites (JSON)
muralis favorites stats     # Show favorites count and disk usage
muralis cache stats         # Show cache size
muralis cache prune         # Prune cache to configured max
muralis quit                # Stop daemon
```

### GUI

```bash
muralis-gui                 # Launch wallpaper browser
```

The GUI provides:
- Source chips for API sources, dropdown for feed sources
- Thumbnail grid with adaptive columns
- Preview drawer with metadata and actions
- One-click favoriting (downloads full image, deduplicates by SHA-256)
- Keyboard-driven navigation (grid/search/preview modes)

## Configuration

User config: `~/.config/muralis/config.toml`

### General

```toml
[general]
backend = "swww"          # "swww" or "hyprpaper"
cache_max_mb = 500        # Max cache size in MB
```

### Display

```toml
[display]
mode = "random"           # static, random, random_startup, sequential, workspace, schedule
interval = "30m"          # Rotation interval (e.g., "15m", "1h")
min_resolution = "auto"   # Minimum resolution or "auto"
aspect_ratio = "auto"     # Target aspect ratio (e.g., "16:9") or "auto"

[display.transition]      # swww only (hyprpaper ignores)
type = "fade"             # Transition type
duration = 2.0            # Duration in seconds
fps = 60
```

### Display Modes

| Mode | Description |
|------|-------------|
| `static` | Single wallpaper, no rotation |
| `random` | Random wallpaper every interval |
| `random_startup` | Random wallpaper on startup only |
| `sequential` | Cycle through wallpapers in order |
| `workspace` | Per-Hyprland-workspace wallpapers |
| `schedule` | Time-of-day based selection |

### Filter

```toml
[filter]
min_width = 2560
min_height = 1440
exclude_tags = ["anime", "cartoon"]
```

### Sources

Sources are plugin-based. Each source has its own config section:

```toml
[sources.wallhaven]
enabled = true
api_key = "optional"        # Required for NSFW/sketchy
categories = "111"          # General/Anime/People
purity = "100"              # SFW/Sketchy/NSFW

[sources.unsplash]
enabled = true
access_key = "your_key"

[sources.pexels]
enabled = true

[[sources.feeds]]
name = "Bing Daily"
url = "https://example.com/feed.rss"
enabled = true
```

### Workspace Mode

```toml
[[workspaces]]
workspace = 1
wallpaper = "nature"

[[workspaces]]
workspace = 2
wallpaper = "urban"
```

### Schedule Mode

```toml
[[schedules]]
time = "08:00"
tags = ["bright", "morning"]

[[schedules]]
time = "22:00"
tags = ["dark", "night"]
```

## Architecture

```
muralis/
├── muralis-core/              # Shared library (traits, models, config, DB, IPC)
├── muralis-cli/               # CLI binary (clap)
├── muralis-daemon/            # Background service (IPC, display engine)
├── muralis-gui/               # Qt6/QML GUI (C++ + QML, calls CLI via QProcess)
├── muralis-source-wallhaven/  # Wallhaven API plugin
├── muralis-source-unsplash/   # Unsplash API plugin
├── muralis-source-pexels/     # Pexels API plugin
└── muralis-source-feed/       # RSS/Atom feed plugin
```

### Adding a New Source

1. Create `muralis-source-foo/` implementing `WallpaperSource` trait
2. Export `pub fn create_sources(table: &toml::Table) -> Vec<Box<dyn WallpaperSource>>`
3. Add to workspace `Cargo.toml`
4. Add one line in `build_registry()` in `muralis-cli/src/main.rs`
5. Add `[sources.foo]` to config

### Data Paths

| Purpose | Path |
|---------|------|
| Config | `~/.config/muralis/config.toml` |
| Database | `~/.local/share/muralis/muralis.db` |
| Wallpapers | `~/.local/share/muralis/wallpapers/` |
| Thumbnails | `~/.cache/muralis/thumbnails/` |
| Previews | `~/.cache/muralis/previews/` |
| IPC socket | `/tmp/muralis-{uid}.sock` |

## Hyprland Integration

Bind CLI commands to keys in your Hyprland config:

```conf
bind = $mainMod SHIFT, N, exec, muralis next
bind = $mainMod SHIFT, P, exec, muralis prev
bind = $mainMod SHIFT, W, exec, muralis-gui
```

## Development

```bash
make                                 # Build Rust crates + QML GUI
cargo test                           # Run all tests
cargo clippy --workspace             # Lint
cargo fmt --all -- --check           # Check formatting
cargo run -p muralis-cli -- status   # Run CLI
cargo run -p muralis-daemon          # Run daemon
./muralis-gui/build/muralis-gui      # Run GUI
```

## License

MIT License - see [LICENSE](LICENSE) for details

Copyright (c) 2026 Andreas Bjoru
