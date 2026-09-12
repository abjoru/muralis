use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::models::WallpaperPreview;

pub mod context;
pub use context::{ContentSafety, SourceContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectRatioFilter {
    All,
    Ratio16x9,
    Ratio21x9,
    Ratio32x9,
    Ratio16x10,
    Ratio4x3,
    Ratio3x2,
}

impl AspectRatioFilter {
    pub const ALL: &[AspectRatioFilter] = &[
        Self::All,
        Self::Ratio16x9,
        Self::Ratio21x9,
        Self::Ratio32x9,
        Self::Ratio16x10,
        Self::Ratio4x3,
        Self::Ratio3x2,
    ];

    pub fn ratio_pair(self) -> Option<(u32, u32)> {
        match self {
            Self::All => None,
            Self::Ratio16x9 => Some((16, 9)),
            Self::Ratio21x9 => Some((21, 9)),
            Self::Ratio32x9 => Some((32, 9)),
            Self::Ratio16x10 => Some((16, 10)),
            Self::Ratio4x3 => Some((4, 3)),
            Self::Ratio3x2 => Some((3, 2)),
        }
    }

    pub fn ratio_value(&self) -> Option<f64> {
        self.ratio_pair().map(|(w, h)| w as f64 / h as f64)
    }

    pub fn matches(self, width: u32, height: u32) -> bool {
        let Some(target) = self.ratio_value() else {
            return true;
        };
        if width == 0 || height == 0 {
            return true;
        }
        let ratio = width as f64 / height as f64;
        (ratio - target).abs() < 0.1
    }

    pub fn from_dimensions(w: u32, h: u32) -> Self {
        if w == 0 || h == 0 {
            return Self::All;
        }
        let ratio = w as f64 / h as f64;
        Self::ALL
            .iter()
            .copied()
            .filter_map(|f| f.ratio_pair().map(|(rw, rh)| (f, rw as f64 / rh as f64)))
            .min_by(|a, b| {
                (ratio - a.1)
                    .abs()
                    .partial_cmp(&(ratio - b.1).abs())
                    .unwrap()
            })
            .map(|(v, _)| v)
            .unwrap_or(Self::All)
    }

    pub fn to_wallhaven_ratio(&self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Ratio16x9 => Some("16x9"),
            Self::Ratio21x9 => Some("21x9"),
            Self::Ratio32x9 => Some("32x9"),
            Self::Ratio16x10 => Some("16x10"),
            Self::Ratio4x3 => Some("4x3"),
            Self::Ratio3x2 => Some("3x2"),
        }
    }
}

impl std::str::FromStr for AspectRatioFilter {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "all" => Ok(Self::All),
            "16x9" | "16:9" => Ok(Self::Ratio16x9),
            "21x9" | "21:9" => Ok(Self::Ratio21x9),
            "32x9" | "32:9" => Ok(Self::Ratio32x9),
            "16x10" | "16:10" => Ok(Self::Ratio16x10),
            "4x3" | "4:3" => Ok(Self::Ratio4x3),
            "3x2" | "3:2" => Ok(Self::Ratio3x2),
            other => Err(format!("unknown aspect ratio: {other}")),
        }
    }
}

impl fmt::Display for AspectRatioFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::Ratio16x9 => write!(f, "16:9"),
            Self::Ratio21x9 => write!(f, "21:9"),
            Self::Ratio32x9 => write!(f, "32:9"),
            Self::Ratio16x10 => write!(f, "16:10"),
            Self::Ratio4x3 => write!(f, "4:3"),
            Self::Ratio3x2 => write!(f, "3:2"),
        }
    }
}

/// How a **Source** is retrieved from — a declared property of the Source, not
/// an inference from its type name.
///
/// **Searched** Sources accept a query and participate in an unscoped
/// `search`. **Browsed** Sources have no query dimension at all: they are
/// excluded from unscoped search and reachable only by naming them, because a
/// Source that silently ignores the query it is handed pollutes every result
/// set it is fanned out over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RetrievalMode {
    /// Accepts a query; participates in unscoped `search`.
    Searched,
    /// Takes no query; reachable only by name, optionally naming a category.
    Browsed,
}

