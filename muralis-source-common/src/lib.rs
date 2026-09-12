//! Shared engine for paged JSON REST wallpaper sources.
//!
//! `RestSource` is the single concrete [`WallpaperSource`] for paged JSON REST
//! APIs (wallhaven, unsplash, pexels). It owns auth injection, request building,
//! pagination, page-filling, JSON parsing, and error context. Per-source crates
//! supply only their response structs and the mapping to `WallpaperPreview` via
//! the [`SearchResponse`] / [`DetailResponse`] traits. See `CONTEXT.md`.

use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use muralis_core::error::{MuralisError, Result};
use muralis_core::models::WallpaperPreview;
use muralis_core::sources::{AspectRatioFilter, WallpaperSource};

pub mod testing;

/// Bytes-level transport seam. The real adapter wraps `reqwest`; tests use a
/// canned stub. Auth injection and JSON parsing live in `RestSource`, not here.
#[async_trait]
pub trait HttpFetch: Send + Sync {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        query: &[(&str, &str)],
    ) -> Result<(StatusCode, bytes::Bytes)>;
}

/// Production [`HttpFetch`] backed by `reqwest`. The only code that touches the
/// HTTP client directly.
pub struct ReqwestFetch(pub reqwest::Client);

#[async_trait]
impl HttpFetch for ReqwestFetch {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        query: &[(&str, &str)],
    ) -> Result<(StatusCode, bytes::Bytes)> {
        let mut req = self.0.get(url).query(query);
        for (name, value) in headers {
            req = req.header(*name, *value);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        Ok((status, bytes))
    }
}

