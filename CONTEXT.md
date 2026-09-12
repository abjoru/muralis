# Muralis Context

Domain and architecture language for muralis — a Wayland wallpaper manager that searches remote **Sources**, favorites images, and rotates them via a display **Backend**.

## Language

### Sources

**Source**:
A provider of wallpapers, implementing the `WallpaperSource` trait (`search`, `browse`, `download`, `resolve_url`), and declaring a **Retrieval mode** that says which of `search`/`browse` actually answers.
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

**Retrieval mode**:
A declared property of every **Source**, not an inference from its type name: **Searched** or **Browsed**. It is what the `SourceRegistry` splits on, what `sources list` reports, and what decides which CLI verb answers. Defaulted to `Searched` on the trait, so the five API source crates declare nothing and behave exactly as before.
_Avoid_: source kind, browsable flag (it is two named modes, not a boolean on one of them)

**Searched Source**:
A **Source** that accepts a query. Participates in an unscoped `search` (`muralis search <query>` with no `--source`) and in the GUI's "All". Every **RestSource** is one.

**Browsed Source**:
A **Source** with no query dimension. Excluded from unscoped `search` and reachable only by naming it, through `muralis browse <source>`. Naming one on `search --source` is *refused* rather than answered: the **Feed Source** used to ignore the query it was handed and return its whole contents, so `muralis search sunset` interleaved every configured feed, unfiltered, with genuine matches. That is the defect the mode exists to remove — so a Browsed Source refuses a query instead of quietly discarding it.
_Avoid_: feed source (feeds are the first Browsed Source, not the only possible one), gallery source

