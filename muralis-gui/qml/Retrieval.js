// Presentation logic for retrieval: which sources are searched, which are
// browsed, what a category selection asks the CLI for, and what the grid says
// when it has nothing to show. Pure functions, no QML state — exercised
// headless by muralis-gui/tests/tst_retrieval.qml.
.pragma library

// A source's declared retrieval mode. A source that declares nothing is
// searched, matching the trait default.
var BROWSED = "browsed"

// How long the bar lets a selection settle before retrieving it. Several chips
// clicked in succession are one intent, and one request — the last selection
// is what must be reflected, not each step towards it.
var SETTLE_MS = 250

function isBrowsed(source) {
    return !!source && source.retrieval_mode === BROWSED
}

function searched(sourceList) {
    var out = []
    for (var i = 0; i < (sourceList || []).length; i++)
        if (!isBrowsed(sourceList[i])) out.push(sourceList[i])
    return out
}

function browsed(sourceList) {
    var out = []
    for (var i = 0; i < (sourceList || []).length; i++)
        if (isBrowsed(sourceList[i])) out.push(sourceList[i])
    return out
}

function find(sourceList, name) {
    for (var i = 0; i < (sourceList || []).length; i++)
        if (sourceList[i].name === name) return sourceList[i]
    return null
}

// The categories the named source publishes. Zero is meaningful: a feed is
// browsed and publishes none, so selecting the feed *is* the selection.
function categoriesOf(sourceList, name) {
    var source = find(sourceList, name)
    if (!isBrowsed(source)) return []
    return source.categories || []
}

function isBrowsedName(sourceList, name) {
    return isBrowsed(find(sourceList, name))
}

// Whether the named source's categories combine, as it declares. A source list
// that says nothing — an older CLI, a searched source — does not combine, the
// same default the trait takes.
function categoriesCombine(sourceList, name) {
    var source = find(sourceList, name)
    return !!(isBrowsed(source) && source.categories_combine)
}

// A selection reduced to what the source publishes, in the order it publishes
// it: duplicates collapse and anything unpublished drops. The same set picked
// either way round is then one selection, one request and one cache entry —
// matching what `select_categories` canonicalises to on the other side.
function canonicalSelection(published, slugs) {
    var out = []
    for (var i = 0; i < (published || []).length; i++)
        if ((slugs || []).indexOf(published[i].slug) >= 0) out.push(published[i].slug)
    return out
}

// The selection after picking one chip. Where the source's categories combine,
// picking toggles; where they do not, picking replaces — exactly the one-at-a
// time bar as it was. Which of the two applies is the source's declaration.
function toggleSelection(published, selection, slug, combines) {
    if (!combines) return [slug]

    var next = (selection || []).slice()
    var at = next.indexOf(slug)
    if (at >= 0) next.splice(at, 1)
    else next.push(slug)
    return canonicalSelection(published, next)
}

// Whether naming no category is itself a selection the named source answers,
// as it declares. A source list that says nothing — an older CLI, a searched
// source — does not, the same default the trait takes.
function emptySelectionIsMeaningful(sourceList, name) {
    var source = find(sourceList, name)
    return !!(isBrowsed(source) && source.empty_selection_is_meaningful)
}

// A categorised browsed source retrieves nothing until a category is named,
// *unless* it declares that naming none is a selection of its own — one with an
// untagged feed behind its categories answers the empty selection with
// everything. Which of the two applies is the source's declaration, never its
// type name: assuming every categorised source needs a category is what left
// clearing the bar showing a prompt instead of the feed.
function needsCategory(sourceList, name) {
    return categoriesOf(sourceList, name).length > 0
        && !emptySelectionIsMeaningful(sourceList, name)
}

