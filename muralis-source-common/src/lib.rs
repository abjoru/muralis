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
    pub source_type: &'static str,
    pub display_name: &'static str,
    pub base: &'static str,
    pub auth: Auth,
    pub search_path: &'static str,
    pub detail_path: &'static str,
    /// Query key for the search term: `"q"` (wallhaven) or `"query"`.
    pub query_key: &'static str,
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
}

/// A source's search-response envelope.
pub trait SearchResponse: DeserializeOwned + Send {
    fn into_previews(self, source_type: &str) -> Vec<WallpaperPreview>;
}

/// A source's detail-response envelope (sometimes the bare item).
pub trait DetailResponse: DeserializeOwned + Send {
    fn into_preview(self, source_type: &str) -> WallpaperPreview;
    /// Parse this source's wallpaper id out of a page URL, or `None` if the
    /// URL doesn't belong to this source.
    fn parse_id(url: &str) -> Option<String>;
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
        self.desc.display_name
    }

    fn source_type(&self) -> &str {
        self.desc.source_type
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

        let mut out: Vec<WallpaperPreview> = Vec::new();
        for upstream in first..=last {
            let upstream_s = upstream.to_string();
            let cap_s = self.desc.per_page_cap.to_string();

            let mut query_params: Vec<(&str, &str)> =
                vec![(self.desc.query_key, query), ("page", &upstream_s)];
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

            for preview in resp.into_previews(self.desc.source_type) {
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
        let Some(id) = D::parse_id(url) else {
            return Ok(None);
        };
        let endpoint = format!("{}{}/{}", self.desc.base, self.desc.detail_path, id);

        let mut headers: Vec<(&str, &str)> = Vec::new();
        let mut query_params: Vec<(&str, &str)> = Vec::new();
        match &self.desc.auth {
            Auth::QueryParam {
                key,
                value: Some(v),
            } => query_params.push((key, v.as_str())),
            Auth::Header { name, value } => headers.push((name, value.as_str())),
            _ => {}
        }

        let (status, bytes) = self.http.get(&endpoint, &headers, &query_params).await?;
        if !status.is_success() {
            return Err(self.err("resolve", &endpoint, status.as_u16().to_string()));
        }
        let resp: D = serde_json::from_slice(&bytes)
            .map_err(|e| self.err("resolve", &endpoint, format!("decode: {e}")))?;
        Ok(Some(resp.into_preview(self.desc.source_type)))
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
            source_type: self.desc.source_type.to_string(),
            op: format!("{op} {detail}"),
            kind,
        }
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
            source_type: "fix",
            display_name: "Fixture",
            base: "https://api.fix.test",
            auth: Auth::None,
            search_path: "/search",
            detail_path: "/photo",
            query_key: "query",
            per_page_param: Some("per_page"),
            per_page_cap: 30,
            block: 1,
            extra_query: Vec::new(),
            server_aspect_param: None,
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
    async fn resolve_url_returns_none_for_foreign_url() {
        let http = Arc::new(StubFetch::ok(r#"{"id":"x","w":1,"h":1}"#));
        let src: RestSource<FixSearch, FixItem> = RestSource::new(descriptor(), http.clone());

        let resolved = src.resolve_url("https://other.example/p/1").await.unwrap();

        assert!(resolved.is_none());
        assert!(http.calls().is_empty(), "should not hit the network");
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
