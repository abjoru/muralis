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

**Extend the IPC protocol first, then speak the socket.** Rejected as v1
sequencing, not on merit; it is the intended successor. See
[#9](https://github.com/abjoru/muralis/issues/9). Doing it first front-loads a
daemon change and JSON framing work before we know the widget's shape is right.

**Spawn the CLI for everything.** Chosen. One transport, one failure mode in the
Consumer, no protocol framing re-implemented in a foreign codebase. It also
matches how DankMaterialShell's own shell behaves: its QML instantiates no raw
socket anywhere, shelling out to its Go binary even for niri and sway, and using
socket paths only as presence probes.

## Consequences

The daemon cannot push. `send_request` writes one line, shuts down the writer,
reads one line, returns — there is no subscription. A Consumer therefore cannot
learn that a timed rotation changed the wallpaper, and the DMS Widget's bar pill
is knowingly stale between rotations in v1. Accepted deliberately: the
alternative is a timer spawning a process on an idle machine forever.

The **CLI contract** spans two backing stores — the DB for favorites, the socket
for status and mutations — so a Consumer sees a working grid and a failing
status when the daemon is down. Consumers must render that split, not treat
daemon-down as all-or-nothing.

Swapping transport later does not disturb Consumer UI, provided each Consumer
keeps command invocation behind the function that loads its model.
