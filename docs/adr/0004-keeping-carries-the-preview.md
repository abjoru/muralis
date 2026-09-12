---
status: accepted
---

# Keeping carries the Preview; a URL is the human's path, not the GUI's

`muralis favorites keep` takes a whole **Preview** — the single-result JSON
object `search` and `browse` already emit — and keeps it without consulting any
**Source**'s `resolve_url`. `favorites add <url>` is unchanged and stays the
path for a pasted link.

This settles [#22](../../issues/22): keeping a result from a **Browsed Source**
was impossible, because keeping was expressed as "resolve this URL" and no
Browsed Source can resolve the `source_url` it publishes.

## Why URL resolution could not be made to work

`resolve_url` asks a Source to reconstruct a **Preview** from a URL. For a
**Searched Source** that is fine: its previews carry a per-image page URL its
own parser understands.

Neither Browsed Source can answer:

- the **Feed Source** publishes the post's page URL and implements no
  `resolve_url` at all;
- the **Ultrawide Source** publishes the **Category** page URL — one URL shared
  by every card on that page, naming a page rather than an image. Its
  `resolve_url` understands only a **Master** URL, and correctly declines its
  own `source_url`.

Rejected: teaching each Browsed Source to resolve its own `source_url`. A feed
post page would have to be fetched and scraped to find the image again, and a
category page URL identifies 23 images, not one — there is no answer to give.

Rejected: changing what `source_url` means, so it names an image. It is the
link-back the Ultrawide Source's access posture commits to, and the post a feed
entry came from; both are what a user wants the "source" of a wallpaper to be.

Rejected: keying the keep path on `source_type` + `source_id` alone. That is
enough for *identity* — it is what `is_favorited` matches on — but not enough to
*fetch*: `download()` takes a whole Preview and would have to rediscover
`full_url` from a lookup key that does not carry it.

## What the Preview path is

A **Consumer** holding a result already has every field a Preview has.
Serialising it down to a URL for a Source to parse back out is the round-trip
that fails; handing the Preview over skips it. The Source is found by the
`source_type` the Preview carries (`SourceRegistry::by_source_type`), and
`download()` takes the Preview as it always has.

Both paths then converge on `WallpaperManager::favorite`: same hash of the same
downloaded bytes, same **Library** row, same dedup. A wallpaper is
indistinguishable afterwards from one kept the other way, and keeping the same
image by both paths yields one row.

Several Sources can share a `source_type` — every **Feed Source** instance is
`feed`. That is not ambiguous here: every instance of a type fetches `full_url`
the same way. The identity that *would* be ambiguous, a **Booru Source**'s host,
already lives in `source_type`.

## A URL that names a page says so

A Source that recognises a URL as its own but cannot make an image of it now
explains itself (`WallpaperSource::explain_unresolvable`, defaulting to
silence), and `favorites add` prints that explanation instead of the bare "no
source could resolve URL". Pasting a category page and pasting a URL muralis has
never heard of are different mistakes and read differently. `resolve_url` itself
is unchanged, and no Browsed Source is obliged to implement it.

## Consequences

The GUI keeps through the Preview path for every result, browsed or searched —
it holds the Preview in either case and branches on nothing. `favorites add`
keeps working for a pasted link, including a **Master** URL, which is the only
way a human has to keep a specific image they found in a browser.

Adding a keep path to the **IPC contract** stays out of scope: no **Consumer**
keeps wallpapers, and the GUI is a CLI client
([ADR 0001](0001-cli-transport-for-consumers.md)).
