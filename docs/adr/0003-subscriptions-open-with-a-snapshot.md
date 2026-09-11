---
status: accepted
---

# A subscription carries whole rows and opens with a snapshot

The `Subscribe` request on the daemon socket is held open and answered with
newline-delimited **Daemon events**, starting with a **Snapshot** of current
state. Each event is tagged (`{"event": …}`) and self-sufficient —
`wallpaper_changed` carries the whole `Wallpaper` row.

This settles the three questions [#9](../../issues/9) was labelled
`ready-for-human` for: framing, payload, and what a subscriber sees on connect.

## Framing: one handler, one accept path

`handle_connection` reads its one request line as before and branches on
`Subscribe` instead of dispatching. Everything else keeps the write / hang-up
shape that scripts and keybinds depend on, and there is no second listener to
keep in step with the first — the socket, the parse and the error reply are
written once.

The subscriber's `broadcast::Receiver` is taken *before* the request line is
read. An event fired between parsing `subscribe` and subscribing would fall in
the gap between the snapshot and the stream and be lost for good; subscribing
first can at worst duplicate an event the snapshot also carries, and a Consumer
re-applying the wallpaper it already has is harmless where missing one is not.

## Payload: the whole row, tagged

Rejected: a flat `{id, file_path}`. It is the stated minimum, but a Consumer
wanting dimensions or tags for a label has to chase the event with a request —
and an event you must chase is not a push.

Rejected: reusing `DaemonStatus` as the event body. It has no `file_path`,
which is precisely what `SessionData.setWallpaper()` needs, and it carries
derived fields (`next_change`, a countdown) that are nonsense frozen into an
event.

A `Wallpaper` row is ~445 B. At one event per rotation that cost is not worth
a second round-trip to avoid.

The tag buys room: `mode_changed` and `pause_changed` ship alongside
`wallpaper_changed` precisely because adding a kind later is
backwards-compatible while changing a shipped one is not, and a Consumer that
matches on `event` ignores kinds it does not know.

## Snapshot on connect

Without it a subscriber learns nothing until the next rotation — up to the whole
rotation interval of staleness on a fresh connection, and worse after a
reconnect: `DankSocket` redials with backoff up to 15 s, and anything that
changed in that window would be missed permanently. That is the same class of
defect the subscription exists to fix, so the stream opens with current state
rather than pushing the problem onto every Consumer.

The same snapshot is the answer to a lagging subscriber. `tokio::sync::broadcast`
drops events on behalf of a receiver that falls behind, which leaves it stale in
ways it cannot detect; on `Lagged` the daemon resends the snapshot instead of
carrying on from a gap. A slow Consumer is never disconnected for being slow,
and a slow Consumer can never stall the engine — `broadcast::send` does not
block, so a subscriber that stops reading parks its own task and nothing else.

## Consequences

`current_wallpaper: Option<String>` on the engine becomes
`current: Option<Wallpaper>`, written only by `set_current` — the chokepoint
[#9](../../issues/9) asked for, now load-bearing rather than tidiness: the
snapshot answers with a `file_path` without going back to the database. Folding
`last_error = None` into it also fixes a real bug, where a successful
`SetWallpaper` left a stale failure showing in `status`.

`DaemonStatus` is unchanged. A Consumer can still poll, and the **CLI contract**
is untouched — a `muralis subscribe` command for shell scripts remains
worth having and is not needed by any Consumer under
[ADR 0002](0002-consumers-speak-the-daemon-socket.md).
