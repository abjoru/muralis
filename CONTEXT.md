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
The per-source data a **RestSource** is built from: auth strategy, search/detail endpoint paths, page-param name + index base, upstream page cap, URL→id parser, block size B, and the response→**Preview** mapping.
_Avoid_: config (that is the user's TOML), spec

**Auth** (strategy):
How a **RestSource** injects credentials. Variants: `QueryParam` (one key, wallhaven `apikey`), `Header` (unsplash/pexels), `QueryParams` (multiple query credentials together — Gelbooru's `api_key`+`user_id`, now mandatory since 2025), `None`. Secrets live here, never in `extra_query`.

**Feed Source**:
The non-REST **Source** backed by RSS/Atom; not a **RestSource** (no paged JSON API, fetches image dimensions itself).

**Booru Source**:
A **RestSource** for the danbooru-family imageboards (Danbooru, Moebooru = yande.re/Konachan, Gelbooru, e621…). One crate (`muralis-source-booru`), many hosts: configured as multiple instances (like **Feed Source**), each instance naming a host plus a **Flavor**. Tag-based search; dimensions present in the JSON. The NSFW/anime track. Aspect filtering is client-side **Page-filling** over a **Block** (no server aspect param) — boorus express aspect inside the `tags` string and cap unauthenticated searches at 2 tags, so the tag budget is spent on `rating:` + the user query, not aspect.
_Avoid_: anime source, imageboard plugin (reserve "plugin" for the crate)

**Flavor**:
The API dialect of a **Booru Source** instance (`danbooru`, `moebooru`, `gelbooru`, …) — selects one **Source Descriptor** (endpoint path, page-param + index base, JSON envelope, detail-request shape). Multiple hosts share one Flavor (yande.re and Konachan are both `moebooru`). Distinct from **source_type**: Flavor is the dialect, source_type is the per-host identity.

**source_type (booru)**:
A **Booru Source** instance's stable per-host identity (e.g. `yandere`, `konachan`, `danbooru`, `gelbooru`), set from its config `name`. It is the dedup key alongside `source_id` — unlike the **Feed Source** (all feeds share `source_type="feed"` because feed `source_id`s are globally-unique URLs). Booru `source_id`s are bare host-local post numbers, so each host needs its own source_type or post `123` from two hosts would collide on `(source_id, source)`. `display_name` is cosmetic; `source_type` is identity.

### Pixabay

**Pixabay Source**:
A one-host **RestSource** (own crate, like wallhaven). Minimal config: `enabled` + `api_key`. `orientation=horizontal` is a constant `extra_query` (landscape server-side, like pexels/unsplash), with **Page-filling** for exact ultrawide. Uniquely, it pushes the global `[filter] min_width/min_height` into its server-side `min_width`/`min_height` params (no other source can). Safe content only ever reaches "moderate" — Pixabay hosts no explicit tier — so the global ceiling maps to `safesearch` true/false.

### Content safety

**Content Safety policy**:
A single global setting governing how much explicit content any **Source** may surface — not booru-specific. Cross-cutting: every Source that *can* return adult content maps the global policy onto its own native control (wallhaven → `purity`, **Booru Source** → `rating:` tag, pixabay → `safesearch`). Sources with no adult content (the SFW photo APIs, most feeds) ignore it. Reaching all plugins requires widening the `create_sources` plugin contract to receive global context, not just the `[sources]` table.

**SourceContext**:
The global cross-cutting context passed to every plugin's `create_sources` (`muralis-core`). Holds the **Content Safety policy** ceiling and global `min_width`/`min_height`. Designed to absorb future global knobs without re-churning the plugin contract. Plugins see this + their `[sources]` table, never the whole `Config`.
_Avoid_: nsfw flag (it is a graded policy, not a boolean), rating (reserve for the booru tag)

The policy is an **ordered level** — `safe` < `moderate` < `nsfw` — acting as a **ceiling**, not an override. Effective safety per source = the stricter of (global policy, source's native control). Global `safe` (the default) forces every source to its safest setting regardless of native config; global `nsfw` lets each source's native control (wallhaven `purity`, booru `rating`) decide up to that cap. A native knob can only tighten below the ceiling, never loosen past it — so a stale `purity="111"` cannot leak NSFW under a `safe` global.

### Search & paging

**Preview**:
A transient search result (`WallpaperPreview`) — not in the **Library**, not on
disk.
_Avoid_: result, thumbnail, image

**Wallpaper**:
A **Preview** that has been kept: downloaded to disk and recorded as a row in
`wallpapers`. The persisted counterpart to a Preview, carrying usage history
(`last_used`, `use_count`) a Preview has no place for.
_Avoid_: favorite (see **Library**), image, file

**Library**:
Every **Wallpaper** on disk — the set the daemon rotates through and a
**Consumer** displays. There is no curated subset within it: the schema has no
favorite flag, so being in the Library *is* being kept, and `muralis favorites
list` returns the whole thing. The CLI spelling is historical and stays (it is
part of the **CLI contract**); our own prose says Library, so nothing implies a
filter that does not exist.
_Avoid_: favorites (the command name, not the concept), collection, gallery

**Page-filling**:
A **RestSource** consuming a fixed block of B upstream API pages for one logical page, applying the aspect filter as it goes, returning up to `per_page` matches. Best-effort *within the block*: a logical page may return fewer than `per_page` even when more matches exist upstream.
_Avoid_: over-fetch, pagination

**Block (B)**:
The fixed number of upstream API pages a **RestSource** maps to one logical page. Logical page N covers upstream pages `(N-1)*B+1 .. N*B`. Per-source in the **Source Descriptor**. Server-filtering sources (wallhaven) use B=1; post-filtered sources use B>1.

### Transport

**HttpFetch**:
The transport seam behind **RestSource**, at bytes level: `get(url, headers, query) -> (StatusCode, Bytes)`. Lives in `muralis-source-common`. The real adapter wraps `reqwest`; the test adapter returns canned bytes and asserts the auth header and pagination params. Auth injection and JSON parsing live in **RestSource**, not behind this seam.
_Avoid_: HttpClient, Transport, Fetcher

### Consumers

**DMS Widget**:
The DankBar surface (in `dms-widget/`) that drives muralis from
DankMaterialShell — a **Consumer**, not a **Source**. It adds no wallpapers; it
reads favorites and drives rotation through the CLI. DMS's own vocabulary calls
this a *plugin* (`plugin.json`, `dms plugins install`); inside this repo that
word stays reserved for a source crate, so the directory and all our prose say
*widget*.
_Avoid_: DMS plugin, wallpaper tab (it replaced one; it is not one)

**Consumer**:
Anything outside the workspace that drives muralis through the **IPC contract**
rather than linking `muralis-core`. The **DMS Widget** is the first. A Consumer
is a client of the contract and never a **Source**. Consumers are not the
audience for the **CLI contract** — that seam serves scripts, keybinds and
humans.

**IPC contract**:
The daemon socket protocol a **Consumer** depends on: the `IpcRequest` /
`IpcResponse` variants and the event stream a subscriber holds open. Being a
contract is what distinguishes it from the daemon's internals — variant names
and response field names are a compatibility promise, not free to churn. It
carries two shapes: request/response, and a subscription the Consumer keeps
open and reconnects to.
_Avoid_: API (reserve for a remote **Source**'s HTTP API)

**CLI contract**:
The subset of `muralis` CLI commands and their JSON output that scripts,
keybinds and humans depend on. Distinct from the **IPC contract** in audience,
not just in wire format: the CLI keeps promises the IPC contract does not, such
as `favorites list` answering from the database with the daemon down. A
**Consumer** does not use it.
_Avoid_: API, treating it as the Consumer seam (that is the **IPC contract**)

## Relationships

- A **RestSource** is built from exactly one **Source Descriptor** and one **HttpFetch** adapter.
- A **RestSource** produces zero or more **Previews** per logical page via **Page-filling** over a **Block** of upstream pages.
- The **Feed Source** is a **Source** but never a **RestSource**.
- The `SourceRegistry` holds **Sources** (any mix of **RestSource**, **Booru Source**, **Pixabay Source**, **Feed Source**).
- `create_sources` takes a **SourceContext** (global cross-cutting knobs: **Content Safety policy**, `min_width`/`min_height`) in addition to the `[sources]` table + client. The contract is the same for every plugin.
- A gelbooru **Flavor** instance points at any gelbooru-clone host by `base` (gelbooru, rule34, safebooru, realbooru) — multi-host for free.
- A **Preview** becomes a **Wallpaper** when kept; the **Library** is every Wallpaper. Nothing distinguishes Wallpapers within the Library — there is no favorite flag.
- A **Consumer** (e.g. the **DMS Widget**) depends only on the **IPC contract**; it never links `muralis-core` and never registers as a **Source**.

## Example dialogue

> **Dev:** "When the GUI asks the **RestSource** for page 2 at 21:9, does it resume where page 1 stopped?"
> **Maintainer:** "No — paging is stateless. Page 2 is the next **Block** of upstream pages. **Page-filling** runs inside that block; if only three match, you get three. We accept the leak — the grid is infinite-scroll, not random-access. A logical page that returns zero matches is the CLI's end-of-results signal; `search` still returns a plain `Vec`."

## Flagged ambiguities

- "filter" was used for both the user's aspect choice and the act of discarding non-matching **Previews** — resolved: the choice is `AspectRatioFilter`; the act is part of **Page-filling**.
- aspect filtering previously lived in the CLI caller (`main.rs:230`); it now lives in each **Source**: **RestSource** filters via **Page-filling**, and the **Feed Source** filters after resolving dimensions. The CLI no longer post-filters at all — every **Source** honors the aspect contract itself.
- URL ownership for `favorites add` (`resolve_url`): the CLI tries every **Source** and takes the first that returns `Some`. With multiple **Booru Source** instances this is ambiguous (two moebooru hosts, same `/post/show/N` shape). Resolved: the engine's `resolve_url` host-matches the URL against the instance's `base` *before* delegating id extraction to the flavor — fixing a latent "first source that parses wins" ambiguity that predates booru. `parse_id` now extracts the id from the path only; the host check lives in the engine.
