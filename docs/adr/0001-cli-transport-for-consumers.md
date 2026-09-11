---
status: accepted
---

# Consumers drive muralis through the CLI, not the daemon socket

A **Consumer** (first: the **DMS Widget**) spawns `muralis` as a subprocess and
parses its JSON, rather than connecting to the daemon's Unix socket at
`/tmp/muralis-$UID.sock`. The socket exists and speaks newline-delimited JSON,
so this looks like the long way round. It isn't: the socket cannot serve the
favorites grid at all, and the CLI is the seam we already promise to keep
stable.

## Considered options

**Speak the socket directly from the Consumer.** Rejected for v1. `IpcRequest`
has no favorites variant — `favorites list` opens SQLite directly in the CLI
process — so a socket-only Consumer cannot draw its grid. It would speak two
protocols and still spawn a process for the most expensive call.

**Extend the IPC protocol first, then speak the socket.** Rejected — but see
the amendment below, which adopts the push half of it via a streaming CLI
command rather than socket access. Consumers speaking the socket directly stays
rejected.

**Spawn the CLI for everything.** Chosen. One transport, one failure mode in the
Consumer, no protocol framing re-implemented in a foreign codebase. It also
matches how DankMaterialShell's own shell behaves: its QML instantiates no raw
socket anywhere, shelling out to its Go binary even for niri and sway, and using
socket paths only as presence probes.

## Consequences

The **CLI contract** spans two backing stores — the DB for favorites, the socket
for status and mutations — so a Consumer sees a working grid and a failing
status when the daemon is down. Consumers must render that split, not treat
daemon-down as all-or-nothing.

Swapping transport later does not disturb Consumer UI, provided each Consumer
keeps command invocation behind the function that loads its model.

## Amendment: streaming commands are part of this decision

Originally this ADR accepted that Consumers cannot be told about wallpaper
changes they did not initiate, since the socket is one-shot. That was priced as
a stale label in the DMS Widget's bar pill and accepted for v1.

It was mispriced. A Consumer syncing a wallpaper change into DankMaterialShell
calls `SessionData.setWallpaper()`, which regenerates the shell's entire matugen
palette from the image. "No push" therefore means the desktop's accent colours
silently stop matching the wallpaper after every timed rotation — a visible
defect, not a cosmetic limitation. See [#9](https://github.com/abjoru/muralis/issues/9).

The fix does not overturn this decision, it extends it: push arrives as a
streaming `muralis subscribe` command writing newline-delimited JSON to stdout,
not as socket access from the Consumer. Consumers still drive muralis through
the CLI. A long-lived child process with a line parser on stdout is how
DankMaterialShell consumes every other stream it has, so this stays inside both
codebases' existing patterns.

The corollary is that **the CLI contract includes streaming commands, not only
request/response ones** — and that a Consumer holding a long-lived child process
owes it a reconnect policy, since a daemon restart kills the stream and a
Consumer that fails to restart it goes deaf exactly like the polling design this
replaces.
