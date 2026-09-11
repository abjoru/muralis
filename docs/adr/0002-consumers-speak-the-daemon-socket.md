---
status: accepted
supersedes: ADR-0001
---

# Consumers speak the daemon socket via DankSocket

A **Consumer** connects to the daemon's Unix socket at `/tmp/muralis-$UID.sock`
and speaks the **IPC contract** directly, rather than spawning `muralis` per
action. This reverses [ADR 0001](0001-cli-transport-for-consumers.md), which was
decided on a false premise.

## Why 0001 was wrong

It rested on two claims. One was a matter of fact and was simply incorrect:
DankMaterialShell's QML does speak raw sockets, through its own `DankSocket`
wrapper (`dank-qml-common/DankCommon/Common/DankSocket.qml`), re-exported by DMS
in `quickshell/Common/` and reachable by plugins via the supported `qs.Common`
import. `MangoService` alone runs three of them.

DankSocket is not a thin shim. It carries exponential backoff with jitter
(400 ms base, 15 s cap), allocates a fresh `Socket` per attempt to work around
Quickshell's inability to redial a failed one, and its `send()` JSON-encodes and
newline-terminates every message — which is already muralis's exact wire format.
With `parser: SplitParser` on the read side, there is no framing left for a
Consumer to write.

So 0001's "no protocol framing re-implemented in a foreign codebase" argued for
the CLI on work that does not exist, and its stale-pill consequence would have
had us hand-roll a reconnect loop markedly worse than the one already shipped.

## What was actually load-bearing

0001's other claim was true and is what kept it alive: `IpcRequest` had no
favorites variant, so a socket-only Consumer could not draw its grid. That is a
missing protocol feature, not a reason to choose a transport — so we add it
(#10) rather than route around it.

## Consequences

Consumers need both halves of the protocol work before they can ship: the
`Subscribe` event stream (#9) and the `Favorites` request (#10). Neither is
large; the `set_current()` chokepoint in #9 was owed regardless.

The **CLI contract** does not go away. It remains the seam for scripts, keybinds
and humans, and keeps its own promise — notably that `favorites list` answers
from the database even with the daemon down. Consumers simply are not its
audience.

A Consumer now has **one** failure mode rather than two. Under 0001 the grid
worked while status failed, and every Consumer had to render that split; now
"the daemon is not running" is a single honest state. The cost is that the grid
no longer works daemon-down — deliberate, since a half-working widget is worse
than one that says what is wrong.

Reconnection is no longer ours to design. Backoff, jitter and redial belong to
DankSocket, and a Consumer on another shell without an equivalent inherits that
work — the first non-DMS Consumer should re-examine this decision rather than
assume it.