/// Where a source's credential goes.
pub enum Auth {
    /// Appended to the query string, e.g. wallhaven `?apikey=…`.
    QueryParam {
        key: &'static str,
        value: Option<String>,
    },
    /// Multiple query credentials sent together — Gelbooru requires both
    /// `api_key` and `user_id` (mandatory since 2025; anonymous = 401). Secrets
    /// live here, never in `extra_query`.
    QueryParams(Vec<(&'static str, String)>),
    /// Sent as a header, e.g. unsplash `Authorization: Client-ID …`.
    Header {
        name: &'static str,
        value: String,
    },
    None,
}

/// Per-source data a [`RestSource`] is built from. No behaviour beyond the
/// mappers carried by the response traits.
pub struct Descriptor {
    /// Per-host identity + dedup key. `String` (not `&'static str`) because
    /// multi-host sources (boorus) derive it from config `name` at runtime.
    pub source_type: String,
    pub display_name: String,
    pub base: &'static str,
    pub auth: Auth,
    pub search_path: &'static str,
    pub detail_path: &'static str,
    /// Query key for the search term: `"q"` (wallhaven) or `"query"`.
    pub query_key: &'static str,
    /// Folded into the search-term value: the engine sends
    /// `"{tag_prefix} {query}"` (trimmed) as `query_key`. Carries a booru's
    /// `rating:` tag (+ any extra tags); `None` when the query stands alone.
    pub tag_prefix: Option<String>,
    /// Query key for page size, or `None` when the API has no such param
    /// (wallhaven uses a fixed server page size).
    pub per_page_param: Option<&'static str>,
    /// Value sent for `per_page_param` — the upstream page size.
    pub per_page_cap: u32,
    /// Number of upstream pages mapped to one logical page (Block B).
    pub block: u32,
    /// Extra constant query params (categories/purity, orientation=landscape).
    pub extra_query: Vec<(&'static str, String)>,
    /// When `Some(key)`, the server filters by aspect via this query param
    /// (wallhaven `ratios`); client-side filtering is then skipped.
    pub server_aspect_param: Option<&'static str>,
    /// Query key for the page number. Almost always `"page"`; Gelbooru uses
    /// `"pid"`.
    pub page_param: &'static str,
    /// First page index the API uses. `1` for most sources; Gelbooru's `pid` is
    /// `0`-indexed.
    pub page_base: u32,
}

/// A source's search-response envelope.
pub trait SearchResponse: DeserializeOwned + Send {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview>;
}

/// A source's detail-response envelope (sometimes the bare item).
pub trait DetailResponse: DeserializeOwned + Send {
    fn into_preview(self, source_type: &str) -> WallpaperPreview;
    /// Parse this source's wallpaper id out of a page URL, or `None` if the
    /// URL doesn't belong to this source. When [`Self::HOST_SCOPED`] is set the
    /// engine has already host-matched, so the flavor may parse the path alone.
    fn parse_id(url: &str) -> Option<String>;

    /// When `true`, `resolve_url` requires the URL's host to equal the
    /// descriptor `base`'s host before calling [`Self::parse_id`]. Multi-host
    /// boorus set this so two instances of one flavor (yande.re + Konachan,
    /// both `/post/show/N`) don't fight over the same path shape. Single-host
    /// sources whose API host differs from their page host leave it `false`.
    const HOST_SCOPED: bool = false;

    /// Build the `(url, extra_query)` for the detail fetch. Default is
    /// path-based `{base}{detail_path}/{id}` with no extra query — the photo
    /// APIs. Flavors override for a `.json` suffix (danbooru) or a search-by-id
    /// query shape (moebooru has no GET-by-id: `tags=id:{id}`).
    fn detail_request(
        base: &str,
        detail_path: &str,
        search_path: &str,
        id: &str,
    ) -> (String, Vec<(&'static str, String)>) {
        let _ = search_path;
        (format!("{base}{detail_path}/{id}"), Vec::new())
    }
}

/// The deep module: a full [`WallpaperSource`] for paged JSON REST APIs.
pub struct RestSource<S, D> {
    desc: Descriptor,
    http: Arc<dyn HttpFetch>,
    _marker: PhantomData<fn() -> (S, D)>,
}

impl<S, D> RestSource<S, D> {
    pub fn new(desc: Descriptor, http: Arc<dyn HttpFetch>) -> Self {
        Self {
            desc,
            http,
            _marker: PhantomData,
        }
    }
}

#[async_trait]
impl<S, D> WallpaperSource for RestSource<S, D>
where
    S: SearchResponse + 'static,
    D: DetailResponse + 'static,
{
    fn name(&self) -> &str {
        &self.desc.display_name
    }

    fn source_type(&self) -> &str {
        &self.desc.source_type
    }

    async fn search(
        &self,
        query: &str,
        page: u32,
        per_page: u32,
        aspect: AspectRatioFilter,
    ) -> Result<Vec<WallpaperPreview>> {
        let page = page.max(1);
        let block = self.desc.block.max(1);
        let first = (page - 1) * block + 1;
        let last = page * block;
        let url = format!("{}{}", self.desc.base, self.desc.search_path);
        // Server filters by aspect for some sources; only then skip client filter.
        let server_ratio = self
            .desc
            .server_aspect_param
            .zip(aspect.to_wallhaven_ratio());
        let client_filter = self.desc.server_aspect_param.is_none();

        // Compose the search-term value once: a booru folds its `rating:` tag
        // (+ extra tags) ahead of the user query into the single `tags` param.
        let composed_query = self
            .desc
            .tag_prefix
            .as_ref()
            .map(|p| format!("{p} {query}").trim().to_string());

        let mut out: Vec<WallpaperPreview> = Vec::new();
        for upstream in first..=last {
            // Map the internal 1-indexed upstream page onto the API's own page
            // numbering: most APIs are 1-indexed (`page_base` 1); Gelbooru's
            // `pid` is 0-indexed (`page_base` 0).
            let page_num = upstream - 1 + self.desc.page_base;
            let page_num_s = page_num.to_string();
            let cap_s = self.desc.per_page_cap.to_string();
            let q_value: &str = composed_query.as_deref().unwrap_or(query);

            let mut query_params: Vec<(&str, &str)> = vec![
                (self.desc.query_key, q_value),
                (self.desc.page_param, &page_num_s),
            ];
            if let Some(pp) = self.desc.per_page_param {
                query_params.push((pp, &cap_s));
            }
            for (k, v) in &self.desc.extra_query {
                query_params.push((k, v.as_str()));
            }
            if let Some((param, ratio)) = server_ratio {
                query_params.push((param, ratio));
            }

            let mut headers: Vec<(&str, &str)> = Vec::new();
            match &self.desc.auth {
                Auth::QueryParam {
                    key,
                    value: Some(v),
                } => query_params.push((key, v.as_str())),
                Auth::QueryParams(pairs) => {
                    for (k, v) in pairs {
                        query_params.push((k, v.as_str()));
                    }
                }
                Auth::Header { name, value } => headers.push((name, value.as_str())),
                _ => {}
            }

            let (status, bytes) = self.http.get(&url, &headers, &query_params).await?;
            if !status.is_success() {
                return Err(self.err(
                    "search",
                    &format!("{query:?} p{upstream}"),
                    status.as_u16().to_string(),
                ));
            }
            let resp: S = serde_json::from_slice(&bytes).map_err(|e| {
                self.err(
                    "search",
                    &format!("{query:?} p{upstream}"),
                    format!("decode: {e}"),
                )
            })?;

            for preview in resp.into_previews(&self.desc.source_type) {
                if client_filter && !aspect.matches(preview.width, preview.height) {
                    continue;
                }
                out.push(preview);
                if out.len() as u32 >= per_page {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }

    async fn resolve_url(&self, url: &str) -> Result<Option<WallpaperPreview>> {
        // Host-scoped sources (multi-host boorus) reject foreign hosts in the
        // engine *before* delegating to the flavor's path-only `parse_id`, so
        // two instances of one flavor don't fight over the same URL shape.
        if D::HOST_SCOPED && !host_matches(url, self.desc.base) {
            return Ok(None);
        }
        let Some(id) = D::parse_id(url) else {
            return Ok(None);
        };
        let (endpoint, extra) = D::detail_request(
            self.desc.base,
            self.desc.detail_path,
            self.desc.search_path,
            &id,
        );

        let mut headers: Vec<(&str, &str)> = Vec::new();
        let mut query_params: Vec<(&str, &str)> = Vec::new();
        match &self.desc.auth {
            Auth::QueryParam {
                key,
                value: Some(v),
            } => query_params.push((key, v.as_str())),
            Auth::QueryParams(pairs) => {
                for (k, v) in pairs {
                    query_params.push((k, v.as_str()));
                }
            }
            Auth::Header { name, value } => headers.push((name, value.as_str())),
            _ => {}
        }
        for (k, v) in &extra {
            query_params.push((k, v.as_str()));
        }

        let (status, bytes) = self.http.get(&endpoint, &headers, &query_params).await?;
        if !status.is_success() {
            return Err(self.err("resolve", &endpoint, status.as_u16().to_string()));
        }
        let resp: D = serde_json::from_slice(&bytes)
            .map_err(|e| self.err("resolve", &endpoint, format!("decode: {e}")))?;
        Ok(Some(resp.into_preview(&self.desc.source_type)))
    }

    async fn download(&self, preview: &WallpaperPreview) -> Result<bytes::Bytes> {
        let (status, bytes) = self.http.get(&preview.full_url, &[], &[]).await?;
        if !status.is_success() {
            return Err(self.err("download", &preview.full_url, status.as_u16().to_string()));
        }
        Ok(bytes)
    }
}

impl<S, D> RestSource<S, D> {
    fn err(&self, op: &str, detail: &str, kind: String) -> MuralisError {
        MuralisError::Source {
            source_type: self.desc.source_type.clone(),
            op: format!("{op} {detail}"),
            kind,
        }
    }
}

/// Host portion of a URL/base (`scheme://HOST/...`), or `None` if absent.
fn host_of(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    (!host.is_empty()).then_some(host)
}

/// Whether `url`'s host equals `base`'s host (case-insensitive).
fn host_matches(url: &str, base: &str) -> bool {
    match (host_of(url), host_of(base)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::StubFetch;
    use muralis_core::models::SourceType;
    use serde::Deserialize;

    // -- fixture source: response shape `{"items":[{"id","w","h"}]}` --

    #[derive(Deserialize)]
    struct FixSearch {
        items: Vec<FixItem>,
    }
    #[derive(Deserialize)]
    struct FixItem {
        id: String,
        w: u32,
        h: u32,
    }

    impl SearchResponse for FixSearch {
        fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview> {
            self.items
                .into_iter()
                .map(|i| WallpaperPreview {
                    source_type: SourceType::new(source_type),
                    source_id: i.id.clone(),
                    source_url: format!("https://fix.test/{}", i.id),
                    thumbnail_url: format!("https://fix.test/{}/thumb", i.id),
                    full_url: format!("https://fix.test/{}/full", i.id),
                    width: i.w,
                    height: i.h,
                    tags: Vec::new(),
                })
                .collect()
        }
    }

    impl DetailResponse for FixItem {
        fn into_preview(self, source_type: &str) -> WallpaperPreview {
            FixSearch { items: vec![self] }
                .into_previews(source_type)
                .pop()
                .unwrap()
        }
        fn parse_id(url: &str) -> Option<String> {
            url.strip_prefix("https://fix.test/")
                .map(|r| r.trim_end_matches('/').to_string())
        }
    }

    fn descriptor() -> Descriptor {
        Descriptor {
            source_type: "fix".into(),
            display_name: "Fixture".into(),
            base: "https://api.fix.test",
            auth: Auth::None,
            search_path: "/search",
            detail_path: "/photo",
            query_key: "query",
            tag_prefix: None,
            per_page_param: Some("per_page"),
            per_page_cap: 30,
            block: 1,
            extra_query: Vec::new(),
            server_aspect_param: None,
            page_param: "page",
            page_base: 1,
        }
    }

    fn source(desc: Descriptor, http: StubFetch) -> RestSource<FixSearch, FixItem> {
        RestSource::new(desc, Arc::new(http))
    }

    #[tokio::test]
    async fn search_maps_response_to_previews() {
        let body = r#"{"items":[{"id":"a1","w":3840,"h":2160}]}"#;
        let src = source(descriptor(), StubFetch::ok(body));

        let previews = src
            .search("trees", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].source_id, "a1");
        assert_eq!(previews[0].source_type, SourceType::new("fix"));
        assert_eq!(previews[0].width, 3840);
    }

    #[tokio::test]
    async fn search_sends_query_page_and_per_page() {
        let http = Arc::new(StubFetch::ok(r#"{"items":[]}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(descriptor(), http.clone());

        src.search("cats", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.url, "https://api.fix.test/search");
        assert_eq!(call.query_value("query"), Some("cats"));
        assert_eq!(call.query_value("page"), Some("1"));
        assert_eq!(call.query_value("per_page"), Some("30")); // the cap
    }

    #[tokio::test]
    async fn auth_query_param_goes_in_query() {
        let mut desc = descriptor();
        desc.auth = Auth::QueryParam {
            key: "apikey",
            value: Some("secret".into()),
        };
        let http = Arc::new(StubFetch::ok(r#"{"items":[]}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        src.search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(http.last_call().query_value("apikey"), Some("secret"));
    }

    #[tokio::test]
    async fn auth_header_goes_in_headers() {
        let mut desc = descriptor();
        desc.auth = Auth::Header {
            name: "Authorization",
            value: "Client-ID k".into(),
        };
        let http = Arc::new(StubFetch::ok(r#"{"items":[]}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        src.search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.header_value("Authorization"), Some("Client-ID k"));
        assert_eq!(call.query_value("apikey"), None);
    }

    #[tokio::test]
    async fn logical_page_maps_to_block_of_upstream_pages() {
        let mut desc = descriptor();
        desc.block = 3;
        let http = Arc::new(StubFetch::ok_pages(&[
            r#"{"items":[]}"#,
            r#"{"items":[]}"#,
            r#"{"items":[]}"#,
        ]));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        // logical page 2, block 3 -> upstream pages 4,5,6
        src.search("x", 2, 100, AspectRatioFilter::All)
            .await
            .unwrap();

        let pages: Vec<_> = http
            .calls()
            .iter()
            .map(|c| c.query_value("page").unwrap().to_string())
            .collect();
        assert_eq!(pages, vec!["4", "5", "6"]);
    }

    #[tokio::test]
    async fn page_filling_filters_by_aspect_and_caps_at_per_page() {
        // 21:9 ultrawide = 2560x1080; 16:9 = 1920x1080 (filtered out at 21:9)
        let mut desc = descriptor();
        desc.block = 2;
        let wide = r#"{"items":[{"id":"w1","w":2560,"h":1080},{"id":"n1","w":1920,"h":1080},{"id":"w2","w":2560,"h":1080}]}"#;
        let http = Arc::new(StubFetch::ok_pages(&[wide, wide]));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        let previews = src
            .search("x", 1, 3, AspectRatioFilter::Ratio21x9)
            .await
            .unwrap();

        // only the 21:9 items survive; capped at per_page=3
        assert_eq!(previews.len(), 3);
        assert!(previews.iter().all(|p| p.source_id.starts_with('w')));
    }

    #[tokio::test]
    async fn page_filling_stops_at_block_edge_even_if_short() {
        let mut desc = descriptor();
        desc.block = 1; // one upstream page only
        let only_narrow = r#"{"items":[{"id":"n1","w":1920,"h":1080}]}"#;
        let http = Arc::new(StubFetch::ok(only_narrow));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http);

        let previews = src
            .search("x", 1, 10, AspectRatioFilter::Ratio21x9)
            .await
            .unwrap();

        // wanted 10, block exhausted with 0 matches -> empty, no further fetch
        assert!(previews.is_empty());
    }

    #[tokio::test]
    async fn server_aspect_emits_ratio_param_and_skips_client_filter() {
        let mut desc = descriptor();
        desc.server_aspect_param = Some("ratios");
        // a 16:9 item is returned but client filter is OFF, so it survives a 21:9 query
        let body = r#"{"items":[{"id":"s1","w":1920,"h":1080}]}"#;
        let http = Arc::new(StubFetch::ok(body));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        let previews = src
            .search("x", 1, 24, AspectRatioFilter::Ratio21x9)
            .await
            .unwrap();

        assert_eq!(http.last_call().query_value("ratios"), Some("21x9"));
        assert_eq!(previews.len(), 1); // not client-filtered
    }

    #[tokio::test]
    async fn non_success_status_yields_source_error_with_context() {
        let src = source(descriptor(), StubFetch::status(429));

        let err = src
            .search("trees", 2, 24, AspectRatioFilter::All)
            .await
            .unwrap_err();

        match err {
            MuralisError::Source {
                source_type,
                op,
                kind,
            } => {
                assert_eq!(source_type, "fix");
                assert!(op.contains("trees"), "op was {op:?}");
                assert!(op.contains("p2"), "op was {op:?}"); // logical p2, block 1 -> upstream p2
                assert_eq!(kind, "429");
            }
            other => panic!("expected Source error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_url_parses_id_fetches_detail_and_maps() {
        let body = r#"{"id":"d9","w":3000,"h":2000}"#;
        let http = Arc::new(StubFetch::ok(body));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(descriptor(), http.clone());

        let preview = src
            .resolve_url("https://fix.test/d9")
            .await
            .unwrap()
            .expect("should resolve");

        assert_eq!(preview.source_id, "d9");
        assert_eq!(http.last_call().url, "https://api.fix.test/photo/d9");
    }

    #[tokio::test]
    async fn tag_prefix_is_folded_ahead_of_the_query() {
        let mut desc = descriptor();
        desc.query_key = "tags";
        desc.tag_prefix = Some("rating:safe".into());
        let http = Arc::new(StubFetch::ok(r#"{"items":[]}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        src.search("landscape", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        assert_eq!(
            http.last_call().query_value("tags"),
            Some("rating:safe landscape")
        );
    }

    #[tokio::test]
    async fn tag_prefix_alone_when_query_empty() {
        let mut desc = descriptor();
        desc.query_key = "tags";
        desc.tag_prefix = Some("rating:safe".into());
        let http = Arc::new(StubFetch::ok(r#"{"items":[]}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        src.search("", 1, 24, AspectRatioFilter::All).await.unwrap();

        assert_eq!(http.last_call().query_value("tags"), Some("rating:safe"));
    }

    #[tokio::test]
    async fn auth_query_params_injects_all_pairs() {
        let mut desc = descriptor();
        desc.auth = Auth::QueryParams(vec![("api_key", "abc".into()), ("user_id", "7".into())]);
        let http = Arc::new(StubFetch::ok(r#"{"items":[]}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        src.search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();

        let call = http.last_call();
        assert_eq!(call.query_value("api_key"), Some("abc"));
        assert_eq!(call.query_value("user_id"), Some("7"));
    }

    #[tokio::test]
    async fn page_param_and_base_map_zero_indexed_pages() {
        let mut desc = descriptor();
        desc.page_param = "pid";
        desc.page_base = 0;
        desc.block = 3;
        let http = Arc::new(StubFetch::ok_pages(&[
            r#"{"items":[]}"#,
            r#"{"items":[]}"#,
            r#"{"items":[]}"#,
        ]));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(desc, http.clone());

        // logical page 1, block 3, 0-indexed → pid 0,1,2 under the "pid" key
        src.search("x", 1, 100, AspectRatioFilter::All)
            .await
            .unwrap();

        let pids: Vec<_> = http
            .calls()
            .iter()
            .map(|c| c.query_value("pid").unwrap().to_string())
            .collect();
        assert_eq!(pids, vec!["0", "1", "2"]);
        // the default "page" key is unused once page_param is overridden
        assert_eq!(http.last_call().query_value("page"), None);
    }

    // -- host-scoped resolve fixture: parse_id is path-only, host check is the
    //    engine's job (HOST_SCOPED) --

    #[derive(Deserialize)]
    struct HostScopedItem {
        id: String,
        w: u32,
        h: u32,
    }

    impl DetailResponse for HostScopedItem {
        fn into_preview(self, source_type: &str) -> WallpaperPreview {
            WallpaperPreview {
                source_type: SourceType::new(source_type),
                source_id: self.id,
                source_url: String::new(),
                thumbnail_url: String::new(),
                full_url: String::new(),
                width: self.w,
                height: self.h,
                tags: Vec::new(),
            }
        }
        // path-only: "post/show/123" -> "123"
        fn parse_id(url: &str) -> Option<String> {
            let path = url.split_once("://")?.1.split_once('/')?.1;
            let id = path.strip_prefix("post/show/")?;
            let id = id.split(['/', '?', '#']).next()?;
            (!id.is_empty()).then(|| id.to_string())
        }
        const HOST_SCOPED: bool = true;
    }

    #[tokio::test]
    async fn host_scoped_rejects_foreign_host_without_fetching() {
        let mut desc = descriptor();
        desc.base = "https://yande.re";
        let http = Arc::new(StubFetch::ok(r#"{"id":"123","w":1,"h":1}"#));
        let src: RestSource<FixSearch, HostScopedItem> = RestSource::new(desc, http.clone());

        // same path shape, different host -> rejected before any network call
        let resolved = src
            .resolve_url("https://konachan.com/post/show/123")
            .await
            .unwrap();

        assert!(resolved.is_none());
        assert!(http.calls().is_empty(), "must not hit the network");
    }

    #[tokio::test]
    async fn host_scoped_resolves_matching_host() {
        let mut desc = descriptor();
        desc.base = "https://yande.re";
        let http = Arc::new(StubFetch::ok(r#"{"id":"123","w":3440,"h":1440}"#));
        let src: RestSource<FixSearch, HostScopedItem> = RestSource::new(desc, http.clone());

        let preview = src
            .resolve_url("https://yande.re/post/show/123")
            .await
            .unwrap()
            .expect("matching host resolves");

        assert_eq!(preview.source_id, "123");
    }

    #[tokio::test]
    async fn resolve_url_returns_none_for_foreign_url() {
        let http = Arc::new(StubFetch::ok(r#"{"id":"x","w":1,"h":1}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(descriptor(), http.clone());

        let resolved = src.resolve_url("https://other.example/p/1").await.unwrap();

        assert!(resolved.is_none());
        assert!(http.calls().is_empty(), "should not hit the network");
    }

    /// One Source needs a `Referer` — ultrawidewallpapers.net gates its
    /// full-resolution images on it — and it sends its own, locally. Nothing
    /// reaches a REST API through this engine claiming to come from a page:
    /// a header that says where a request came from is a statement about one
    /// site's gate, not a thing to leak to every host muralis talks to.
    #[tokio::test]
    async fn no_rest_source_says_where_its_request_came_from() {
        let http = Arc::new(StubFetch::new(vec![
            (
                StatusCode::OK,
                bytes::Bytes::from_static(br#"{"items":[{"id":"a","w":1,"h":1}]}"#),
            ),
            (
                StatusCode::OK,
                bytes::Bytes::from_static(br#"{"id":"a","w":1,"h":1}"#),
            ),
            (StatusCode::OK, bytes::Bytes::from_static(b"IMGDATA")),
        ]));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(descriptor(), http.clone());
        let preview = WallpaperPreview {
            source_type: SourceType::new("fix"),
            source_id: "a".into(),
            source_url: "https://fix.test/a".into(),
            thumbnail_url: "t".into(),
            full_url: "https://cdn.fix.test/a.jpg".into(),
            width: 1,
            height: 1,
            tags: Vec::new(),
        };

        src.search("x", 1, 24, AspectRatioFilter::All)
            .await
            .unwrap();
        src.resolve_url("https://fix.test/p/1").await.unwrap();
        src.download(&preview).await.unwrap();

        for call in http.calls() {
            assert_eq!(
                call.header_value("Referer"),
                None,
                "no Referer on {}",
                call.url
            );
        }
        assert_eq!(http.calls().len(), 3, "search, resolve and download");
    }

    #[tokio::test]
    async fn download_fetches_full_url_bytes() {
        let http = Arc::new(StubFetch::ok("IMGDATA"));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(descriptor(), http.clone());
        let preview = WallpaperPreview {
            source_type: SourceType::new("fix"),
            source_id: "a".into(),
            source_url: "https://fix.test/a".into(),
            thumbnail_url: "t".into(),
            full_url: "https://cdn.fix.test/a.jpg".into(),
            width: 1,
            height: 1,
            tags: Vec::new(),
        };

        let bytes = src.download(&preview).await.unwrap();

        assert_eq!(&bytes[..], b"IMGDATA");
        assert_eq!(http.last_call().url, "https://cdn.fix.test/a.jpg");
    }
}
