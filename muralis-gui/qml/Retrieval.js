// Presentation logic for retrieval: which sources are searched, which are
// browsed, what a category selection asks the CLI for, and what the grid says
// when it has nothing to show. Pure functions, no QML state — exercised
// headless by muralis-gui/tests/tst_retrieval.qml.
.pragma library

// A source's declared retrieval mode. A source that declares nothing is
// searched, matching the trait default.
var BROWSED = "browsed"

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

// A categorised browsed source retrieves nothing until a category is named —
// browsing "everything" is not a slice it offers.
function needsCategory(sourceList, name) {
    return categoriesOf(sourceList, name).length > 0
}

// The CLI argv for one retrieval. Which verb answers follows from the source's
// declared retrieval mode, never from its type name. Returns null when the
// request is not one the CLI can answer — a categorised browsed source with no
// category picked yet — so the call site simply issues nothing.
function args(sourceList, req) {
    var page = ["--page", String(req.page), "--per-page", String(req.perPage)]
    var aspect = req.aspect && req.aspect !== "all" ? ["--aspect", req.aspect] : []

    if (isBrowsedName(sourceList, req.source)) {
        var category = []
        if (needsCategory(sourceList, req.source)) {
            if (!req.category) return null
            category = ["--category", req.category]
        }
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

// The display label for a category slug, falling back to the slug itself so an
// unlabelled category is still nameable.
function labelOf(sourceList, sourceName, slug) {
    var cats = categoriesOf(sourceList, sourceName)
    for (var i = 0; i < cats.length; i++)
        if (cats[i].slug === slug) return cats[i].label || cats[i].slug
    return slug
}

// What the grid shows when it is not showing wallpapers. Loading, awaiting a
// category, empty and failed are four distinct states: an empty category names
// itself, and a failed retrieval says so rather than passing for an empty one.
function gridMessage(sourceList, state) {
    if (state.loading) return { text: "", isError: false }
    if (state.error) return { text: state.error, isError: true }

    if (needsCategory(sourceList, state.source) && !state.category)
        return { text: "Select a category to browse " + state.source, isError: false }

    if (state.resultCount > 0) return { text: "", isError: false }
    if (!state.retrieved) return { text: "Search for wallpapers to get started", isError: false }

    if (state.category)
        return { text: "No wallpapers in " + labelOf(sourceList, state.source, state.category),
                 isError: false }
    if (isBrowsedName(sourceList, state.source))
        return { text: "Nothing to show from " + state.source, isError: false }
    return { text: "No results for this search", isError: false }
}
