//! Multi-host booru source: one crate, many imageboard hosts, configured as
//! multiple `[[sources.booru]]` instances (like the feed source). Each instance
//! names a host (`name` → per-host `source_type`/dedup identity) plus a `flavor`
//! (the API dialect → which response shape + endpoints). See `CONTEXT.md`
//! ("Booru Source", "Flavor", "source_type (booru)").
//!
//! Boorus are tag-based with no usable server aspect param, so aspect filtering
//! is client-side page-filling over a block (`BLOCK`). The 2-tag cap on
//! unauthenticated search means the tag budget goes to `rating:` + the user
//! query — the `rating:` tag is derived from the global content-safety ceiling
//! and folded ahead of the query by the shared engine (`Descriptor.tag_prefix`).

use std::sync::Arc;

use serde::Deserialize;

use muralis_core::models::{SourceType, WallpaperPreview};
use muralis_core::sources::{ContentSafety, SourceContext, WallpaperSource};
use muralis_source_common::{
    Auth, Descriptor, DetailResponse, HttpFetch, ReqwestFetch, RestSource, SearchResponse,
};

/// Upstream pages consumed per logical page. Boorus filter aspect client-side,
/// so over-fetch a block to fill an ultrawide page (Block B > 1).
const BLOCK: u32 = 3;
/// Per-upstream-page size. Kept modest: a block already fetches `BLOCK ×` this.
const PER_PAGE_CAP: u32 = 40;

#[derive(Debug, Clone, Deserialize)]
pub struct BooruConfig {
    /// Per-host identity → `source_type` (dedup key) and display name.
    pub name: String,
    /// API dialect → which flavor descriptor (`danbooru`, `moebooru`).
    pub flavor: String,
    #[serde(default)]
    pub enabled: bool,
    /// Optional per-instance rating override. Reserved: the rating tag is
    /// derived from the global ceiling for now; honoring an override (tighten
    /// only, never past the ceiling) is a follow-up.
    #[serde(default)]
    pub rating: Option<String>,
}

pub fn create_sources(
    table: &toml::Table,
    client: reqwest::Client,
    ctx: &SourceContext,
) -> Vec<Box<dyn WallpaperSource>> {
    let Some(arr) = table.get("booru").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let http: Arc<dyn HttpFetch> = Arc::new(ReqwestFetch(client));
    let mut out: Vec<Box<dyn WallpaperSource>> = Vec::new();

    for item in arr {
        let cfg: BooruConfig = match item.clone().try_into() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("booru: skipping malformed instance: {e}");
                continue;
            }
        };
        if !cfg.enabled {
            continue;
        }
        let Some(base) = default_base(&cfg.name) else {
            tracing::warn!(
                "booru: unknown host name {:?}; no known base, skipping",
                cfg.name
            );
            continue;
        };
        let tag_prefix = rating_prefix(&cfg.flavor, ctx.content_safety);

        match cfg.flavor.as_str() {
            "danbooru" => {
                let desc = descriptor(&cfg.name, base, "/posts.json", "/posts", tag_prefix);
                out.push(Box::new(RestSource::<DanbooruSearch, DanbooruPost>::new(
                    desc,
                    http.clone(),
                )));
            }
            "moebooru" => {
                let desc = descriptor(&cfg.name, base, "/post.json", "/post", tag_prefix);
                out.push(Box::new(
                    RestSource::<MoebooruResponse, MoebooruResponse>::new(desc, http.clone()),
                ));
            }
            other => tracing::warn!("booru: unknown flavor {other:?} for {:?}", cfg.name),
        }
    }
    out
}

/// Shared descriptor shape for every booru flavor: tag search, no server
/// aspect param (client-side block filtering), 1-indexed `page`.
fn descriptor(
    name: &str,
    base: &'static str,
    search_path: &'static str,
    detail_path: &'static str,
    tag_prefix: Option<String>,
) -> Descriptor {
    Descriptor {
        source_type: name.to_string(),
        display_name: name.to_string(),
        base,
        auth: Auth::None,
        search_path,
        detail_path,
        query_key: "tags",
        tag_prefix,
        per_page_param: Some("limit"),
        per_page_cap: PER_PAGE_CAP,
        block: BLOCK,
        extra_query: Vec::new(),
        server_aspect_param: None,
    }
}

