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
                ["space", "nature"])
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

        compare(Retrieval.args(sources, { source: "Ultrawide", category: "space", page: 1, perPage: 24, aspect: "all" }),
                ["browse", "Ultrawide", "--category", "space", "--page", "1", "--per-page", "24"])

        // A feed takes no category: selecting the feed is the selection.
        compare(Retrieval.args(sources, { source: "Bing Daily", category: "", page: 1, perPage: 24, aspect: "all" }),
                ["browse", "Bing Daily", "--page", "1", "--per-page", "24"])

        // Paging stays inside the selected category.
        compare(Retrieval.args(sources, { source: "Ultrawide", category: "nature", page: 3, perPage: 24, aspect: "all" }),
                ["browse", "Ultrawide", "--category", "nature", "--page", "3", "--per-page", "24"])
    }

    // A categorised browsed source with nothing selected yet is not a request
    // the CLI can answer, so none is issued.
    function test_a_categorised_source_without_a_category_issues_nothing() {
        compare(Retrieval.args(fixture(),
                               { source: "Ultrawide", category: "", page: 1, perPage: 24, aspect: "all" }),
                null)
    }

    // Loading, empty and failed are three distinguishable states, and an empty
    // category says which category was empty.
    function test_the_grid_message_distinguishes_loading_empty_and_failed() {
        var sources = fixture()
        var browsing = { source: "Ultrawide", category: "space", retrieved: true, resultCount: 0 }

        // Loading owns the grid; no message competes with the indicator.
        var loading = Retrieval.gridMessage(sources, { source: "Ultrawide", category: "space",
                                                       loading: true, retrieved: true, resultCount: 0 })
        compare(loading.text, "")

        var empty = Retrieval.gridMessage(sources, browsing)
        verify(empty.text.indexOf("Space") >= 0, "empty state names the category: " + empty.text)
        verify(!empty.isError)

        var failed = Retrieval.gridMessage(sources, { source: "Ultrawide", category: "space",
                                                      retrieved: true, resultCount: 0,
                                                      error: "no cards parsed on ultrawide/space" })
        verify(failed.isError)
        verify(failed.text.indexOf("no cards parsed") >= 0, failed.text)

        // Results on screen need no message at all.
        compare(Retrieval.gridMessage(sources, { source: "Ultrawide", category: "space",
                                                 retrieved: true, resultCount: 24 }).text, "")
    }

    // Picking a categorised source is not yet a retrieval; the grid says what
    // is missing instead of looking like an empty result.
    function test_a_categorised_source_awaiting_a_category_says_so() {
        var msg = Retrieval.gridMessage(fixture(), { source: "Ultrawide", category: "", retrieved: false,
                                                     resultCount: 0 })
        verify(msg.text.indexOf("categor") >= 0, msg.text)
        verify(msg.text.indexOf("Ultrawide") >= 0, msg.text)
        verify(!msg.isError)
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

    function fixture() {
        return [
            { name: "Wallhaven", source_type: "wallhaven", retrieval_mode: "searched", categories: [] },
            { name: "Ultrawide", source_type: "ultrawide", retrieval_mode: "browsed",
              categories: [{ slug: "space", label: "Space" }, { slug: "nature", label: "Nature" }] },
            { name: "Bing Daily", source_type: "feed", retrieval_mode: "browsed", categories: [] }
        ]
    }
}
