# Muralis DMS widget

A [DankMaterialShell](https://github.com/AvengeMedia/DankMaterialShell) widget:
a DankBar pill showing the current wallpaper, and a popout with the muralis
favorites grid plus transport and mode controls.

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

Hide DMS's built-in wallpaper tab (it browses a directory, not favorites) by
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
| `muralis favorites list` | grid contents (JSON array) |
| `muralis status` | current wallpaper, mode, paused, count |
| `muralis set <id>` | click-to-set |
| `muralis next` / `muralis prev` | transport |
| `muralis pause` / `muralis resume` | rotation toggle |
| `muralis mode <mode>` | static, random, random_startup, sequential, workspace, schedule |

`favorites list` entries carry `id`, `source_type`, `source_id`, `source_url`,
`width`, `height`, `tags`, `file_path`, `added_at`, `last_used`, `use_count`.

`status` returns `current_wallpaper`, `mode`, `next_change`, `paused`,
`running`, `wallpaper_count`.

Thumbnails are read directly from `~/.cache/muralis/thumbnails/<id>_thumb.jpg`,
one per favorite — not through the CLI.

On set, the widget also calls `SessionData.setWallpaper(file_path)` so the rest
of DMS (lock screen, blurred backgrounds, matugen theming) sees the change.

## Scope

First cut is parity with the old WallpaperTab patch: favorites grid with
thumbnails and paging, click-to-set, next/prev, pause/resume, mode switch,
SessionData sync, and selection following `current_wallpaper`.

Search, tag filtering, per-monitor wallpapers, favorite/unfavorite and source
browsing are deliberately out of the first cut.