/// Known host bases keyed by config `name`. Arbitrary hosts (gelbooru clones
/// like rule34) join this table per flavor; `base` stays `&'static` so the
/// shared `Descriptor` is unchanged.
fn default_base(name: &str) -> Option<&'static str> {
    Some(match name {
        "danbooru" => "https://danbooru.donmai.us",
        "yandere" | "yande.re" => "https://yande.re",
        "konachan" => "https://konachan.com",
        _ => return None,
    })
}

/// The `rating:` tag for a flavor under the global ceiling. The ceiling is a
/// cap: `Safe` forces the single safest rating (never leaks); `Moderate`
/// excludes only explicit; `Nsfw` adds no rating tag (full range up to the
/// host's own cap).
fn rating_prefix(flavor: &str, ceiling: ContentSafety) -> Option<String> {
    let tag = match flavor {
        "danbooru" => match ceiling {
            ContentSafety::Safe => "rating:g",
            ContentSafety::Moderate => "-rating:e",
            ContentSafety::Nsfw => "",
        },
        "moebooru" => match ceiling {
            ContentSafety::Safe => "rating:safe",
            ContentSafety::Moderate => "-rating:explicit",
            ContentSafety::Nsfw => "",
        },
        _ => "",
    };
    (!tag.is_empty()).then(|| tag.to_string())
}

/// Path portion of a URL (`scheme://host/PATH`), or `None` if there is none.
fn path_of(url: &str) -> Option<&str> {
    url.split_once("://")?.1.split_once('/').map(|(_, p)| p)
}

/// First path segment after `prefix`, e.g. `posts/123?foo` minus `posts/` → `123`.
fn id_after(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let seg = rest.split(['/', '?', '#']).next()?;
    (!seg.is_empty()).then(|| seg.to_string())
}

// -- danbooru flavor: `/posts.json` flat array, detail `/posts/{id}.json` --

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct DanbooruSearch(Vec<DanbooruPost>);

#[derive(Debug, Deserialize)]
pub struct DanbooruPost {
    id: u64,
    #[serde(default)]
    image_width: u32,
    #[serde(default)]
    image_height: u32,
    #[serde(default)]
    file_url: String,
    #[serde(default)]
    large_file_url: String,
    #[serde(default)]
    preview_file_url: String,
    #[serde(default)]
    tag_string: String,
}

impl SearchResponse for DanbooruSearch {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview> {
        self.0
            .into_iter()
            .map(|p| p.into_preview(source_type))
            .collect()
    }
}

impl DetailResponse for DanbooruPost {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        let full = first_nonempty([self.file_url, self.large_file_url, self.preview_file_url]);
        WallpaperPreview {
            source_type: SourceType::new(source_type),
            source_id: self.id.to_string(),
            // No post-page URL in the JSON and the mapper lacks `base`; use the
            // image URL (cosmetic — dedup keys on (source_id, source_type)).
            source_url: full.full.clone(),
            thumbnail_url: full.thumb,
            full_url: full.full,
            width: self.image_width,
            height: self.image_height,
            tags: split_tags(&self.tag_string),
        }
    }
    fn parse_id(url: &str) -> Option<String> {
        id_after(path_of(url)?, "posts/")
    }
    const HOST_SCOPED: bool = true;
    fn detail_request(
        base: &str,
        detail_path: &str,
        _search_path: &str,
        id: &str,
    ) -> (String, Vec<(&'static str, String)>) {
        (format!("{base}{detail_path}/{id}.json"), Vec::new())
    }
}

// -- moebooru flavor (yande.re, Konachan): `/post.json` flat array; no
//    GET-by-id, so resolve via `/post.json?tags=id:{id}` --

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct MoebooruResponse(Vec<MoebooruPost>);

#[derive(Debug, Deserialize)]
pub struct MoebooruPost {
    id: u64,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    file_url: String,
    #[serde(default)]
    jpeg_url: String,
    #[serde(default)]
    sample_url: String,
    #[serde(default)]
    preview_url: String,
    #[serde(default)]
    tags: String,
}

impl MoebooruPost {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        let full = first_nonempty([self.file_url, self.jpeg_url, self.sample_url]);
        let thumb = if self.preview_url.is_empty() {
            full.thumb
        } else {
            self.preview_url
        };
        WallpaperPreview {
            source_type: SourceType::new(source_type),
            source_id: self.id.to_string(),
            source_url: full.full.clone(),
            thumbnail_url: thumb,
            full_url: full.full,
            width: self.width,
            height: self.height,
            tags: split_tags(&self.tags),
        }
    }
}

impl SearchResponse for MoebooruResponse {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview> {
        self.0
            .into_iter()
            .map(|p| p.into_preview(source_type))
            .collect()
    }
}

