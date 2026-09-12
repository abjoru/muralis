---
status: accepted
---

# The GUI fetches like the CLI does: identified, and cached to disk

Every HTTP request the GUI originates goes out carrying the **muralis
User-Agent**, and every **Preview** thumbnail it renders resolves through the
**Preview thumbnail cache** on disk rather than off the network.

This settles [#23](../../issues/23): the GUI bound an `Image`'s `source`
straight to a remote URL, so Qt's own network stack did the fetching —
anonymously, and into a per-process memory cache that dies with the process.
Against ultrawidewallpapers.net that is an HTTP 429 for an anonymous request and
a 200 for the same request identified, so browsed thumbnails rendered blank.

## One definition, two build systems

The User-Agent's single definition is `assets/user-agent.tmpl`, a one-line
template with a `{version}` placeholder. `muralis-core::http::user_agent()`
renders it with the crate version; `muralis-gui/CMakeLists.txt` renders the same
template with the version read out of the workspace `Cargo.toml`, into a
generated header. Neither side holds a literal.

Rejected: a second literal in C++. That is the defect restated — the CLI and the
**Ultrawide Source** had already drifted into *two* spellings before this work,
and nothing noticed.

Rejected: asking the CLI for its User-Agent at GUI startup. It is one definition
too, but it makes an identification the GUI cannot state without spawning a
process, and leaves an undefined fallback for when that spawn fails.

## Identification belongs to the network layer

A `QQmlNetworkAccessManagerFactory` stamps the header on every request the QML
engine issues, so the drawer's full-resolution `Image` is covered without naming
it, and a future `Image` cannot silently go out anonymous. Redirects re-enter
the same path and stay identified — the site's thumbnail endpoint redirects
before it answers.

## The cache is the one the cache commands already see

`ThumbnailCache` writes into `~/.cache/muralis/thumbnails`, the directory
`cache stats` counts and `cache prune` trims. Nothing new accounts for it,
because the accounting was never the missing half — the writer was. Entries are
named by a SHA-256 of the whole URL, so two hosts' `/preview/pic.jpg` are two
entries, and they carry no `_thumb` suffix, which is the daemon's naming for a
kept **Wallpaper**'s thumbnail in the same directory. Files are written whole or
not at all: a torn file would be served as a broken image forever, since nothing
refetches what is already on disk.

Full-resolution drawer images are identified but not cached. They are large and
looked at once, and `cache prune` would be trimming them against thumbnails that
earn their space.

## Consequences

A card renders three distinguishable states rather than two: loading, ready, and
failed — a thumbnail the host refuses now reads as an error instead of a card
that never stops spinning. A failed fetch is retried on the next render, because
nothing was written.

The GUI's fetch path is asserted against a local test server
(`muralis-gui/tests/`), never a live site: `cargo test --workspace` and `ctest`
stay offline.