impl fmt::Display for RetrievalMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Searched => write!(f, "searched"),
            Self::Browsed => write!(f, "browsed"),
        }
    }
}

/// A named, selectable slice of a **Browsed** Source's catalog. The `slug` is
/// what a caller passes; the `label` is what a UI renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCategory {
    pub slug: String,
    pub label: String,
}

impl SourceCategory {
    pub fn new(slug: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            slug: slug.into(),
            label: label.into(),
        }
    }
}

#[async_trait]
pub trait WallpaperSource: Send + Sync {
    /// Display name / tab label (e.g. "Wallhaven", "Bing Daily")
    fn name(&self) -> &str;
    /// DB type string (e.g. "wallhaven", "feed")
    fn source_type(&self) -> &str;

    /// Return up to `per_page` previews for logical `page`, each matching
    /// `aspect` (sources MUST honor the aspect filter themselves — callers do
    /// not post-filter). `per_page` is a best-effort target, not a guarantee:
    /// a page may return fewer matches even when more exist upstream, because
    /// filtering happens within a bounded block of upstream pages (see the
    /// `Block` / page-filling design in `muralis-source-common`). An empty
    /// result is the caller's end-of-results signal.
    async fn search(
        &self,
        query: &str,
        page: u32,
        per_page: u32,
        aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>>;
    async fn download(&self, preview: &WallpaperPreview) -> Result<bytes::Bytes>;

    /// This Source's **retrieval mode**. Defaults to `Searched`, so a Source
    /// that declares nothing keeps its existing behavior with no edit.
    fn retrieval_mode(&self) -> RetrievalMode {
        RetrievalMode::Searched
    }

    /// The categories a **Browsed** Source publishes. Zero categories is
    /// meaningful and is the **Feed Source**'s case: selecting the feed *is*
    /// the selection. A **Searched** Source publishes none.
    fn categories(&self) -> Vec<SourceCategory> {
        Vec::new()
    }

    /// Whether this Source's **Categories** combine — whether naming several
    /// asks for their intersection, or whether they are mutually exclusive
    /// slices of which exactly one can be selected. Declared, never assumed:
    /// a Source that cannot intersect must be able to refuse a second
    /// category rather than silently honour the first. Defaults to `false`,
    /// so a Source that declares nothing keeps the single-category contract.
    fn categories_combine(&self) -> bool {
        false
    }

    /// Whether naming *no* category is itself a selection this Source can
    /// answer. Defaults to `false`: a Source publishing categories requires
    /// one, because browsing "everything" is not a slice it offers. A Source
    /// with an untagged feed behind its categories declares otherwise.
    /// Irrelevant to a Source publishing no categories at all — there the
    /// empty selection is the only one.
    fn empty_selection_is_meaningful(&self) -> bool {
        false
    }

    /// Retrieve from a **Browsed** Source: no query, a *set* of category
    /// slugs, paged and aspect-filtered exactly as `search` is. Several slugs
    /// ask for their intersection, and only a Source declaring
    /// [`categories_combine`](Self::categories_combine) is ever handed more
    /// than one. The set arrives resolved by [`select_categories`] — every
    /// slug published, deduplicated, in a canonical order — so a Source never
    /// validates it again. Defaults to refusing, because a **Searched**
    /// Source has nothing to browse.
    async fn browse(
        &self,
        _categories: &[String],
        _page: u32,
        _per_page: u32,
        _aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        Err(crate::error::MuralisError::Source {
            source_type: self.source_type().to_string(),
            op: "browse".into(),
            kind: format!(
                "'{}' is a searched source; use: muralis search",
                self.name()
            ),
        })
    }

    /// Resolve a URL from this source into a WallpaperPreview.
    /// Sources opt in by overriding; default returns None.
    async fn resolve_url(&self, _url: &str) -> Result<Option<WallpaperPreview>> {
        Ok(None)
    }

    /// Why this Source declines `url` — asked only after `resolve_url` has
    /// yielded nothing, and answered only when the Source recognises the URL
    /// as its own but it names something other than an image (a **Category**
    /// page, a listing, a post). It exists so a caller pasting the wrong kind
    /// of URL learns *what kind* was expected, instead of being told only that
    /// nothing resolved. Default: nothing to say.
    fn explain_unresolvable(&self, _url: &str) -> Option<String> {
        None
    }
}

/// Resolve the categories a browse request names against what the Source
/// actually publishes, before any network call.
///
/// The answer is a **set**: deduplicated, and in the Source's own published
/// order rather than the caller's typing order, so the same selection asked
/// for either way round produces one upstream request and one cache key.
///
/// Refused, always before the network and never as an empty result:
/// - a slug the Source does not publish — answered with the ones it does, so
///   a typo reads as a typo rather than as "nothing found";
/// - more than one slug at a Source whose categories do not combine, named
///   as that Source's limit rather than silently honouring the first;
/// - no slug at all at a Source that publishes categories, unless it declares
///   the empty selection meaningful. A Source publishing none (the **Feed
///   Source**) takes the empty selection and nothing else: selecting the feed
///   *is* the selection.
pub fn select_categories(
    source: &dyn WallpaperSource,
    requested: &[String],
) -> Result<Vec<String>> {
    let published = source.categories();
    let refuse = |kind: String| {
        Err(crate::error::MuralisError::Source {
            source_type: source.source_type().to_string(),
            op: "browse".into(),
            kind,
        })
    };
    let available = || {
        published
            .iter()
            .map(|c| c.slug.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    if requested.is_empty() {
        if published.is_empty() || source.empty_selection_is_meaningful() {
            return Ok(Vec::new());
        }
        return refuse(format!(
            "'{}' publishes categories; name one with --category. available: {}",
            source.name(),
            available()
        ));
    }
    if published.is_empty() {
        return refuse(format!(
            "'{}' publishes no categories; browse it without --category (got '{}')",
            source.name(),
            requested.join(", ")
        ));
    }

    // Every named slug, not just the first: a typo beside a real tag is still
    // a typo, and the endpoint would answer it with an empty window.
    for slug in requested {
        if !published.iter().any(|c| &c.slug == slug) {
            return refuse(format!(
                "'{}' does not publish category '{slug}'. available: {}",
                source.name(),
                available()
            ));
        }
    }

    // Canonical order is the published one, so the set — not the typing — is
    // what the request is built from. Dedup happens here, before
    // combinability is asked about: naming one category twice is naming it
    // once, at every Source.
    let selected: Vec<String> = published
        .iter()
        .filter(|c| requested.iter().any(|r| r == &c.slug))
        .map(|c| c.slug.clone())
        .collect();

    if selected.len() > 1 && !source.categories_combine() {
        return refuse(format!(
            "'{}' takes one category at a time; its categories do not combine (got: {})",
            source.name(),
            selected.join(", ")
        ));
    }
    Ok(selected)
}

pub struct SourceRegistry {
    sources: Vec<Box<dyn WallpaperSource>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub fn register(&mut self, source: Box<dyn WallpaperSource>) {
        self.sources.push(source);
    }

    pub fn names(&self) -> Vec<&str> {
        self.sources.iter().map(|s| s.name()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn WallpaperSource> {
        self.sources
            .iter()
            .find(|s| s.name() == name)
            .map(|s| s.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn WallpaperSource> {
        self.sources.iter().map(|s| s.as_ref())
    }

    /// The **Source** that can download a **Preview**, found by the
    /// `source_type` the Preview carries rather than by display name — a
    /// Preview records its type, never the name a UI renders.
    ///
    /// Several Sources can share one `source_type`: every **Feed Source**
    /// instance is `feed`. That is not an ambiguity here, because downloading
    /// a Preview needs only the type's transport and every instance of a type
    /// fetches `full_url` the same way. Per-host identity that *would* be
    /// ambiguous — the **Booru Source**'s — already lives in `source_type`.
    pub fn by_source_type(&self, source_type: &str) -> Option<&dyn WallpaperSource> {
        self.iter().find(|s| s.source_type() == source_type)
    }

    /// Only the **Searched** Sources — the set an unscoped `search` fans out
    /// over. Filtering lives here so the call site cannot forget it and let a
    /// **Browsed** Source leak query-ignoring results into a query's answer.
    pub fn searched(&self) -> impl Iterator<Item = &dyn WallpaperSource> {
        self.iter()
            .filter(|s| s.retrieval_mode() == RetrievalMode::Searched)
    }
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceType;

    /// A **Source** that declares nothing beyond the required methods — the
    /// shape every existing API source crate has today.
    struct Undeclared;

    #[async_trait]
    impl WallpaperSource for Undeclared {
        fn name(&self) -> &str {
            "undeclared"
        }
        fn source_type(&self) -> &str {
            "undeclared"
        }
        async fn search(
            &self,
            _query: &str,
            _page: u32,
            _per_page: u32,
            _aspect: AspectRatioFilter,
        ) -> Result<Vec<WallpaperPreview>> {
            Ok(Vec::new())
        }
        async fn download(&self, _preview: &WallpaperPreview) -> Result<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
    }

    /// A **Browsed** Source with a configurable category list and the
    /// declarations a selection is resolved against.
    #[derive(Default)]
    struct Browsable {
        categories: Vec<SourceCategory>,
        combines: bool,
        empty_ok: bool,
    }

    #[async_trait]
    impl WallpaperSource for Browsable {
        fn name(&self) -> &str {
            "browsable"
        }
        fn source_type(&self) -> &str {
            "browsable"
        }
        fn retrieval_mode(&self) -> RetrievalMode {
            RetrievalMode::Browsed
        }
        fn categories(&self) -> Vec<SourceCategory> {
            self.categories.clone()
        }
        fn categories_combine(&self) -> bool {
            self.combines
        }
        fn empty_selection_is_meaningful(&self) -> bool {
            self.empty_ok
        }
        async fn search(
            &self,
            _query: &str,
            _page: u32,
            _per_page: u32,
            _aspect: AspectRatioFilter,
        ) -> Result<Vec<WallpaperPreview>> {
            unreachable!("a browsed source is never searched")
        }
        async fn download(&self, _preview: &WallpaperPreview) -> Result<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
    }

    #[test]
    fn a_source_that_declares_nothing_is_searched_with_no_categories() {
        let s = Undeclared;
        assert_eq!(s.retrieval_mode(), RetrievalMode::Searched);
        assert!(s.categories().is_empty());
        let _ = SourceType::new("undeclared");
    }

    #[test]
    fn the_registry_yields_only_searched_sources_so_the_call_site_cannot_forget() {
        let mut registry = SourceRegistry::new();
        registry.register(Box::new(Undeclared));
        registry.register(Box::new(Browsable::default()));

        assert_eq!(registry.names(), vec!["undeclared", "browsable"]);
        assert_eq!(
            registry.searched().map(|s| s.name()).collect::<Vec<_>>(),
            vec!["undeclared"]
        );
    }

    /// A **Source** whose display name is nothing like its `source_type` —
    /// the **Feed Source**'s shape, and what a Preview-keyed lookup must cope
    /// with.
    struct Renamed;

    #[async_trait]
    impl WallpaperSource for Renamed {
        fn name(&self) -> &str {
            "Daily Feed"
        }
        fn source_type(&self) -> &str {
            "feed"
        }
        async fn search(
            &self,
            _query: &str,
            _page: u32,
            _per_page: u32,
            _aspect: AspectRatioFilter,
        ) -> Result<Vec<WallpaperPreview>> {
            Ok(Vec::new())
        }
        async fn download(&self, _preview: &WallpaperPreview) -> Result<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
    }

    #[test]
    fn a_preview_finds_its_source_by_source_type_not_by_display_name() {
        let mut registry = SourceRegistry::new();
        registry.register(Box::new(Undeclared));
        registry.register(Box::new(Renamed));

        assert_eq!(
            registry.by_source_type("feed").map(|s| s.name()),
            Some("Daily Feed"),
            "a Preview carries its type, never the name a UI renders"
        );
        assert!(registry.by_source_type("Daily Feed").is_none());
        assert!(registry.by_source_type("nothing-configured").is_none());
    }

    #[test]
    fn a_source_that_declares_nothing_explains_no_url() {
        assert_eq!(
            Undeclared.explain_unresolvable("https://example.com/a"),
            None
        );
    }

    fn with_categories() -> Browsable {
        Browsable {
            categories: vec![
                SourceCategory::new("space", "Space"),
                SourceCategory::new("nature", "Nature"),
            ],
            ..Browsable::default()
        }
    }

    fn combining() -> Browsable {
        Browsable {
            combines: true,
            ..with_categories()
        }
    }

    fn named(slugs: &[&str]) -> Vec<String> {
        slugs.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn an_unpublished_category_is_refused_with_the_ones_that_are_published() {
        let err = select_categories(&with_categories(), &named(&["volcanoes"]))
            .expect_err("a category the source does not publish is not retrievable");
        let msg = err.to_string();

        assert!(msg.contains("volcanoes"), "{msg}");
        assert!(msg.contains("space"), "{msg}");
        assert!(msg.contains("nature"), "{msg}");
    }

    /// Validation is of *every* named category, not just the first — a typo
    /// alongside a real tag is still a typo, and it is caught before the
    /// request rather than read as an empty result afterwards.
    #[test]
    fn a_typo_among_valid_categories_is_refused_rather_than_browsed_around() {
        let err = select_categories(&combining(), &named(&["space", "volcanos"]))
            .expect_err("one unpublished name spoils the selection");

        assert!(err.to_string().contains("volcanos"), "{err}");
    }

    #[test]
    fn a_published_category_selects_by_slug() {
        assert_eq!(
            select_categories(&with_categories(), &named(&["space"])).unwrap(),
            named(&["space"])
        );
    }

    /// Order does not matter: the same *set* canonicalises to one selection,
    /// so the upstream request a cache or a comparison sees is the same
    /// string whichever way it was typed.
    #[test]
    fn the_same_set_of_categories_in_either_order_is_one_selection() {
        let one = select_categories(&combining(), &named(&["space", "nature"])).unwrap();
        let other = select_categories(&combining(), &named(&["nature", "space"])).unwrap();

        assert_eq!(one, other);
        assert_eq!(
            one,
            named(&["space", "nature"]),
            "canonical: published order"
        );
    }

    #[test]
    fn naming_the_same_category_twice_is_naming_it_once() {
        assert_eq!(
            select_categories(&combining(), &named(&["space", "space"])).unwrap(),
            named(&["space"])
        );
        assert_eq!(
            select_categories(&with_categories(), &named(&["space", "space"])).unwrap(),
            named(&["space"]),
            "duplicates collapse before combinability is asked about"
        );
    }

    /// Combination is a property the Source declares. One that does not
    /// combine refuses the second category by name rather than honouring the
    /// first and dropping the rest.
    #[test]
    fn several_categories_at_a_source_that_does_not_combine_them_are_refused() {
        let err = select_categories(&with_categories(), &named(&["space", "nature"]))
            .expect_err("its categories are mutually exclusive slices");
        let msg = err.to_string();

        assert!(
            msg.contains("browsable"),
            "the refusal names the Source: {msg}"
        );
        assert!(msg.contains("one"), "{msg}");
    }

    #[test]
    fn a_combining_source_takes_the_whole_selection() {
        assert_eq!(
            select_categories(&combining(), &named(&["nature", "space"])).unwrap(),
            named(&["space", "nature"])
        );
    }

    #[test]
    fn zero_categories_means_selecting_the_source_is_the_selection() {
        let feed_shaped = Browsable::default();

        assert_eq!(
            select_categories(&feed_shaped, &[]).unwrap(),
            Vec::<String>::new()
        );

        let err = select_categories(&feed_shaped, &named(&["anything"]))
            .expect_err("there is no slice to name");
        assert!(err.to_string().contains("publishes no categories"));
    }

    #[test]
    fn a_categorised_source_requires_a_category_to_retrieve_anything() {
        let err = select_categories(&with_categories(), &[])
            .expect_err("browsing everything is not a slice it offers");
        let msg = err.to_string();

        assert!(msg.contains("space") && msg.contains("nature"), "{msg}");
    }

    /// Unless it says otherwise: a Source for which the empty selection is
    /// itself a slice gets to answer one, and that is a declaration too.
    #[test]
    fn an_empty_selection_is_honoured_where_the_source_declares_it_meaningful() {
        let untagged = Browsable {
            empty_ok: true,
            ..with_categories()
        };

        assert_eq!(
            select_categories(&untagged, &[]).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_source_that_declares_nothing_neither_combines_nor_browses_empty() {
        assert!(!Undeclared.categories_combine());
        assert!(!Undeclared.empty_selection_is_meaningful());
    }
}