impl DetailResponse for MoebooruResponse {
    fn into_preview(self, source_type: &str) -> WallpaperPreview {
        self.0
            .into_iter()
            .next()
            .map(|p| p.into_preview(source_type))
            .unwrap_or_else(|| WallpaperPreview {
                source_type: SourceType::new(source_type),
                source_id: String::new(),
                source_url: String::new(),
                thumbnail_url: String::new(),
                full_url: String::new(),
                width: 0,
                height: 0,
                tags: Vec::new(),
            })
    }
    fn parse_id(url: &str) -> Option<String> {
        id_after(path_of(url)?, "post/show/")
    }
    const HOST_SCOPED: bool = true;
    fn detail_request(
        base: &str,
        _detail_path: &str,
        search_path: &str,
        id: &str,
    ) -> (String, Vec<(&'static str, String)>) {
        (
            format!("{base}{search_path}"),
            vec![("tags", format!("id:{id}"))],
        )
    }
}

// -- shared mapping helpers --

struct Urls {
    full: String,
    thumb: String,
}

/// Pick the first non-empty URL as the full image; the last as a thumb fallback.
fn first_nonempty<const N: usize>(candidates: [String; N]) -> Urls {
    let full = candidates
        .iter()
        .find(|c| !c.is_empty())
        .cloned()
        .unwrap_or_default();
    let thumb = candidates
        .iter()
        .rev()
        .find(|c| !c.is_empty())
        .cloned()
        .unwrap_or_else(|| full.clone());
    Urls { full, thumb }
}

fn split_tags(s: &str) -> Vec<String> {
    s.split_whitespace().map(|t| t.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use muralis_core::sources::AspectRatioFilter;
    use muralis_source_common::testing::StubFetch;

    fn safe_ctx() -> SourceContext {
        SourceContext {
            content_safety: ContentSafety::Safe,
            min_width: 1920,
            min_height: 1080,
        }
    }

    fn danbooru(
        http: Arc<StubFetch>,
        ctx: &SourceContext,
    ) -> RestSource<DanbooruSearch, DanbooruPost> {
        let desc = descriptor(
            "danbooru",
            "https://danbooru.donmai.us",
            "/posts.json",
            "/posts",
            rating_prefix("danbooru", ctx.content_safety),
        );
        RestSource::new(desc, http)
    }

    fn moebooru(
        name: &str,
        base: &'static str,
        http: Arc<StubFetch>,
        ctx: &SourceContext,
    ) -> RestSource<MoebooruResponse, MoebooruResponse> {
        let desc = descriptor(
            name,
            base,
            "/post.json",
            "/post",
            rating_prefix("moebooru", ctx.content_safety),
        );
        RestSource::new(desc, http)
    }

    const DANBOORU_BODY: &str = r#"[
        {"id":1,"image_width":3440,"image_height":1440,
         "file_url":"https://cdn.donmai.us/orig/1.jpg",
         "large_file_url":"https://cdn.donmai.us/sample/1.jpg",
         "preview_file_url":"https://cdn.donmai.us/preview/1.jpg",
         "tag_string":"landscape scenery"}
    ]"#;

    #[tokio::test]
    async fn danbooru_parses_array_and_maps_preview() {
        let http = Arc::new(StubFetch::ok_pages(&[DANBOORU_BODY, "[]", "[]"]));
        let previews = danbooru(http, &safe_ctx())
            .search("landscape", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "1");
        assert_eq!(previews[0].width, 3440);
        assert_eq!(previews[0].full_url, "https://cdn.donmai.us/orig/1.jpg");
        assert_eq!(
            previews[0].thumbnail_url,
            "https://cdn.donmai.us/preview/1.jpg"
        );
        assert_eq!(previews[0].tags, vec!["landscape", "scenery"]);
    }

    #[tokio::test]
    async fn danbooru_folds_rating_tag_and_pages_one_indexed() {
        let http = Arc::new(StubFetch::ok_pages(&["[]", "[]", "[]"]));
        danbooru(http.clone(), &safe_ctx())
            .search("forest", 2, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let calls = http.calls();
        // safe ceiling → rating:g folded ahead of the query in the `tags` param
        assert_eq!(calls[0].query_value("tags"), Some("rating:g forest"));
        assert_eq!(calls[0].url, "https://danbooru.donmai.us/posts.json");
        assert_eq!(calls[0].query_value("limit"), Some("40"));
        // logical page 2, block 3 → upstream pages 4,5,6 (1-indexed)
        let pages: Vec<_> = calls
            .iter()
            .map(|c| c.query_value("page").unwrap().to_string())
            .collect();
        assert_eq!(pages, vec!["4", "5", "6"]);
    }

    #[tokio::test]
    async fn danbooru_resolves_post_url_via_json_detail() {
        let body = r#"{"id":7,"image_width":1920,"image_height":1080,
            "file_url":"https://cdn.donmai.us/orig/7.jpg","tag_string":"sky"}"#;
        let http = Arc::new(StubFetch::ok(body));
        let preview = danbooru(http.clone(), &safe_ctx())
            .resolve_url("https://danbooru.donmai.us/posts/7")
            .await
            .unwrap()
            .expect("danbooru resolves its own host");

        assert_eq!(preview.source_id, "7");
        assert_eq!(
            http.last_call().url,
            "https://danbooru.donmai.us/posts/7.json"
        );
    }

    const MOEBOORU_BODY: &str = r#"[
        {"id":42,"width":2560,"height":1080,
         "file_url":"https://files.yande.re/image/42.jpg",
         "sample_url":"https://files.yande.re/sample/42.jpg",
         "preview_url":"https://files.yande.re/preview/42.jpg",
         "tags":"original long_hair"}
    ]"#;