**Category**:
A named, selectable slice of a **Browsed Source**'s catalog: a stable `slug` the caller passes and a human-readable `label` a UI renders. A Browsed Source publishing categories requires one to be named to retrieve anything — browsing "everything" is not a slice it offers — and a slug it does not publish is refused with the list of ones it does.
_Avoid_: tag (that is a **Preview**'s metadata), collection, section

**Zero categories**:
The **Feed Source**'s correct declaration, and a meaningful answer rather than an unfilled one: a feed is a single undifferentiated stream, so *selecting the feed is the selection*. Browsing it takes no category, and naming one is refused. A Browsed Source is therefore not obliged to have categories — publishing none is a statement about its shape, not a gap.

**browse (verb)**:
The **CLI contract**'s entry point to a **Browsed Source**: `muralis browse <source> [--category <slug>] [--page] [--per-page] [--aspect]`. It pages and aspect-filters exactly as `search` does and emits the *same JSON result shape*, `is_favorited` included, so every existing consumer of a search result works unchanged against a browse result. One function renders both.

**Feed Source**:
The non-REST **Source** backed by RSS/Atom; not a **RestSource** (no paged JSON API, fetches image dimensions itself). The first **Browsed Source**, declaring **Zero categories**. It has no paging dimension either: every entry the feed currently carries, aspect-filtered, is one page, so `page`/`per_page` do not slice it.

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

### Ultrawide

**Ultrawide Source**:
The **Browsed Source** for ultrawidewallpapers.net (`muralis-source-ultrawide`), and the first Source to publish **Categories**. Emphatically *not* a **RestSource**: the site has no REST/JSON API, no feed and no text search, so there is no paged JSON for a **Source Descriptor** to drive and no query dimension to expose. Its nearest sibling is the **Feed Source** — non-REST, parsing markup, browsed. One `[sources.ultrawide]` subsection, `enabled = false` by default, with an optional `categories` slug list replacing the **Shipped categories**.
_Avoid_: scraper source, ultrawide plugin (reserve "plugin" for the crate), UWW

**Category bar**:
The GUI's presentation of a **Browsed Source**'s **Categories**, below the filter bar, by display label. Shown only for a Browsed Source that publishes at least one — **Zero categories** shows no bar, because selecting the feed is already the selection. Selecting a category is what retrieves: it issues the **Browse verb** for that slug's first page, the way selecting a feed does for the feed. A Browsed Source is offered no search field and no aspect combo at all, since it has no query dimension to answer them with.
_Avoid_: category filter (it selects what is retrieved, it does not narrow a result set), tab bar

**Category page**:
The only navigational dimension ultrawidewallpapers.net has: one URL per slug, listing a fixed, curated set of cards. There is no pagination, no load-more and no detail page behind a card, so a **Category page** is the whole of what a **Category** can return. Logical `page`/`per_page` walk *that* parsed set and then return empty (the established end-of-results signal); the Source never reaches for a page the site does not publish. It is fetched once per user-initiated selection — never speculatively, never on a timer.

**Card**:
One entry on a **Category page**: an anchor to the full-resolution **Master** carrying its filename, wrapping the thumbnail `<img>`. Parsing yields one **Preview** per distinct filename (the site repeats a card across its carousel pages). A page yielding *no* card is an error naming the source and the category, never an empty result — an upstream markup change must be diagnosable instead of looking like an empty category.

**Master**:
The single 7680x2160 (32:9) image the site publishes per wallpaper; every other ratio it shows exists only inside its own server-side crop tool, whose resize endpoint serves whitelisted widths and 403s anything else. **Previews** therefore report 7680x2160 — the Master's *true* dimensions, never the thumbnail tag's `width`/`height`, which describe the thumbnail — and `download()` fetches the Master unaltered, so a kept **Wallpaper** is byte-identical to what the site serves. A user whose aspect filter excludes 32:9 correctly gets nothing here; crop-to-fit would be a Library-wide capability, not something one Source grows in private.

**source_type (ultrawide)**:
`ultrawide` — its own stable identity, because there are no detail pages and the **Master**'s filename is the natural `source_id`. A bare filename is far too generic to be unique across Sources, so the pair `(ultrawide, aishot-5774.jpg)` is what keys the **Library**. For the same reason `resolve_url()` matches the *host* (exactly: `ultrawidewallpapers.net` or `www.`-prefixed) **before** parsing an id out of a URL, and returns `None` otherwise — a filename-tailed path must not let this Source claim another's URL.

**Shipped categories**:
The eight-slug default the crate supplies when config names none. The site's sitemap lists ~88 slugs: shipping them all makes an unusable menu and goes stale silently, while scraping the nav at runtime costs a request per session and breaks quietly. Slugs come from config with defaults, so menu length and staleness are the user's call.

**Access posture**:
Part of the **Ultrawide Source**'s contract, not an optimisation: one page fetch per user action, thumbnails served from the local thumbnail cache rather than re-fetched per render (the Source itself never fetches a thumbnail — it passes the card's URL on), a muralis-identifying `User-Agent` on every outbound request, attribution plus a `source_url` link-back on every **Preview**, and no enumeration beyond what a **Category page** publicly lists. The site's terms restrict automated tools; this integration is a user-initiated renderer producing a browser's request volume, and these constraints are what keep that true as the feature evolves.

**Drift check**:
The weekly scheduled job that asks whether ultrawidewallpapers.net still parses (`.github/workflows/ultrawide-drift.yml`, driven by `muralis-source-ultrawide::check_live_category`). The **Card** fixtures are frozen on their capture date, so the offline suite passes however far the site moves — confidence grows exactly as accuracy decays. The check fetches one **Category page** and runs the *production* parser over it, never a reimplementation: a check with its own parsing logic can only disagree with the real one. It is emphatically not part of the suite, which stays offline and deterministic; it never runs on push or pull request, and its one fetch per week carries the **muralis User-Agent**, consistent with the **Access posture**. Only a page that arrived intact and yielded no Card is drift, and only drift is filed as an issue — a timeout, a 5xx and a rate limit are transient and say nothing about the markup. Success is silent, and refreshing a fixture stays a human decision, because a new capture is a new parser contract.
_Avoid_: smoke test (it monitors a third party, it does not exercise muralis end to end), scraper health check

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

