# Muralis DMS widget

A [DankMaterialShell](https://github.com/AvengeMedia/DankMaterialShell) widget:
a DankBar pill showing the current wallpaper, and a popout with the muralis
library grid plus transport and mode controls.

DMS calls this a *plugin* — hence `plugin.json`, and installation under
`~/.config/DankMaterialShell/plugins/`. This repo does not: `CONTEXT.md`
reserves "plugin" for a Rust source crate implementing `create_sources`. The
directory is named for DMS's own narrower word, `widget`, so nothing in the repo
root reads as a wallpaper source that isn't one.

Replaces the old approach of overwriting DMS's `DankDash/WallpaperTab.qml` in
`/usr/share`, which DMS 1.6 ended by embedding the whole UI in the `dms` binary.

## Why it lives in the muralis repo

The widget's entire contract is the muralis CLI's JSON output. Versioning them
apart is how that contract silently drifts. The DMS plugin registry supports
monorepo subdirectories natively — a registry entry carries `repo` plus an
optional `path`, and the installer clones into `plugins/.repos/<name>` then
installs `<repo>/<path>`. So living here costs nothing in distribution.

## Install

Until it is in a registry:

```bash
ln -s "$PWD/dms-widget" ~/.config/DankMaterialShell/plugins/muralis
```

Enable it in DMS Settings → Plugins, then add the widget to a DankBar section.

Hide DMS's built-in wallpaper tab (it browses a directory, not the library) by
setting `wallpaper` to disabled in `dashTabs` in
`~/.config/DankMaterialShell/settings.json`:

```json
"dashTabs": [
    { "id": "overview",  "enabled": true  },
    { "id": "media",     "enabled": true  },
    { "id": "wallpaper", "enabled": false },
    { "id": "weather",   "enabled": true  },
    { "id": "settings",  "enabled": true  }
]
```

`SettingsData.visibleDashTabIds()` reads this; the tab disappears from DankDash.

## CLI contract

Everything the widget needs, via `Proc.runCommand`:

| Command | Used for |
| --- | --- |
| `muralis favorites list` | grid contents: the whole library, JSON array |
| `muralis status` | current wallpaper, mode, paused, count |
| `muralis set <id>` | click-to-set |
| `muralis next` / `muralis prev` | transport |
| `muralis pause` / `muralis resume` | rotation toggle |
| `muralis mode <mode>` | static, random, random_startup, sequential, workspace, schedule |
| `muralis subscribe` | push: wallpaper-changed events, held open ([#9](https://github.com/abjoru/muralis/issues/9)) |

Entries carry `id`, `source_type`, `source_id`, `source_url`,
`width`, `height`, `tags`, `file_path`, `added_at`, `last_used`, `use_count`.

`status` returns `current_wallpaper`, `mode`, `next_change`, `paused`,
`running`, `wallpaper_count`.

Thumbnails are read directly from `~/.cache/muralis/thumbnails/<id>_thumb.jpg`,
one per Wallpaper — not through the CLI.

On every wallpaper change — the widget's own, and rotations arriving over
`subscribe` — the widget calls `SessionData.setWallpaper(file_path)`. That is not
bookkeeping: it regenerates DMS's whole matugen palette from the image, which is
why push is a v1 blocker rather than a nicety. See ADR 0001.

`subscribe` is a long-lived child process, so the widget owes it a reconnect
policy: a daemon restart kills the stream, and failing to restart it leaves the
widget silently deaf.

## Scope

First cut is parity with the old WallpaperTab patch: library grid with
thumbnails and paging, click-to-set, next/prev, pause/resume, mode switch,
SessionData sync, and selection following `current_wallpaper` — plus consuming
`muralis subscribe`, so the pill and the palette stay correct through timed
rotations the widget did not initiate.

Search, tag filtering, per-monitor wallpapers and source browsing are
deliberately out of the first cut. Search is affordable to omit only while the
library stays small — the grid pages 16 at a time over the *whole* library, since
muralis has no curated subset within it (see **Library** in `CONTEXT.md`).