    #[tokio::test]
    async fn moebooru_parses_array_and_folds_rating() {
        let http = Arc::new(StubFetch::ok_pages(&[MOEBOORU_BODY, "[]", "[]"]));
        let previews = moebooru("yandere", "https://yande.re", http.clone(), &safe_ctx())
            .search("original", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "42");
        assert_eq!(previews[0].full_url, "https://files.yande.re/image/42.jpg");
        assert_eq!(
            http.calls()[0].query_value("tags"),
            Some("rating:safe original")
        );
    }

    #[tokio::test]
    async fn moebooru_resolves_via_search_by_id() {
        let http = Arc::new(StubFetch::ok(MOEBOORU_BODY));
        let preview = moebooru("yandere", "https://yande.re", http.clone(), &safe_ctx())
            .resolve_url("https://yande.re/post/show/42")
            .await
            .unwrap()
            .expect("matching host resolves");

        assert_eq!(preview.source_id, "42");
        let call = http.last_call();
        assert_eq!(call.url, "https://yande.re/post.json");
        assert_eq!(call.query_value("tags"), Some("id:42"));
    }

    #[tokio::test]
    async fn two_moebooru_hosts_do_not_collide_on_resolution() {
        // a konachan instance must reject a yande.re URL (same path shape) in
        // the engine, before any network call — host_scoped guards it.
        let http = Arc::new(StubFetch::ok(MOEBOORU_BODY));
        let resolved = moebooru(
            "konachan",
            "https://konachan.com",
            http.clone(),
            &safe_ctx(),
        )
        .resolve_url("https://yande.re/post/show/42")
        .await
        .unwrap();

        assert!(resolved.is_none());
        assert!(http.calls().is_empty(), "must not hit the network");
    }

    #[test]
    fn rating_prefix_tracks_the_ceiling() {
        assert_eq!(
            rating_prefix("danbooru", ContentSafety::Safe).as_deref(),
            Some("rating:g")
        );
        assert_eq!(
            rating_prefix("danbooru", ContentSafety::Moderate).as_deref(),
            Some("-rating:e")
        );
        assert_eq!(rating_prefix("danbooru", ContentSafety::Nsfw), None);
        assert_eq!(
            rating_prefix("moebooru", ContentSafety::Safe).as_deref(),
            Some("rating:safe")
        );
        assert_eq!(rating_prefix("moebooru", ContentSafety::Nsfw), None);
    }

    #[test]
    fn create_sources_builds_enabled_known_hosts_only() {
        let toml_str = r#"
            [[booru]]
            name = "danbooru"
            flavor = "danbooru"
            enabled = true

            [[booru]]
            name = "yandere"
            flavor = "moebooru"
            enabled = true

            [[booru]]
            name = "konachan"
            flavor = "moebooru"
            enabled = false

            [[booru]]
            name = "unknownhost"
            flavor = "danbooru"
            enabled = true
        "#;
        let table: toml::Table = toml_str.parse().unwrap();
        let sources = create_sources(&table, reqwest::Client::new(), &safe_ctx());
        // danbooru + yandere enabled & known; konachan disabled; unknownhost skipped
        let names: Vec<_> = sources
            .iter()
            .map(|s| s.source_type().to_string())
            .collect();
        assert_eq!(names, vec!["danbooru", "yandere"]);
    }
}
