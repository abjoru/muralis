# Muralis Context

Domain and architecture language for muralis — a Wayland wallpaper manager that searches remote **Sources**, favorites images, and rotates them via a display **Backend**.

## Language

### Sources

**Source**:
A provider of wallpapers, implementing the `WallpaperSource` trait (`search`, `download`, `resolve_url`).
_Avoid_: provider, plugin (reserve "plugin" for the crate, not the running object)

**RestSource**:
The single concrete `WallpaperSource` for paged JSON REST APIs (wallhaven, unsplash, pexels), configured by a **Source Descriptor**. Owns auth, request building, pagination, **page-filling**, JSON parsing, and error context — the per-source crate supplies only response structs and their mapping to **Preview**. Lives in the `muralis-source-common` crate, not core.
_Avoid_: ApiClient, RestClient, HttpSource

**muralis-source-common**:
The crate holding **RestSource**, **HttpFetch**, **Source Descriptor**, and the `SearchResponse`/`DetailResponse` traits. Depends on `muralis-core`; the wallhaven/unsplash/pexels crates depend on it.

**Source Descriptor**:
The per-source data a **RestSource** is built from: auth strategy, search/detail endpoint paths, upstream page cap, URL→id parser, block size B, and the response→**Preview** mapping.
_Avoid_: config (that is the user's TOML), spec

**Feed Source**:
The non-REST **Source** backed by RSS/Atom; not a **RestSource** (no paged JSON API, fetches image dimensions itself).

### Search & paging

**Preview**:
A transient search result (`WallpaperPreview`) — not yet favorited, not on disk.
_Avoid_: result, thumbnail, image

**Page-filling**:
A **RestSource** consuming a fixed block of B upstream API pages for one logical page, applying the aspect filter as it goes, returning up to `per_page` matches. Best-effort *within the block*: a logical page may return fewer than `per_page` even when more matches exist upstream.
_Avoid_: over-fetch, pagination

**Block (B)**:
The fixed number of upstream API pages a **RestSource** maps to one logical page. Logical page N covers upstream pages `(N-1)*B+1 .. N*B`. Per-source in the **Source Descriptor**. Server-filtering sources (wallhaven) use B=1; post-filtered sources use B>1.

### Transport

**HttpFetch**:
The transport seam behind **RestSource**, at bytes level: `get(url, headers, query) -> (StatusCode, Bytes)`. Lives in `muralis-source-common`. The real adapter wraps `reqwest`; the test adapter returns canned bytes and asserts the auth header and pagination params. Auth injection and JSON parsing live in **RestSource**, not behind this seam.
_Avoid_: HttpClient, Transport, Fetcher

## Relationships

- A **RestSource** is built from exactly one **Source Descriptor** and one **HttpFetch** adapter.
- A **RestSource** produces zero or more **Previews** per logical page via **Page-filling** over a **Block** of upstream pages.
- The **Feed Source** is a **Source** but never a **RestSource**.
- The `SourceRegistry` holds **Sources** (any mix of **RestSource** and **Feed Source**).

## Example dialogue

> **Dev:** "When the GUI asks the **RestSource** for page 2 at 21:9, does it resume where page 1 stopped?"
> **Maintainer:** "No — paging is stateless. Page 2 is the next **Block** of upstream pages. **Page-filling** runs inside that block; if only three match, you get three. We accept the leak — the grid is infinite-scroll, not random-access. A logical page that returns zero matches is the CLI's end-of-results signal; `search` still returns a plain `Vec`."

## Flagged ambiguities

- "filter" was used for both the user's aspect choice and the act of discarding non-matching **Previews** — resolved: the choice is `AspectRatioFilter`; the act is part of **Page-filling**.
- aspect filtering previously lived in the CLI caller (`main.rs:230`); it now lives in each **Source**: **RestSource** filters via **Page-filling**, and the **Feed Source** filters after resolving dimensions. The CLI no longer post-filters at all — every **Source** honors the aspect contract itself.