// The CLI argv for one retrieval. Which verb answers follows from the source's
// declared retrieval mode, never from its type name. Returns null when the
// request is not one the CLI can answer — a categorised browsed source with no
// category picked yet — so the call site simply issues nothing.
function args(sourceList, req) {
    var page = ["--page", String(req.page), "--per-page", String(req.perPage)]
    var aspect = req.aspect && req.aspect !== "all" ? ["--aspect", req.aspect] : []

    if (isBrowsedName(sourceList, req.source)) {
        // Whether a source *takes* categories and whether it *needs* one are
        // two questions. A feed publishes none and takes none; a categorised
        // source takes whatever is selected, and only refuses an empty
        // selection when it has no feed behind its categories.
        var slugs = categoriesOf(sourceList, req.source).length > 0 ? (req.categories || []) : []
        if (slugs.length === 0 && needsCategory(sourceList, req.source)) return null

        var category = []
        // One occurrence per category: no delimiter convention, so a label
        // carrying a comma needs nothing escaped.
        for (var i = 0; i < slugs.length; i++) category.push("--category", slugs[i])
        return ["browse", req.source].concat(category, page, aspect)
    }

    var query = req.query && req.query.length > 0 ? [req.query] : []
    var source = req.source && req.source !== "All" ? ["--source", req.source] : []
    return ["search"].concat(query, source, page, aspect)
}

// The CLI argv for keeping one result. The whole Preview goes over, exactly as
// `search`/`browse` emitted it — a browsed result's source_url names the page
// it came from, which no source can resolve back to an image, and we hold every
// field already. Returns null when there is nothing to keep: no result, or one
// the library already has.
function keepArgs(item) {
    if (!item || item.is_favorited) return null
    return ["favorites", "keep", JSON.stringify(item)]
}

// The result at an index, or null when there is none. The Preview reads the
// grid's model through this rather than holding a result of its own, so one
// favorited state serves both surfaces.
function itemAt(results, idx) {
    if (!results || idx < 0 || idx >= results.length) return null
    return results[idx]
}

// A successful keep, applied to the model. The kept result is replaced rather
// than written through: a field set inside a JavaScript object notifies no QML
// binding, so the Preview would go on offering to keep what is already kept.
// An index naming no result leaves the model exactly as it was.
function markKept(results, idx) {
    if (!results || idx < 0 || idx >= results.length) return results
    var out = results.slice()
    var kept = Object.assign({}, results[idx])
    kept.is_favorited = true
    out[idx] = kept
    return out
}

// The display label for a category slug, falling back to the slug itself so an
// unlabelled category is still nameable.
function labelOf(sourceList, sourceName, slug) {
    var cats = categoriesOf(sourceList, sourceName)
    for (var i = 0; i < cats.length; i++)
        if (cats[i].slug === slug) return cats[i].label || cats[i].slug
    return slug
}

// A whole selection named for a human, in the order the chips read.
function selectionLabel(sourceList, sourceName, slugs) {
    var out = []
    for (var i = 0; i < (slugs || []).length; i++)
        out.push(labelOf(sourceList, sourceName, slugs[i]))
    return out.join(", ")
}

// What the grid shows when it is not showing wallpapers. Loading, awaiting a
// category, empty and failed are four distinct states: an empty category names
// itself, and a failed retrieval says so rather than passing for an empty one.
function gridMessage(sourceList, state) {
    if (state.loading) return { text: "", isError: false }
    if (state.error) return { text: state.error, isError: true }

    var selection = state.categories || []
    if (needsCategory(sourceList, state.source) && selection.length === 0)
        return { text: "Select a category to browse " + state.source, isError: false }

    if (state.resultCount > 0) return { text: "", isError: false }

    // A selection built from the keyboard exists before it is retrieved:
    // toggling is deliberately not committing, so the grid says what is waiting
    // rather than falling back to the prompt a searched source shows.
    if (!state.retrieved && selection.length > 0)
        return { text: "Press Enter to browse " + selectionLabel(sourceList, state.source, selection),
                 isError: false }
    // A browsed source is never searched, so it must never be told to search.
    if (!state.retrieved && isBrowsedName(sourceList, state.source))
        return { text: "Browsing " + state.source + "…", isError: false }
    if (!state.retrieved) return { text: "Search for wallpapers to get started", isError: false }

    // An intersection matching nothing names the whole selection: which
    // combination was empty is the only useful thing to say about it.
    if (selection.length > 0)
        return { text: "No wallpapers in " + selectionLabel(sourceList, state.source, selection),
                 isError: false }
    if (isBrowsedName(sourceList, state.source))
        return { text: "Nothing to show from " + state.source, isError: false }
    return { text: "No results for this search", isError: false }
}