**keep (verb)**:
Turning a **Preview** into a **Wallpaper**. Two entry points, one outcome: a
human pastes a link (`muralis favorites add <url>`, which asks every **Source**
to `resolve_url` it), and anything already holding a Preview hands it straight
over (`muralis favorites keep <result-json>`, reading stdin when the argument is
omitted). Both download through the Preview's **Source**, hash the bytes and
write one **Library** row, so what was kept is indistinguishable afterwards and
the same image kept twice is still one row. The Preview path exists because a
**Browsed Source**'s `source_url` names a *page*, not an image, and no URL
round-trip can recover one (ADR 0004).
_Avoid_: favorite (the command name, not the concept), save, download (that is
one step of it)

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

**muralis User-Agent**:
The one identification every request muralis originates carries, whichever
binary originates it — `muralis/<version> (wallpaper manager; +<repo url>)`.
Defined once, in `assets/user-agent.tmpl`: `muralis-core` renders it for the CLI
client and the **Sources**, CMake renders the same template for the GUI. A
second literal would drift, and the **Access posture** is a promise about what
goes on the wire, not about one crate. The GUI stamps it at its network layer,
so no `Image` can reintroduce an anonymous request.
_Avoid_: UA string, client identifier

**Preview thumbnail cache**:
The GUI's on-disk store of **Preview** thumbnails, in the same
`~/.cache/muralis/thumbnails` that `muralis cache stats` accounts for and
`muralis cache prune` trims. Keyed by a digest of the whole thumbnail URL, so
two images sharing a filename are two entries. A thumbnail is fetched once and
served locally on every render after, including after a restart — which is what
the **Access posture** means by serving thumbnails from a local cache. Distinct
from the **Library** thumbnails the daemon writes for kept **Wallpapers**
(`<id>_thumb.jpg`) alongside it. Full-resolution drawer images are identified
but never cached: large, and looked at once.
_Avoid_: image cache (Qt's own per-process one is what this replaces)

### Display

**Backend**:
The thing that puts a **Wallpaper** on screen, implementing `WallpaperBackend`
(`set_wallpaper`, `set_wallpaper_all`, `is_ready`). Each one drives a separate
long-running process of its own — awww-daemon, hyprpaper — which muralis does
not start and cannot assume is up.
_Avoid_: renderer, compositor (that is Hyprland), swww (that is one backend's
binary, and it is now named awww)

**Readiness probe**:
A **Backend** answering whether its process is accepting requests yet
(`awww query`, `hyprctl hyprpaper listactive`). Exists because the compositor
launches muralis and the backend together with no sequencing: the `random_startup`
apply is the only one of the session, so firing it into an unbound socket costs
the whole session's wallpaper. The daemon probes with bounded backoff
(`ReadinessPolicy`) before that apply, and only then.
_Avoid_: health check (it gates one apply, it does not monitor)

**last_error**:
The `DaemonStatus` field carrying why the last apply failed, cleared by the next
success. A backend failure used to be a `warn!` on a stderr nobody captures; this
is the same fact on the **IPC contract**, so a **Consumer** can show a wrong
screen as wrong.

**Current wallpaper**:
What is actually on screen, held by the daemon as the whole `Wallpaper` row
rather than its id. One function writes it (`DisplayEngine::set_current`), and
writing it is what clears **last_error** and emits a **Daemon event** — so a
timed rotation, a `SetWallpaper` and a workspace switch cannot disagree about
what "current" means or about who gets told. It had three independent
assignment sites before, and only two of them cleared the error.
_Avoid_: current_index (that is the rotation cursor, not what is displayed)

**Mode viability**:
Whether a display mode (`DisplayMode`) has the config it needs to do anything —
`schedule` needs at least one entry in `schedules`, `workspace` at least one in
`workspaces`; the four rotation modes need nothing. One predicate
(`Config::mode_unavailable_reason`) answers it, returning the reason a mode
cannot run. `SetMode` refuses on a reason instead of accepting a mode whose
handler would no-op forever, and the usable set a **Consumer** reads off
`status` (`DaemonStatus::available_modes`, built by `Config::available_modes`
as the modes answering `None`) is derived from the same call — encoding the
rule twice is how the refusal and the offer drift apart. The reason itself
stays on the refusal; `status` carries the set, not the prose.
_Avoid_: valid mode (a mode is well-formed either way; it is the config that is
missing), enabled

**Mode write-through**:
A `SetMode` landing in `config.toml` before it takes effect
(`Config::persist_mode`), so the running mode and the one the next daemon boots
into cannot disagree. The edit is surgical — only the `mode` value moves, and the
comments and spacing of a hand-written config survive — and a file that will not
parse is refused rather than overwritten. A write that fails fails the command:
a mode that works now and silently reverts at reboot is the thing being avoided.
Pause is deliberately *not* written through; pausing rotation reads as temporary
in a way that choosing a mode does not.
_Avoid_: save (`Config::save` reserialized the whole file and took the comments
with it; it is gone), autosave, sync

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
`IpcResponse` variants and the **Daemon events** a subscriber holds open. Being
a contract is what distinguishes it from the daemon's internals — variant names
and response field names are a compatibility promise, not free to churn. It
carries two shapes: request/response, and a **Subscription** the Consumer keeps
open and reconnects to.
_Avoid_: API (reserve for a remote **Source**'s HTTP API)

**Subscription**:
A **Consumer**'s `Subscribe` connection, held open while the daemon writes
**Daemon events** to it. The one request that gets no `IpcResponse` — the first
lines back are a **Snapshot**, and everything after is live. It exists because
the daemon cannot otherwise push: a timed rotation left the DMS Widget's matugen
palette matching a wallpaper that was no longer on screen, resyncing only when
the user happened to open the popout.
_Avoid_: watch, listener, poll (the point is that it is not polling)

**Daemon event**:
One line of a **Subscription**: `{"event": …}`, tagged the way `IpcRequest` is
tagged `command`, so a Consumer switches on one field. Three kinds:
`wallpaper_changed` (carrying the whole `Wallpaper` row), `mode_changed`,
`pause_changed`. An event is self-sufficient by design — `wallpaper_changed`
holds `file_path` because `SessionData.setWallpaper()` takes a path, and an
event a Consumer must chase with a `Status` is not a push. Adding a kind is
backwards-compatible; changing a shipped one is not. `wallpaper_changed` fires
only on a *successful* apply: a Consumer regenerates a whole palette from what
it is told, so announcing an image nobody can see is worse than announcing
nothing.
_Avoid_: message, notification, signal

**Snapshot**:
The daemon's current state rendered as the **Daemon events** that would have
produced it, written first on every **Subscription** and again after a lagging
subscriber is resynced. Without it a Consumer learns nothing until the next
rotation, and a reconnect after a `DankSocket` backoff would silently miss
whatever changed while it was away — which is exactly the defect a subscription
exists to fix.
_Avoid_: initial state, replay (nothing is replayed — it is current state, not
history)

**Favorites request**:
The **IPC contract**'s read of the **Library**: `Favorites { offset, limit }`,
answered with a `FavoritesPage` (`wallpapers`, `total`, `offset`). Both bounds
are optional and a bare `favorites` means the whole Library — the **CLI
contract**'s shape — while a **Consumer** drawing a grid asks for the window it
draws. Paging is on the wire because the numbers say so: a **Wallpaper** row is
~445 B of JSON, so a 1000-wallpaper Library is a ~435 KiB single socket line
against ~7 KiB for a 16-item page. The daemon reads the database per request
rather than serving its rotation cache, because `favorites add` writes the store
behind the daemon's back and a Consumer must see the wallpaper it just kept.
_Avoid_: library request (the command name is `Favorites`, historical like the
CLI's), listFavorites

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
- Every **Source** declares a **Retrieval mode**; `SourceRegistry::searched()` yields only the **Searched** ones, so the unscoped-search call site cannot forget to filter and let a **Browsed Source** leak into a query's answer.
- A **Browsed Source** publishes zero or more **Categories**; a **Searched Source** publishes none.
- The GUI partitions its filter bar on the declared **Retrieval mode**: **Searched Sources** are chips beside "All", **Browsed Sources** live in a selector, and a Browsed Source's **Categories** are a bar of their own below it. Nothing in the GUI branches on a **Source**'s type name.
- `browse` and `search` render **Previews** through one function, so their output shapes cannot drift apart.
- The `SourceRegistry` holds **Sources** (any mix of **RestSource**, **Booru Source**, **Pixabay Source**, **Feed Source**).
- `create_sources` takes a **SourceContext** (global cross-cutting knobs: **Content Safety policy**, `min_width`/`min_height`) in addition to the `[sources]` table + client. The contract is the same for every plugin.
- A gelbooru **Flavor** instance points at any gelbooru-clone host by `base` (gelbooru, rule34, safebooru, realbooru) — multi-host for free.
- A **Preview** becomes a **Wallpaper** when kept; the **Library** is every Wallpaper. Nothing distinguishes Wallpapers within the Library — there is no favorite flag.
- The GUI **keeps** by handing the whole **Preview** to the CLI, for every result alike — it never keeps by URL and never branches on **Retrieval mode** to decide. A **Source** is found for a Preview by its `source_type`, never by the name a UI renders.
- A **Consumer** (e.g. the **DMS Widget**) depends only on the **IPC contract**; it never links `muralis-core` and never registers as a **Source**.
- The **Library** has exactly one backing store behind both seams: the daemon answers the **Favorites request** from the database, and `favorites list` falls back to that same database only when the daemon cannot answer.
- Every **Daemon event** originates inside the daemon: a **Current wallpaper**
  write, a `SetMode` that took effect, or a pause toggle. A **Consumer** never
  emits one.
- A **Subscription** begins with a **Snapshot**, so a Consumer's state is a
  function of the connection alone — it never needs a `Status` round-trip to
  become current.
- A **Mode write-through** precedes the mode taking effect, so a refused or
  unwritable config leaves the daemon on the mode it already had.
- **Mode viability** is read from the `Config` alone — never from the daemon's running state — so `SetMode` and `status` cannot disagree about which modes are usable.

## Example dialogue

> **Dev:** "When the GUI asks the **RestSource** for page 2 at 21:9, does it resume where page 1 stopped?"
> **Maintainer:** "No — paging is stateless. Page 2 is the next **Block** of upstream pages. **Page-filling** runs inside that block; if only three match, you get three. We accept the leak — the grid is infinite-scroll, not random-access. A logical page that returns zero matches is the CLI's end-of-results signal; `search` still returns a plain `Vec`."

## Flagged ambiguities

- the searched/browsed distinction was already load-bearing before it was named: the GUI partitioned its filter bar by comparing `source_type` against the literal string `"feed"`, and avoided the unfiltered-feed defect only by never firing an empty-query "All" search. Resolved: it is a **Retrieval mode** declared by the Source and reported by `sources list`. The QML string test is now retired — the GUI partitions on the declared mode and reaches a Browsed Source through the **Browse verb**.
- which URL identifies a browsed **Preview** for `favorites add`: none does, and that was the wrong question. The **Feed Source** emits the post's page URL and implements no `resolve_url`; the **Ultrawide Source** emits the **Category page** URL, one URL shared by every card on it. Resolved: a Preview is identified by *being* a Preview — `favorites keep` takes the whole result object and never consults `resolve_url`, and the GUI keeps that way for every result, browsed or searched (ADR 0004). `favorites add <url>` is unchanged for a pasted link, including an Ultrawide **Master** URL, and a Source that recognises a URL it cannot make an image of now says so (`explain_unresolvable`) instead of leaving it indistinguishable from a URL nothing recognises.

- "filter" was used for both the user's aspect choice and the act of discarding non-matching **Previews** — resolved: the choice is `AspectRatioFilter`; the act is part of **Page-filling**.
- aspect filtering previously lived in the CLI caller (`main.rs:230`); it now lives in each **Source**: **RestSource** filters via **Page-filling**, and the **Feed Source** filters after resolving dimensions. The CLI no longer post-filters at all — every **Source** honors the aspect contract itself.
- URL ownership for `favorites add` (`resolve_url`): the CLI tries every **Source** and takes the first that returns `Some`. With multiple **Booru Source** instances this is ambiguous (two moebooru hosts, same `/post/show/N` shape). Resolved: the engine's `resolve_url` host-matches the URL against the instance's `base` *before* delegating id extraction to the flavor — fixing a latent "first source that parses wins" ambiguity that predates booru. `parse_id` now extracts the id from the path only; the host check lives in the engine.
