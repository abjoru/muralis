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

## IPC contract

The widget speaks the daemon socket directly through `DankSocket` (`qs.Common`),
which supplies backoff, jitter, redial and the newline-JSON framing muralis
already uses. See ADR 0002.

| `IpcRequest` | Used for |
| --- | --- |
| `Favorites { offset, limit }` | grid contents: a window onto the library, or the whole thing when both are omitted |
| `Status` | current wallpaper, mode, paused, count |
| `SetWallpaper { id }` | click-to-set |
| `Next` / `Prev` | transport |
| `Pause` / `Resume` | rotation toggle |
| `SetMode { mode }` | static, random, random_startup, sequential, workspace, schedule — offered per `available_modes` ([#14](https://github.com/abjoru/muralis/issues/14)) |
| `Subscribe` | push: wallpaper-changed events, held open ([#9](https://github.com/abjoru/muralis/issues/9)) |

Every one of these is built. `Subscribe` ([#9](https://github.com/abjoru/muralis/issues/9))
was the last, and is what the widget holds open for the life of the session.

### Mode switching

`workspace` and `schedule` only function when `config.toml` declares workspaces
or schedules, and the widget is not a config editor. It offers the modes the
daemon reports as usable in `available_modes` ([#14](https://github.com/abjoru/muralis/issues/14))
and greys out the rest, rather than presenting six equal choices of which two
may silently stop the wallpaper changing. `available_modes` carries the set, not
the reason — the missing precondition is always "nothing declared in
`config.toml`", so the widget writes that itself rather than the daemon
shipping prose for it. The daemon refuses an
unusable mode regardless ([#13](https://github.com/abjoru/muralis/issues/13)),
so the widget's filtering is a courtesy, not the safety net.

A mode chosen here persists to `config.toml`
([#12](https://github.com/abjoru/muralis/issues/12)), so it survives a daemon
restart.

`Favorites` answers with `wallpapers`, `total` and the `offset` it served, so the
grid can size its scrollbar without holding the whole library. Entries carry
`id`, `source_type`, `source_id`, `source_url`, `width`, `height`, `tags`,
`file_path`, `added_at`, `last_used`, `use_count` — ~445 B each, so the 16-item
page the grid draws is ~7 KiB where a 1000-wallpaper library would be ~435 KiB in
one socket line.

`Status` returns `current_wallpaper`, `mode`, `next_change`, `paused`,
`running`, `wallpaper_count`, `available_modes`, `last_error`.

`available_modes` is the modes this config can actually run, in
`DisplayMode::ALL` order — the switcher offers these and greys out the rest.

`last_error` is why the daemon's last apply failed, `null` once one succeeds
([#11](https://github.com/abjoru/muralis/issues/11)). It is what separates
"nothing applied yet" from "the backend refused" — the widget shows the reason
rather than inventing one.

Thumbnails are read directly from `~/.cache/muralis/thumbnails/<id>_thumb.jpg`,
one per Wallpaper — not through the CLI.

On every wallpaper change — the widget's own, and rotations arriving over
`Subscribe` — the widget calls `SessionData.setWallpaper(file_path)`. That is not
bookkeeping: it regenerates DMS's whole matugen palette from the image, which is
why push is a v1 blocker rather than a nicety. See ADR 0001.

Reconnection is not ours to design: `DankSocket` redials with exponential
backoff and jitter, capped at 15s, allocating a fresh socket per attempt.

### Two sockets

The contract has two shapes, so the widget opens two connections:

- **`eventSocket`** holds one `Subscribe` open for the life of the session. It
  carries a snapshot on connect — so a redial resynchronises without a separate
  request — and every change after. This is the one that keeps the matugen
  palette matching the wallpaper through rotations the widget did not initiate,
  which is the whole reason the widget exists rather than polling.
- **`commandSocket`** carries one-shot requests. The daemon answers a command
  and hangs up, so each gets a fresh dial; they queue and drain one at a time
  rather than racing onto a socket that is already closing. Commands here are
  always user-initiated, so the extra dial costs nothing anyone can perceive.

`SessionData.setWallpaper()` is called only when the path actually differs from
what DMS already has. Every redial replays the snapshot, and regenerating a
palette we already have is pure cost.

## Files

| File | Role |
| --- | --- |
| `plugin.json` | DMS manifest. `component` and `startupCheck` are the only fields that gate anything |
| `MuralisWidget.qml` | The `PluginComponent`: both sockets, all daemon state, both bar pills |
| `MuralisPopout.qml` | The popout view — grid, paging, transport, mode chips. Owns no socket; every value it draws is a property of the widget |
| `StartupCheck.qml` | Blocks activation when the `muralis` binary is absent |

## Failure states

Four states, three of which the widget must render rather than hide. Note the
daemon-down row is a deliberate regression from the CLI design: the grid used to
survive it by reading the database directly. One honest failure beats a
half-working widget — see ADR 0002.

| State | Socket | Widget |
| --- | --- | --- |
| muralis not installed | — | never loads — `StartupCheck.qml` blocks activation with an install hint |
| daemon down | connect fails, DankSocket retrying | one honest state: "muralis is not running". No grid — it lives behind the same socket |
| daemon up, nothing applied | connects, `current_wallpaper: null`, maybe `last_error` | says so explicitly, with `last_error` as the reason when there is one; grid usable, one click fixes it |
| empty library | connects, `wallpaper_count: 0` | empty-state pointing at `muralis search` |

Note `dependencies` in the manifest gates nothing — the schema calls it registry
metadata, and neither it nor its deprecated alias `requires` is enforced anywhere
in `PluginService.qml` or the installer. `startupCheck` is the only real gate.

The third row is a real state, not a hypothetical: muralis applies its
`random_startup` wallpaper exactly once, so losing the startup race with the
backend daemon left a wallpaper on screen that muralis did not know about
([#11](https://github.com/abjoru/muralis/issues/11)). Reporting "nothing
selected" there would read as the widget being broken, when the truth is that the
daemon applied nothing this session. The widget says which, because it is the
only component positioned to surface it. #11 made the state rare — the daemon now
waits for the backend before its startup apply — and no longer silent: a failure
that does happen arrives as `last_error`.

## Scope

First cut is parity with the old WallpaperTab patch: library grid with
thumbnails and paging, click-to-set, next/prev, pause/resume, mode switch,
SessionData sync, and selection following `current_wallpaper` — plus consuming
`Subscribe`, so the pill and the palette stay correct through timed
rotations the widget did not initiate.

Search, tag filtering, per-monitor wallpapers and source browsing are
deliberately out of the first cut. Search is affordable to omit only while the
library stays small — the grid pages 16 at a time over the *whole* library, since
muralis has no curated subset within it (see **Library** in `CONTEXT.md`).
