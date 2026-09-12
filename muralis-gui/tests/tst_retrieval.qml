import QtQuick
import QtTest
import "../qml/Retrieval.js" as Retrieval

TestCase {
    name: "Retrieval"

    // A browsed source is recognised by its declared retrieval mode, never by
    // its type name — ultrawide is browsed and is not a feed.
    function test_sources_partition_on_declared_retrieval_mode() {
        var sources = [
            { name: "Wallhaven", source_type: "wallhaven", retrieval_mode: "searched", categories: [] },
            { name: "Ultrawide", source_type: "ultrawide", retrieval_mode: "browsed",
              categories: [{ slug: "space", label: "Space" }] },
            { name: "Bing Daily", source_type: "feed", retrieval_mode: "browsed", categories: [] }
        ]

        compare(Retrieval.searched(sources).map(function (s) { return s.name }), ["Wallhaven"])
        compare(Retrieval.browsed(sources).map(function (s) { return s.name }), ["Ultrawide", "Bing Daily"])
    }

    // The category bar is driven by what the selected source publishes: a
    // categorised browsed source needs one named, a feed does not, and a
    // searched source has no categories at all.
    function test_only_a_categorised_browsed_source_needs_a_category() {
        var sources = fixture()

        compare(Retrieval.categoriesOf(sources, "Ultrawide").map(function (c) { return c.slug }),
                ["space", "nature", "dark"])
        compare(Retrieval.categoriesOf(sources, "Bing Daily"), [])
        compare(Retrieval.categoriesOf(sources, "Wallhaven"), [])
        compare(Retrieval.categoriesOf(sources, "All"), [])

        verify(Retrieval.needsCategory(sources, "Ultrawide"))
        verify(!Retrieval.needsCategory(sources, "Bing Daily"))
        verify(!Retrieval.needsCategory(sources, "Wallhaven"))
        verify(!Retrieval.needsCategory(sources, "All"))
    }

    // "All" and a searched source ask the search verb; the query and the
    // aspect are the dimensions it has.
    function test_a_searched_source_is_asked_with_the_search_verb() {
        var sources = fixture()

        compare(Retrieval.args(sources, { source: "All", query: "sunset", page: 1, perPage: 24, aspect: "all" }),
                ["search", "sunset", "--page", "1", "--per-page", "24"])

        compare(Retrieval.args(sources, { source: "Wallhaven", query: "sunset", page: 2, perPage: 24, aspect: "21x9" }),
                ["search", "sunset", "--source", "Wallhaven",
                 "--page", "2", "--per-page", "24", "--aspect", "21x9"])
    }

    // A browsed source is asked with the browse verb and is never handed a
    // query — it has no query dimension to answer one with.
    function test_a_browsed_source_is_asked_with_the_browse_verb() {
        var sources = fixture()

        compare(Retrieval.args(sources, { source: "Ultrawide", categories: ["space"], page: 1, perPage: 24, aspect: "all" }),
                ["browse", "Ultrawide", "--category", "space", "--page", "1", "--per-page", "24"])

        // A feed takes no category: selecting the feed is the selection.
        compare(Retrieval.args(sources, { source: "Bing Daily", categories: [], page: 1, perPage: 24, aspect: "all" }),
                ["browse", "Bing Daily", "--page", "1", "--per-page", "24"])

        // Paging stays inside the selected category.
        compare(Retrieval.args(sources, { source: "Ultrawide", categories: ["nature"], page: 3, perPage: 24, aspect: "all" }),
                ["browse", "Ultrawide", "--category", "nature", "--page", "3", "--per-page", "24"])
    }

    // A combined selection is one request naming every category: the option
    // repeats, one value per occurrence, so no label needs a delimiter escaped.
    function test_a_combined_selection_repeats_the_option() {
        compare(Retrieval.args(fixture(), { source: "Ultrawide", categories: ["space", "dark"],
                                            page: 1, perPage: 24, aspect: "all" }),
                ["browse", "Ultrawide", "--category", "space", "--category", "dark",
                 "--page", "1", "--per-page", "24"])
    }

    // A categorised browsed source with nothing selected yet is not a request
    // the CLI can answer, so none is issued.
    function test_a_categorised_source_without_a_category_issues_nothing() {
        compare(Retrieval.args(fixture(),
                               { source: "Ultrawide", categories: [], page: 1, perPage: 24, aspect: "all" }),
                null)
        // Clearing a selection lands here too, by the same route.
        compare(Retrieval.args(fixture(),
                               { source: "Ultrawide", page: 1, perPage: 24, aspect: "all" }),
                null)
    }

    // Whether picking adds to a selection or replaces it is the Source's
    // declaration, never an assumption about its type.
    function test_whether_categories_combine_is_read_from_the_source() {
        var sources = fixture()

        verify(Retrieval.categoriesCombine(sources, "Ultrawide"))
        verify(!Retrieval.categoriesCombine(sources, "Curated"))
        verify(!Retrieval.categoriesCombine(sources, "Bing Daily"))
        verify(!Retrieval.categoriesCombine(sources, "Wallhaven"))
        verify(!Retrieval.categoriesCombine(sources, "All"))
        // A source list from a CLI that predates the field declares nothing.
        verify(!Retrieval.categoriesCombine([{ name: "Old", retrieval_mode: "browsed",
                                               categories: [{ slug: "a" }] }], "Old"))
    }

    // Where categories combine, picking toggles: picking again removes.
    function test_picking_toggles_where_categories_combine() {
        var published = Retrieval.categoriesOf(fixture(), "Ultrawide")

        var one = Retrieval.toggleSelection(published, [], "space", true)
        compare(one, ["space"])

        var two = Retrieval.toggleSelection(published, one, "dark", true)
        compare(two, ["space", "dark"])

        compare(Retrieval.toggleSelection(published, two, "space", true), ["dark"])
        compare(Retrieval.toggleSelection(published, ["dark"], "dark", true), [])
    }

    // Where they do not, picking replaces — exactly as the bar behaved before
    // any of this, one selection at a time.
    function test_picking_replaces_where_categories_do_not_combine() {
        var published = Retrieval.categoriesOf(fixture(), "Curated")

        compare(Retrieval.toggleSelection(published, [], "week", false), ["week"])
        compare(Retrieval.toggleSelection(published, ["week"], "year", false), ["year"])
        // Picking the loaded one again reloads it rather than emptying the bar.
        compare(Retrieval.toggleSelection(published, ["week"], "week", false), ["week"])
    }

    // The same set picked in either order is one selection — and so one
    // request and one cache entry, matching what the CLI canonicalises to.
    function test_a_selection_is_canonicalised_to_the_published_order() {
        var published = Retrieval.categoriesOf(fixture(), "Ultrawide")

        var forwards = Retrieval.toggleSelection(
            published, Retrieval.toggleSelection(published, [], "space", true), "dark", true)
        var backwards = Retrieval.toggleSelection(
            published, Retrieval.toggleSelection(published, [], "dark", true), "space", true)

        compare(forwards, backwards)
        compare(forwards, ["space", "dark"])

        // Anything the source does not publish is not part of a selection.
        compare(Retrieval.canonicalSelection(published, ["dark", "volcanoes", "space"]),
                ["space", "dark"])
    }

    // The selection is nameable for a human, in the published order.
    function test_the_selection_names_itself_by_label() {
        var sources = fixture()

        compare(Retrieval.selectionLabel(sources, "Ultrawide", ["space", "dark"]), "Space, Dark")
        compare(Retrieval.selectionLabel(sources, "Ultrawide", []), "")
        // An unlabelled category is still nameable.
        compare(Retrieval.selectionLabel(sources, "Ultrawide", ["unknown"]), "unknown")
    }

    // Loading, empty and failed are three distinguishable states, and an empty
    // category says which category was empty.
    function test_the_grid_message_distinguishes_loading_empty_and_failed() {
        var sources = fixture()
        var browsing = { source: "Ultrawide", categories: ["space"], retrieved: true, resultCount: 0 }

        // Loading owns the grid; no message competes with the indicator.
        var loading = Retrieval.gridMessage(sources, { source: "Ultrawide", categories: ["space"],
                                                       loading: true, retrieved: true, resultCount: 0 })
        compare(loading.text, "")

        var empty = Retrieval.gridMessage(sources, browsing)
        verify(empty.text.indexOf("Space") >= 0, "empty state names the category: " + empty.text)
        verify(!empty.isError)

        var failed = Retrieval.gridMessage(sources, { source: "Ultrawide", categories: ["space"],
                                                      retrieved: true, resultCount: 0,
                                                      error: "no cards parsed on ultrawide/space" })
        verify(failed.isError)
        verify(failed.text.indexOf("no cards parsed") >= 0, failed.text)

        // Results on screen need no message at all.
        compare(Retrieval.gridMessage(sources, { source: "Ultrawide", categories: ["space"],
                                                 retrieved: true, resultCount: 24 }).text, "")
    }

    // An intersection matching nothing is the case a combining source hits
    // most: the empty state names the whole selection, not one of its parts,
    // and stays distinct from a failure.
    function test_an_empty_combination_names_every_category_in_it() {
        var sources = fixture()

        var empty = Retrieval.gridMessage(sources, { source: "Ultrawide", categories: ["space", "dark"],
                                                     retrieved: true, resultCount: 0 })
        verify(empty.text.indexOf("Space") >= 0, empty.text)
        verify(empty.text.indexOf("Dark") >= 0, empty.text)
        verify(!empty.isError)

        var failed = Retrieval.gridMessage(sources, { source: "Ultrawide", categories: ["space", "dark"],
                                                      retrieved: true, resultCount: 0,
                                                      error: "ultrawide returned 503" })
        verify(failed.isError)
        verify(failed.text !== empty.text, "a failure does not read as an empty intersection")
    }

    // Picking a categorised source is not yet a retrieval; the grid says what
    // is missing instead of looking like an empty result.
    function test_a_categorised_source_awaiting_a_category_says_so() {
        var msg = Retrieval.gridMessage(fixture(), { source: "Ultrawide", categories: [], retrieved: false,
                                                     resultCount: 0 })
        verify(msg.text.indexOf("categor") >= 0, msg.text)
        verify(msg.text.indexOf("Ultrawide") >= 0, msg.text)
        verify(!msg.isError)

        // Clearing a selection returns the grid to that same prompt rather
        // than to an empty result.
        var cleared = Retrieval.gridMessage(fixture(), { source: "Ultrawide", categories: [],
                                                         retrieved: true, resultCount: 0 })
        compare(cleared.text, msg.text)
        verify(!cleared.isError)
    }

    // Keeping hands the whole result over, browsed and searched alike: a
    // browsed result's source_url names the page it came from, which no source
    // can resolve back to an image. Nothing here branches on retrieval mode.
    function test_keeping_hands_the_whole_preview_over() {
        var browsed = { source_type: "ultrawide", source_id: "aishot-5793.jpg",
                        source_url: "https://www.ultrawidewallpapers.net/space-wallpapers",
                        thumbnail_url: "https://www.ultrawidewallpapers.net/thumb.php?image=a",
                        full_url: "https://www.ultrawidewallpapers.net/wallpapers/329/highres/a.jpg",
                        width: 7680, height: 2160, tags: ["Space Wallpapers"], is_favorited: false }

        var argv = Retrieval.keepArgs(browsed)

        compare(argv.length, 3)
        compare(argv[0], "favorites")
        compare(argv[1], "keep")
        var sent = JSON.parse(argv[2])
        compare(sent.source_type, "ultrawide")
        compare(sent.source_id, "aishot-5793.jpg")
        compare(sent.full_url, browsed.full_url)

        var searchedArgv = Retrieval.keepArgs({ source_type: "wallhaven", source_id: "abc123",
                                                source_url: "https://wallhaven.cc/w/abc123",
                                                full_url: "https://w.wallhaven.cc/full/abc123.jpg" })
        compare(searchedArgv[1], "keep")
        compare(JSON.parse(searchedArgv[2]).source_id, "abc123")
    }

    // There is nothing to keep when there is no result, or when this one is
    // already in the library.
    function test_keeping_nothing_or_something_already_kept_issues_nothing() {
        compare(Retrieval.keepArgs(null), null)
        compare(Retrieval.keepArgs({ source_type: "feed", source_id: "a",
                                     full_url: "https://example.com/a.jpg",
                                     is_favorited: true }), null)
    }

    // The Preview reads the result out of the grid's model by index rather
    // than holding a copy of its own, so the two surfaces cannot hold
    // different answers about one wallpaper.
    function test_the_preview_reads_the_result_out_of_the_model() {
        var results = [{ source_id: "a", is_favorited: false },
                       { source_id: "b", is_favorited: true }]

        compare(Retrieval.itemAt(results, 1).source_id, "b")
        compare(Retrieval.itemAt(results, -1), null)
        compare(Retrieval.itemAt(results, 2), null)
        compare(Retrieval.itemAt(null, 0), null)
    }

    // A successful keep replaces the result rather than writing through it: a
    // field written inside a JavaScript object notifies no QML binding, so the
    // Preview would go on offering to keep what is already kept.
    function test_a_keep_replaces_the_result_so_bindings_observe_it() {
        var results = [{ source_id: "a", is_favorited: false },
                       { source_id: "b", is_favorited: false }]

        var kept = Retrieval.markKept(results, 0)

        verify(kept !== results, "the model is replaced, not written through")
        verify(kept[0] !== results[0], "the kept result is replaced, not written through")
        verify(kept[0].is_favorited)
        compare(kept[0].source_id, "a")
        compare(kept[1], results[1])
        verify(!results[0].is_favorited, "the result the Preview still holds is untouched")

        // What the Preview reads afterwards is the kept one.
        verify(Retrieval.itemAt(kept, 0).is_favorited)
    }

    // A keep that names no result leaves the model exactly as it was — the
    // failure path must not flip anything.
    function test_keeping_at_no_index_leaves_the_model_alone() {
        var results = [{ source_id: "a", is_favorited: false }]

        compare(Retrieval.markKept(results, -1), results)
        compare(Retrieval.markKept(results, 1), results)
        compare(Retrieval.markKept(null, 0), null)
    }

    // The Preview's button is a binding onto the model, so what the model does
    // decides whether the button follows. Replacing the result re-evaluates it;
    // writing the field in place does not, which is the defect this pins.
    Component {
        id: preview
        QtObject {
            property var results: []
            property int index: -1
            readonly property var previewed: Retrieval.itemAt(results, index)
            // The favorite button's label and enabled state, both derived from
            // the one observed state.
            readonly property bool offersToKeep: !(previewed && previewed.is_favorited)
        }
    }

    function test_a_keep_flips_the_preview_s_button_while_it_stays_open() {
        var p = preview.createObject(null)
        p.results = [{ source_id: "a", is_favorited: false },
                     { source_id: "b", is_favorited: false }]
        p.index = 0
        verify(p.offersToKeep)

        p.results = Retrieval.markKept(p.results, 0)

        verify(!p.offersToKeep, "the button follows the keep without reopening")
        verify(p.results[0].is_favorited, "and the grid reads the same model")
        p.destroy()
    }

    // Why the keep must replace rather than write through: a field set inside a
    // JavaScript object notifies nothing, and the button stays wrong.
    function test_writing_the_field_in_place_notifies_nothing() {
        var p = preview.createObject(null)
        p.results = [{ source_id: "a", is_favorited: false }]
        p.index = 0
        verify(p.offersToKeep)

        p.results[0].is_favorited = true

        verify(p.offersToKeep, "an in-place write leaves the binding stale")
        p.destroy()
    }

    // Navigating to another result and back shows each one's true state.
    function test_navigating_shows_each_result_s_own_state() {
        var p = preview.createObject(null)
        p.results = [{ source_id: "a", is_favorited: false },
                     { source_id: "b", is_favorited: true }]

        p.index = 0
        verify(p.offersToKeep)
        p.index = 1
        verify(!p.offersToKeep, "an already-kept result shows kept on arrival")
        p.index = 0
        verify(p.offersToKeep)
        p.destroy()
    }

    // Ultrawide's categories are the site's tags and intersect, so they
    // combine; Curated's are mutually exclusive editorial pages and do not.
    // A selection built from the keyboard exists before it is retrieved —
    // toggling is deliberately not committing. The grid says what is waiting to
    // load rather than falling back to the message a searched source shows.
    function test_a_selection_built_but_not_yet_retrieved_says_what_will_load() {
        var msg = Retrieval.gridMessage(fixture(), { source: "Ultrawide", categories: ["space", "dark"],
                                                     retrieved: false, resultCount: 0 })
        verify(msg.text.indexOf("Space") >= 0, msg.text)
        verify(msg.text.indexOf("Dark") >= 0, msg.text)
        verify(msg.text.indexOf("wallpapers to get started") < 0,
               "a browsed source never shows the search prompt: " + msg.text)
        verify(!msg.isError)
    }

    // The bar's settling timer, wired the way CategoryBar.qml wires it: each
    // pick updates the selection at once (the chips must follow the click) and
    // restarts the timer, so a burst of picks costs one retrieval for the
    // final selection rather than one per pick.
    Component {
        id: settling
        QtObject {
            property var sources: []
            property string source: ""
            property var selection: []
            property var issued: []

            readonly property var published: Retrieval.categoriesOf(sources, source)
            readonly property bool combines: Retrieval.categoriesCombine(sources, source)

            property Timer settle: Timer {
                interval: Retrieval.SETTLE_MS
                onTriggered: fire()
            }

            function pick(slug) {
                selection = Retrieval.toggleSelection(published, selection, slug, combines)
                settle.restart()
            }

            function commit() {
                settle.stop()
                fire()
            }

            function fire() {
                var argv = Retrieval.args(sources, { source: source, categories: selection,
                                                     page: 1, perPage: 24, aspect: "all" })
                if (argv) issued.push(argv)
            }
        }
    }

    function test_a_burst_of_picks_costs_one_retrieval_for_the_final_selection() {
        var bar = settling.createObject(null, { sources: fixture(), source: "Ultrawide" })

        bar.pick("space")
        bar.pick("dark")
        bar.pick("nature")
        compare(bar.issued.length, 0, "nothing fires while the selection is still moving")
        compare(bar.selection, ["space", "nature", "dark"], "the chips follow every pick at once")

        tryCompare(bar, "issued", [["browse", "Ultrawide", "--category", "space",
                                    "--category", "nature", "--category", "dark",
                                    "--page", "1", "--per-page", "24"]])
        bar.destroy()
    }

    // Committing is distinguishable from toggling: it fires the selection
    // built so far without waiting, and a pending settle does not then fire a
    // second, identical request.
    function test_committing_fires_once_and_cancels_the_settle() {
        var bar = settling.createObject(null, { sources: fixture(), source: "Ultrawide" })

        bar.pick("space")
        bar.commit()
        compare(bar.issued.length, 1)

        wait(Retrieval.SETTLE_MS * 2)
        compare(bar.issued.length, 1, "the cancelled settle does not fire again")
        bar.destroy()
    }

    function fixture() {
        return [
            { name: "Wallhaven", source_type: "wallhaven", retrieval_mode: "searched", categories: [] },
            { name: "Ultrawide", source_type: "ultrawide", retrieval_mode: "browsed",
              categories: [{ slug: "space", label: "Space" }, { slug: "nature", label: "Nature" },
                           { slug: "dark", label: "Dark" }],
              categories_combine: true },
            { name: "Curated", source_type: "curated", retrieval_mode: "browsed",
              categories: [{ slug: "week", label: "This Week" }, { slug: "year", label: "This Year" }],
              categories_combine: false },
            { name: "Bing Daily", source_type: "feed", retrieval_mode: "browsed", categories: [] }
        ]
    }
}
