//! The **drift check**: does the live site still parse?
//!
//! The **Ultrawide Source** parses markup it does not own. Its parser is
//! specified by fixtures captured on one day, so the offline suite goes on
//! passing however far the site moves — confidence grows exactly as accuracy
//! decays. This module closes that gap by running the *production* parser
//! ([`parse_gallery`]) over a freshly fetched window of the **Gallery
//! endpoint**, and saying which of several things happened.
//!
//! It is not part of the suite: it reaches the network, so it runs only from a
//! scheduled workflow (`.github/workflows/ultrawide-drift.yml`), never on push
//! and never under `cargo test`. Its volume obeys the **Access posture** — one
//! window, one gallery page, and a bounded handful of one-byte verification
//! requests, all carrying the **muralis User-Agent**.
//!
//! "Did anything parse?" is not the question. The Source once shipped reading
//! `src` on a page that lazy-loads, and three quarters of every category's
//! Previews carried a 1x1 placeholder: a full complement of cards, every one
//! unusable. So the check asks whether what parsed is *usable* — fields that
//! hold locations rather than values, a plausible number of them, and a sample
//! the site will actually serve — and names which of those questions failed.
//!
//! It asks one more question the markup cannot answer on its own: does the
//! **shipped tag vocabulary** still match the set the gallery page publishes?
//! A vocabulary that has drifted is otherwise found by a user selecting a tag
//! that silently returns nothing, because an unknown tag and a genuinely empty
//! one are the same empty fragment upstream.
//!
//! A transport failure is *not* drift. A timeout, a 5xx and a rate limit are
//! transient and say nothing about the markup.

use reqwest::StatusCode;
use scraper::{Html, Selector};
use std::sync::LazyLock;

use muralis_core::models::WallpaperPreview;
use muralis_source_common::HttpFetch;

use super::{gallery_request, parse_gallery, user_agent, CARD_SEL, DEFAULT_TAGS, GALLERY_PAGE};

static TITLE_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("title").expect("valid selector"));
static ANCHOR_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a").expect("valid selector"));
/// The gallery page publishes its vocabulary as `data-tag` attributes, one per
/// chip. Matched on the attribute alone rather than on `span.tag`: a restyle
/// that is not a vocabulary change must not read as one.
static TAG_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("[data-tag]").expect("valid selector"));

/// What the parser expects of a window, phrased for a maintainer reading a
/// failed run rather than for a stack trace. Only the no-cards-at-all report
/// needs it: every other failure has an offending value of its own to show.
const EXPECTED: &str = "at least one card matching `a[data-filename][href]` wrapping an `img` \
     whose `src` holds a fetchable thumbnail URL";

/// The outcome of one drift check. Distinct answers, because conflating them
/// is the failure mode this exists to prevent: only [`Drifted`] means the
/// parser — or the shipped vocabulary — is wrong.
///
/// [`Drifted`]: DriftCheck::Drifted
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftCheck {
    /// The window parsed into usable **Previews** and the vocabulary matched.
    /// Quiet success — nothing to file, nothing to say.
    Parsed {
        tag: String,
        url: String,
        cards: usize,
        /// How many URLs were fetched to confirm the site still serves them.
        verified: usize,
        /// How many tags the gallery page published, all of them ours.
        vocabulary: usize,
    },
    /// What arrived arrived intact, and what was made of it is not usable.
    /// This, and only this, is drift.
    Drifted {
        tag: String,
        url: String,
        /// Which question tripped, and on what value.
        failure: DriftFailure,
    },
    /// The site asked us to slow down. Transient; nothing was learned.
    RateLimited {
        tag: String,
        url: String,
        status: u16,
    },
    /// A non-success status that is not a rate limit — a 5xx, or a redirect
    /// landing on an error page. Transient; nothing was learned.
    Rejected {
        tag: String,
        url: String,
        status: u16,
    },
    /// The request never completed: DNS, TLS, connection, timeout.
    Unreachable {
        tag: String,
        url: String,
        detail: String,
    },
}

/// Which check failed, and the offending value. A **Card** count alone is not
/// an answer: the Source once shipped reading `src` on a page that lazy-loads,
/// and produced a full complement of Previews every one of which carried a
/// 1x1 placeholder. So the check asks whether what parsed is *usable*, and
/// says which of those questions got the wrong answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftFailure {
    /// Nothing in what arrived matched the card selector.
    NoCards {
        /// What the parser looks for.
        expected: String,
        /// Enough of what actually arrived to tell a markup change apart from
        /// an error page: size, `<title>`, and how many anchors were present.
        saw: String,
    },
    /// Cards were there, and a field that must hold a fetchable URL does not.
    UnusableField { detail: String },
    /// Usable cards, implausibly few or many of them.
    ImplausibleCount {
        cards: usize,
        min: usize,
        max: usize,
    },
    /// The gallery page no longer publishes the tags the crate ships. Named
    /// both ways round, because an addition and a removal are different jobs:
    /// one is a menu to extend, the other a tag users can still select and get
    /// nothing for.
    VocabularyDrifted {
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// A well-formed URL the site does not serve.
    UnresolvableUrl {
        field: &'static str,
        card: String,
        url: String,
        status: u16,
    },
}

impl DriftFailure {
    /// The name of the check that tripped, so a report distinguishes "no
    /// cards" from "cards without usable fields" from "fields that do not
    /// resolve" without anyone reading the prose closely.
    pub fn check(&self) -> &'static str {
        match self {
            DriftFailure::NoCards { .. } => "cards-present",
            DriftFailure::UnusableField { .. } => "fields-usable",
            DriftFailure::ImplausibleCount { .. } => "card-count",
            DriftFailure::VocabularyDrifted { .. } => "tag-vocabulary",
            DriftFailure::UnresolvableUrl { .. } => "urls-resolve",
        }
    }

    fn detail(&self) -> String {
        match self {
            DriftFailure::NoCards { expected, saw } => {
                format!("the parser expected {expected}, and saw {saw}")
            }
            DriftFailure::UnusableField { detail } => detail.clone(),
            DriftFailure::ImplausibleCount { cards, min, max } => format!(
                "{cards} cards parsed, outside the plausible band of {min}-{max} — \
                 a window yielding a handful where it used to yield a full page is a \
                 parser reading something that is no longer the card list"
            ),
            DriftFailure::VocabularyDrifted { added, removed } => format!(
                "the gallery page publishes tags the crate does not ship ({added}), \
                 and the crate ships tags it no longer publishes ({removed}) — a \
                 shipped tag the site has dropped returns an empty page rather than \
                 an error, so a user would read it as 'nothing found'",
                added = list(added),
                removed = list(removed),
            ),
            DriftFailure::UnresolvableUrl {
                field,
                card,
                url,
                status,
            } => format!(
                "the {field} of card '{card}' is well-formed and the site does not \
                 serve it: HTTP {status} for {url}"
            ),
        }
    }
}

/// A list of names for a report, or `none` — an empty list reads as a mistake
/// when it is rendered as nothing at all.
fn list(names: &[String]) -> String {
    if names.is_empty() {
        return "none".to_string();
    }
    names.join(", ")
}

impl DriftCheck {
    /// True only for an answer that arrived intact and is wrong. Everything
    /// else is a transport story, and filing it as drift would cry wolf.
    pub fn is_drift(&self) -> bool {
        matches!(self, DriftCheck::Drifted { .. })
    }

    /// `0` quiet success, `1` drift, `2` transient. The scheduled workflow
    /// files an issue on `1` alone.
    pub fn exit_code(&self) -> i32 {
        match self {
            DriftCheck::Parsed { .. } => 0,
            DriftCheck::Drifted { .. } => 1,
            DriftCheck::RateLimited { .. }
            | DriftCheck::Rejected { .. }
            | DriftCheck::Unreachable { .. } => 2,
        }
    }

    /// A one-word tag the workflow can branch on without parsing prose.
    pub fn tag(&self) -> &'static str {
        match self {
            DriftCheck::Parsed { .. } => "PARSED",
            DriftCheck::Drifted { .. } => "DRIFTED",
            DriftCheck::RateLimited { .. } => "RATE_LIMITED",
            DriftCheck::Rejected { .. } => "REJECTED",
            DriftCheck::Unreachable { .. } => "UNREACHABLE",
        }
    }

    /// The diagnosable report. Always names the tag and the URL being fetched,
    /// so a failed run is readable without opening the code.
    pub fn report(&self) -> String {
        match self {
            DriftCheck::Parsed {
                tag,
                url,
                cards,
                verified,
                vocabulary,
            } => format!(
                "PARSED tag '{tag}' at {url}: {cards} cards, \
                 {verified} sampled URLs served, {vocabulary} published tags matched"
            ),
            DriftCheck::Drifted { tag, url, failure } => format!(
                "DRIFTED tag '{tag}' at {url}: check '{check}' failed — {detail}. \
                 The site has most likely changed; recapture the fixture by hand — \
                 a new capture is a new parser contract.",
                check = failure.check(),
                detail = failure.detail(),
            ),
            DriftCheck::RateLimited { tag, url, status } => format!(
                "RATE_LIMITED tag '{tag}' while fetching {url}: HTTP {status}. \
                 Transient — the site asked us to slow down, so this says nothing \
                 about the markup."
            ),
            DriftCheck::Rejected { tag, url, status } => format!(
                "REJECTED tag '{tag}' while fetching {url}: HTTP {status}. \
                 Transient — nothing arrived to read, so this says nothing about \
                 the markup."
            ),
            DriftCheck::Unreachable { tag, url, detail } => format!(
                "UNREACHABLE tag '{tag}' while fetching {url}: {detail}. \
                 Transient — the request never completed, so this says nothing \
                 about the markup."
            ),
        }
    }
}

/// What the check considers plausible, and how much it is willing to spend
/// finding out. One window, one gallery page and `2 * sample` verification
/// requests is the whole weekly budget, and the **Access posture** is why that
/// is a constant rather than a crawl.
pub struct DriftLimits {
    /// The window asked of the endpoint — the same size a user's page is, not
    /// the uncapped `limit` the endpoint would honour.
    pub window: u32,
    /// Fewest distinct **Cards** a full window may yield and still be
    /// believed. A window asked for 24 and answered with three is a parser
    /// reading something that is no longer the card list.
    pub min_cards: usize,
    /// Most it may yield. A window cannot exceed its own `limit`, so more than
    /// that is a selector that has started matching something else.
    pub max_cards: usize,
    /// How many **Previews** get both of their URLs fetched.
    pub sample: usize,
}

impl Default for DriftLimits {
    fn default() -> Self {
        Self {
            window: 24,
            min_cards: 12,
            max_cards: 24,
            sample: 2,
        }
    }
}

/// Verification asks only whether the site serves the URL, so it asks for one
/// byte rather than a 7680x2160 master. A server that ignores `Range` answers
/// `200` instead of `206`; both are success and both answer the question.
const ONE_BYTE: &str = "bytes=0-0";

/// Fetch one window of the **Gallery endpoint**, run the production parser
/// over it, and ask whether what came out is usable — then ask whether the
/// shipped vocabulary still matches the gallery page.
///
/// One window, one gallery page and a bounded handful of verification
/// requests, whatever the outcome: there is no retry, because a retry would
/// double the run's traffic to distinguish nothing a maintainer cannot
/// distinguish a week later.
pub async fn check_live_gallery(http: &dyn HttpFetch, tag: &str) -> DriftCheck {
    check_live_gallery_with(http, tag, &DriftLimits::default()).await
}

/// [`check_live_gallery`] with the budget spelled out.
pub async fn check_live_gallery_with(
    http: &dyn HttpFetch,
    tag: &str,
    limits: &DriftLimits,
) -> DriftCheck {
    let url = gallery_request(&[tag], 0, limits.window);
    let tag = tag.to_string();

    let body = match fetch(http, &tag, &url, &[]).await {
        Ok(body) => body,
        Err(outcome) => return outcome,
    };

    let html = String::from_utf8_lossy(&body);
    let previews = match parse_gallery(&html, std::slice::from_ref(&tag), &url) {
        Ok(previews) if !previews.is_empty() => previews,
        refused => {
            // Which question tripped is decided by whether the fragment had
            // cards at all, counted with the parser's own selector rather than
            // by a second parser. An *empty* fragment is the end-of-results
            // signal in production, and here it is the same no-cards answer:
            // the tag the check asks for is one the site publishes.
            let failure = if count_cards(&html) == 0 {
                DriftFailure::NoCards {
                    expected: EXPECTED.to_string(),
                    saw: describe(&body),
                }
            } else {
                DriftFailure::UnusableField {
                    detail: match refused {
                        Err(e) => format!("the parser refused the window: {e}"),
                        Ok(_) => "the parser returned no card and no error".to_string(),
                    },
                }
            };
            return DriftCheck::Drifted { tag, url, failure };
        }
    };

    // Every card, not a sample: an unusable field costs nothing to spot and
    // means the same thing wherever in the window it turns up. This is the
    // check the Source lacked when it shipped reading `src` on a page that
    // lazy-loads — a full complement of Previews, every one unusable.
    for preview in &previews {
        if let Some(failure) = unusable_field(preview, &url) {
            return DriftCheck::Drifted { tag, url, failure };
        }
    }

    // Decided from the window already in hand, before a single further request
    // is spent.
    let cards = previews.len();
    if cards < limits.min_cards || cards > limits.max_cards {
        return DriftCheck::Drifted {
            tag,
            url,
            failure: DriftFailure::ImplausibleCount {
                cards,
                min: limits.min_cards,
                max: limits.max_cards,
            },
        };
    }

    // One page, asked once: the question the markup cannot answer. A user
    // selecting a tag the site has dropped gets an empty fragment, which is
    // indistinguishable from a tag with nothing in it.
    let vocabulary = match check_vocabulary(http, &tag).await {
        Ok(published) => published,
        Err(outcome) => return outcome,
    };

    // A sample, not every card: this is the part that costs requests.
    let mut verified = 0;
    for (field, card, target) in sample_urls(&previews, limits.sample) {
        match verify(http, &tag, &url, field, card, target).await {
            Some(outcome) => return outcome,
            None => verified += 1,
        }
    }

    DriftCheck::Parsed {
        tag,
        url,
        cards,
        verified,
        vocabulary,
    }
}

/// One GET, with the transport outcomes already sorted from the body. `Err`
/// carries the outcome to report and stop on.
async fn fetch(
    http: &dyn HttpFetch,
    tag: &str,
    url: &str,
    extra_headers: &[(&str, &str)],
) -> std::result::Result<bytes::Bytes, DriftCheck> {
    let mut headers = vec![("User-Agent", user_agent())];
    headers.extend_from_slice(extra_headers);
    let (status, body) = match http.get(url, &headers, &[]).await {
        Ok(response) => response,
        Err(e) => {
            return Err(DriftCheck::Unreachable {
                tag: tag.to_string(),
                url: url.to_string(),
                detail: e.to_string(),
            })
        }
    };
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(DriftCheck::RateLimited {
            tag: tag.to_string(),
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    // Any non-success is transient: nothing arrived, so there is nothing to
    // conclude about the markup.
    if !status.is_success() {
        return Err(DriftCheck::Rejected {
            tag: tag.to_string(),
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    Ok(body)
}

/// Does the gallery page still publish exactly the tags the crate ships?
/// `Ok` is how many it published, all of them ours.
async fn check_vocabulary(
    http: &dyn HttpFetch,
    tag: &str,
) -> std::result::Result<usize, DriftCheck> {
    let body = fetch(http, tag, GALLERY_PAGE, &[]).await?;
    let published = published_tags(&String::from_utf8_lossy(&body));

    let shipped: Vec<String> = DEFAULT_TAGS.iter().map(|t| (*t).to_string()).collect();
    let added: Vec<String> = published
        .iter()
        .filter(|t| !shipped.contains(t))
        .cloned()
        .collect();
    let removed: Vec<String> = shipped
        .iter()
        .filter(|t| !published.contains(t))
        .cloned()
        .collect();
    if added.is_empty() && removed.is_empty() {
        return Ok(published.len());
    }
    Err(DriftCheck::Drifted {
        tag: tag.to_string(),
        url: GALLERY_PAGE.to_string(),
        failure: DriftFailure::VocabularyDrifted { added, removed },
    })
}

/// The tags the gallery page publishes, deduplicated, in the page's own order.
/// Order is not part of the comparison — the set is.
fn published_tags(html: &str) -> Vec<String> {
    let doc = Html::parse_document(html);
    let mut tags: Vec<String> = Vec::new();
    for element in doc.select(&TAG_SEL) {
        let Some(name) = element.value().attr("data-tag").map(str::trim) else {
            continue;
        };
        if !name.is_empty() && !tags.iter().any(|t| t == name) {
            tags.push(name.to_string());
        }
    }
    tags
}

/// One verification request. `None` means the site served it; `Some` is the
/// outcome to report and stop on — drift for a URL the site does not have,
/// transient for a rate limit, a 5xx or a request that never completed.
async fn verify(
    http: &dyn HttpFetch,
    tag: &str,
    window: &str,
    field: &'static str,
    card: &str,
    target: &str,
) -> Option<DriftCheck> {
    let (status, _) = match http
        .get(
            target,
            &[("User-Agent", user_agent()), ("Range", ONE_BYTE)],
            &[],
        )
        .await
    {
        Ok(response) => response,
        Err(e) => {
            return Some(DriftCheck::Unreachable {
                tag: tag.to_string(),
                url: target.to_string(),
                detail: e.to_string(),
            })
        }
    };
    if status.is_success() {
        return None;
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Some(DriftCheck::RateLimited {
            tag: tag.to_string(),
            url: target.to_string(),
            status: status.as_u16(),
        });
    }
    if status.is_server_error() {
        return Some(DriftCheck::Rejected {
            tag: tag.to_string(),
            url: target.to_string(),
            status: status.as_u16(),
        });
    }
    // A 404 or a 403 on a URL the parser just extracted is not a transport
    // story: the parser built a URL the site does not have.
    Some(DriftCheck::Drifted {
        tag: tag.to_string(),
        url: window.to_string(),
        failure: DriftFailure::UnresolvableUrl {
            field,
            card: card.to_string(),
            url: target.to_string(),
            status: status.as_u16(),
        },
    })
}

/// The first field of a **Preview** that holds something other than a URL the
/// GUI could fetch, phrased for a maintainer.
fn unusable_field(preview: &WallpaperPreview, request_url: &str) -> Option<DriftFailure> {
    let fields = [
        ("thumbnail URL", &preview.thumbnail_url),
        ("master URL", &preview.full_url),
    ];
    for (field, value) in fields {
        let trimmed = value.trim();
        let why = if trimmed.is_empty() {
            "absent"
        } else if trimmed.to_ascii_lowercase().starts_with("data:") {
            // A value, not a location: the lazy placeholder itself. It loads
            // successfully and paints nothing, so a Preview carrying one fails
            // silently — the grid renders blank and no error is ever raised.
            "a data: URI rather than a fetchable URL"
        } else if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            "not an absolute http(s) URL"
        } else if trimmed == request_url {
            // An empty `href` resolves to what it was found in, which is a
            // location — just not of an image.
            "the request URL itself rather than an image"
        } else {
            continue;
        };
        return Some(DriftFailure::UnusableField {
            detail: format!(
                "the {field} of card '{card}' is {why}: {value}",
                card = preview.source_id,
                value = clip(trimmed),
            ),
        });
    }
    None
}

/// Enough of a value to recognise it, without pasting a whole base64 payload
/// into an issue body.
fn clip(value: &str) -> String {
    const LIMIT: usize = 96;
    if value.chars().count() <= LIMIT {
        return value.to_string();
    }
    format!("{}...", value.chars().take(LIMIT).collect::<String>())
}

/// Both URLs of each sampled **Preview**, in the order the window listed them.
fn sample_urls(previews: &[WallpaperPreview], sample: usize) -> Vec<(&'static str, &str, &str)> {
    spread(previews.len(), sample)
        .into_iter()
        .flat_map(|i| {
            let p = &previews[i];
            [
                (
                    "thumbnail URL",
                    p.source_id.as_str(),
                    p.thumbnail_url.as_str(),
                ),
                ("master URL", p.source_id.as_str(), p.full_url.as_str()),
            ]
        })
        .collect()
}

/// Evenly spread indices, so a sample of two is the first card and the last —
/// both ends of the window, not two neighbours from the top of it.
fn spread(len: usize, sample: usize) -> Vec<usize> {
    let sample = sample.min(len);
    match sample {
        0 => Vec::new(),
        1 => vec![0],
        n => (0..n).map(|k| k * (len - 1) / (n - 1)).collect(),
    }
}

/// How many cards the fragment has, by the parser's own selector.
fn count_cards(html: &str) -> usize {
    Html::parse_fragment(html).select(&CARD_SEL).count()
}

/// A shape summary of what arrived — deliberately *not* a second parser. It
/// extracts no card and builds no **Preview**; it only measures the document
/// so a markup change reads differently from a captcha or a 404 body served
/// with a 200.
fn describe(body: &[u8]) -> String {
    let html = String::from_utf8_lossy(body);
    let doc = Html::parse_document(&html);
    let title = doc
        .select(&TITLE_SEL)
        .next()
        .map(|t| t.text().collect::<String>().trim().to_string())
        .unwrap_or_else(|| "<no title>".to_string());
    let anchors = doc.select(&ANCHOR_SEL).count();
    let cards = doc.select(&CARD_SEL).count();
    format!(
        "{bytes} bytes, title {title:?}, {anchors} <a> elements, {cards} of them cards",
        bytes = body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use muralis_core::error::{MuralisError, Result};
    use muralis_source_common::testing::StubFetch;
    use std::sync::Arc;

    /// One captured window of the endpoint, and the gallery page trimmed to
    /// the tag cloud it publishes.
    const WINDOW: &str = include_str!("../tests/fixtures/gallery-dark-page-1.html");
    const GALLERY: &str = include_str!("../tests/fixtures/gallery-tags.html");
    const DRIFTED: &str = include_str!("../tests/fixtures/gallery-drifted.html");

    /// The window the check asks for, then the gallery page, then one canned
    /// response per verification request — the order the check spends them in.
    fn run(window: &str, gallery: &str, verifications: &[u16]) -> StubFetch {
        let mut responses = vec![
            (StatusCode::OK, bytes::Bytes::from(window.to_string())),
            (StatusCode::OK, bytes::Bytes::from(gallery.to_string())),
        ];
        responses.extend(verifications.iter().map(|c| {
            (
                StatusCode::from_u16(*c).expect("a status"),
                bytes::Bytes::new(),
            )
        }));
        StubFetch::new(responses)
    }

    fn window_url() -> String {
        gallery_request(&["Dark"], 0, DriftLimits::default().window)
    }

    /// A transport that never completes — DNS, TLS or timeout, from the
    /// check's point of view all the same shape: `Err` before a status.
    struct BrokenFetch;

    #[async_trait]
    impl HttpFetch for BrokenFetch {
        async fn get(
            &self,
            _url: &str,
            _headers: &[(&str, &str)],
            _query: &[(&str, &str)],
        ) -> Result<(StatusCode, bytes::Bytes)> {
            Err(MuralisError::Io(std::io::Error::other(
                "connection timed out",
            )))
        }
    }

    #[tokio::test]
    async fn a_window_whose_cards_are_usable_and_served_is_a_quiet_success() {
        let check = check_live_gallery(&run(WINDOW, GALLERY, &[200; 4]), "Dark").await;

        assert_eq!(
            check,
            DriftCheck::Parsed {
                tag: "Dark".to_string(),
                url: window_url(),
                cards: 24,
                verified: 4,
                vocabulary: DEFAULT_TAGS.len(),
            }
        );
        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 0, "quiet: nothing filed, nothing said");
    }

    /// The check exercises the production path: it asks the endpoint for a
    /// window, not a category page the Source no longer fetches.
    #[tokio::test]
    async fn the_check_asks_the_endpoint_for_a_window_and_the_gallery_for_its_vocabulary() {
        let stub = Arc::new(run(WINDOW, GALLERY, &[200; 4]));

        check_live_gallery(stub.as_ref(), "Dark").await;

        let calls = stub.calls();
        assert_eq!(
            calls[0].url,
            "https://ultrawidewallpapers.net/gallery_load.php?offset=0&limit=24&tag=Dark"
        );
        assert_eq!(calls[1].url, "https://ultrawidewallpapers.net/gallery");
    }

    #[tokio::test]
    async fn the_run_costs_one_window_one_gallery_page_and_a_bounded_sample() {
        let limits = DriftLimits {
            sample: 2,
            ..DriftLimits::default()
        };
        let stub = Arc::new(run(WINDOW, GALLERY, &[200; 4]));

        check_live_gallery_with(stub.as_ref(), "Dark", &limits).await;

        let calls = stub.calls();
        assert_eq!(
            calls.len(),
            2 + 2 * limits.sample,
            "one window, one gallery page, both URLs of each sampled card — no retry"
        );
        for call in &calls {
            let ua = call
                .header_value("User-Agent")
                .expect("every request identifies itself exactly as the Source does");
            assert!(ua.starts_with("muralis/"), "{ua}");
        }
        for call in &calls[2..] {
            assert_eq!(
                call.header_value("Range"),
                Some("bytes=0-0"),
                "verification asks whether the site serves the URL, not for the master"
            );
        }
    }

    #[tokio::test]
    async fn the_sample_spans_the_window_rather_than_its_first_two_cards() {
        let stub = Arc::new(run(WINDOW, GALLERY, &[200; 4]));

        check_live_gallery(stub.as_ref(), "Dark").await;

        let sampled: Vec<String> = stub.calls()[2..].iter().map(|c| c.url.clone()).collect();
        assert!(
            sampled.iter().any(|u| u.contains("aishot-5792")),
            "the first card: {sampled:?}"
        );
        assert!(
            sampled.iter().any(|u| u.contains("aishot-5699")),
            "and the last: {sampled:?}"
        );
    }

    #[tokio::test]
    async fn a_window_that_parses_to_no_cards_is_drift_and_says_what_it_saw() {
        let check = check_live_gallery(&StubFetch::ok(DRIFTED), "Dark").await;

        assert!(check.is_drift());
        assert_eq!(check.exit_code(), 1);
        let report = check.report();
        assert!(report.contains("'Dark'"), "{report}");
        assert!(
            report.contains(&window_url()),
            "the failure names the request it made: {report}"
        );
        assert!(
            report.contains("cards-present"),
            "and which check tripped: {report}"
        );
        assert!(
            report.contains("a[data-filename][href]"),
            "and what the parser expected: {report}"
        );
        assert!(
            report.contains("bytes") && report.contains("<a> elements"),
            "and enough of what arrived to tell markup drift from an error page: {report}"
        );
    }

    /// In production an empty fragment is the end of the results. Here it is
    /// not: the check asks for the first window of a tag the site publishes,
    /// so nothing coming back is drift, not an answer.
    #[tokio::test]
    async fn an_empty_window_for_a_published_tag_is_drift_rather_than_an_end_of_results() {
        let check = check_live_gallery(&StubFetch::ok(""), "Dark").await;

        assert!(check.is_drift(), "{}", check.report());
        assert!(
            check.report().contains("cards-present"),
            "{}",
            check.report()
        );
    }

    #[tokio::test]
    async fn a_card_count_outside_the_plausible_band_is_drift_even_though_cards_parsed() {
        let thinned = DriftLimits {
            min_cards: 50,
            ..DriftLimits::default()
        };

        let check =
            check_live_gallery_with(&run(WINDOW, GALLERY, &[200; 4]), "Dark", &thinned).await;

        assert!(check.is_drift(), "{}", check.report());
        let report = check.report();
        assert!(report.contains("card-count"), "{report}");
        assert!(
            report.contains("24 cards") && report.contains("50"),
            "the report shows what it counted and the band it wanted: {report}"
        );
    }

    #[tokio::test]
    async fn the_band_is_decided_before_a_single_further_request_is_spent() {
        let thinned = DriftLimits {
            min_cards: 50,
            ..DriftLimits::default()
        };
        let stub = Arc::new(run(WINDOW, GALLERY, &[200; 4]));

        check_live_gallery_with(stub.as_ref(), "Dark", &thinned).await;

        assert_eq!(
            stub.calls().len(),
            1,
            "the band is decided from the window already fetched"
        );
    }

    /// The capture, with one card's master link rewritten. Not a new fixture:
    /// the committed capture stays the parser's only input, and this is the
    /// same edit the drift copy makes — one attribute, by hand.
    fn with_first_master(href: &str) -> String {
        WINDOW.replacen(
            r#"href="wallpapers/329/highres/aishot-5792.jpg""#,
            &format!(r#"href="{href}""#),
            1,
        )
    }

    #[tokio::test]
    async fn a_master_url_that_is_a_placeholder_rather_than_a_location_is_drift() {
        let window = with_first_master(
            "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
        );

        let check = check_live_gallery(&run(&window, GALLERY, &[200; 4]), "Dark").await;

        assert!(check.is_drift(), "{}", check.report());
        let report = check.report();
        assert!(report.contains("fields-usable"), "{report}");
        assert!(report.contains("master URL"), "{report}");
        assert!(
            report.contains("aishot-5792.jpg") && report.contains("data:image/gif"),
            "the report shows the offending card and its value: {report}"
        );
    }

    #[tokio::test]
    async fn an_unusable_field_is_found_before_a_single_further_request_is_spent() {
        let stub = Arc::new(run(
            &with_first_master("data:image/gif;base64,AAAA"),
            GALLERY,
            &[200; 4],
        ));

        check_live_gallery(stub.as_ref(), "Dark").await;

        assert_eq!(stub.calls().len(), 1, "a free check runs before a paid one");
    }

    #[tokio::test]
    async fn cards_the_parser_refuses_read_differently_from_a_window_with_no_cards() {
        // Rename the one attribute the thumbnail lives in and every card is a
        // card with nothing fetchable — cards present, none of them usable.
        let window = WINDOW.replace(r#"<img src="#, r#"<img data-lazy="#);

        let refused = check_live_gallery(&run(&window, GALLERY, &[200; 4]), "Dark").await;
        let empty = check_live_gallery(&run(DRIFTED, GALLERY, &[200; 4]), "Dark").await;

        assert!(refused.is_drift() && empty.is_drift());
        assert!(
            refused.report().contains("fields-usable"),
            "{}",
            refused.report()
        );
        assert!(
            empty.report().contains("cards-present"),
            "{}",
            empty.report()
        );
        assert!(
            refused.report().contains("aishot-5792.jpg"),
            "the refusal names the card it choked on: {}",
            refused.report()
        );
    }

    /// The question the markup cannot answer: a shipped tag the site has
    /// dropped returns an empty fragment, which a user reads as "nothing
    /// found" rather than as a stale menu.
    #[tokio::test]
    async fn a_vocabulary_that_no_longer_matches_is_drift_naming_what_moved_either_way() {
        let moved = GALLERY.replace(
            r#"data-tag="Cyberpunk">Cyberpunk"#,
            r#"data-tag="Solarpunk">Solarpunk"#,
        );
        assert_ne!(moved, GALLERY, "one tag was renamed");

        let check = check_live_gallery(&run(WINDOW, &moved, &[200; 4]), "Dark").await;

        assert!(check.is_drift(), "{}", check.report());
        let report = check.report();
        assert!(report.contains("tag-vocabulary"), "{report}");
        assert!(
            report.contains("Solarpunk") && report.contains("Cyberpunk"),
            "the report names the tag added and the tag removed: {report}"
        );
        assert!(
            report.contains("https://ultrawidewallpapers.net/gallery"),
            "and the page it asked: {report}"
        );
    }

    #[tokio::test]
    async fn a_vocabulary_in_a_different_order_is_not_drift() {
        let mut lines: Vec<&str> = GALLERY.lines().collect();
        lines.reverse();
        let reordered = lines.join("\n");

        let check = check_live_gallery(&run(WINDOW, &reordered, &[200; 4]), "Dark").await;

        assert!(
            !check.is_drift(),
            "order is not the comparison: {}",
            check.report()
        );
    }

    #[tokio::test]
    async fn the_vocabulary_is_asked_only_once_the_cards_are_known_good() {
        let stub = Arc::new(run(DRIFTED, GALLERY, &[200; 4]));

        check_live_gallery(stub.as_ref(), "Dark").await;

        assert_eq!(
            stub.calls().len(),
            1,
            "a broken parse is reported without spending a second page fetch"
        );
    }

    #[tokio::test]
    async fn a_verification_request_that_is_rate_limited_or_fails_is_never_called_drift() {
        for (status, tag) in [(429, "RATE_LIMITED"), (503, "REJECTED")] {
            let check = check_live_gallery(&run(WINDOW, GALLERY, &[status]), "Dark").await;

            assert!(!check.is_drift(), "{status}: {}", check.report());
            assert_eq!(check.tag(), tag);
            assert_eq!(check.exit_code(), 2);
            let report = check.report();
            assert!(report.contains("Transient"), "{report}");
            assert!(
                report.contains("resizecachethumbs.php"),
                "the report names the URL it could not verify: {report}"
            );
        }
    }

    #[tokio::test]
    async fn a_rate_limit_is_its_own_outcome_and_is_never_reported_as_drift() {
        let check = check_live_gallery(&StubFetch::status(429), "Space").await;

        assert_eq!(
            check,
            DriftCheck::RateLimited {
                tag: "Space".to_string(),
                url: gallery_request(&["Space"], 0, 24),
                status: 429,
            }
        );
        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 2);
        assert!(check.report().contains("Transient"), "{}", check.report());
    }

    #[tokio::test]
    async fn a_server_error_is_its_own_outcome_and_is_never_reported_as_drift() {
        let check = check_live_gallery(&StubFetch::status(503), "Space").await;

        assert_eq!(
            check,
            DriftCheck::Rejected {
                tag: "Space".to_string(),
                url: gallery_request(&["Space"], 0, 24),
                status: 503,
            }
        );
        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 2);
    }

    /// The gallery page is a second fetch, and a transport failure on it says
    /// as little about the vocabulary as one on the window says about markup.
    #[tokio::test]
    async fn a_gallery_page_that_cannot_be_fetched_is_transient_not_a_drifted_vocabulary() {
        let stub = StubFetch::new(vec![
            (StatusCode::OK, bytes::Bytes::from(WINDOW.to_string())),
            (StatusCode::SERVICE_UNAVAILABLE, bytes::Bytes::new()),
        ]);

        let check = check_live_gallery(&stub, "Dark").await;

        assert!(!check.is_drift(), "{}", check.report());
        assert_eq!(check.exit_code(), 2);
        assert!(
            check
                .report()
                .contains("https://ultrawidewallpapers.net/gallery"),
            "{}",
            check.report()
        );
    }

    #[tokio::test]
    async fn a_request_that_never_completes_is_its_own_outcome_and_is_never_drift() {
        let check = check_live_gallery(&BrokenFetch, "Dark").await;

        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 2);
        assert_eq!(check.tag(), "UNREACHABLE");
        let report = check.report();
        assert!(report.contains("connection timed out"), "{report}");
        assert!(report.contains("'Dark'"), "{report}");
    }

    #[tokio::test]
    async fn a_sampled_url_the_site_does_not_serve_is_drift_and_names_that_check() {
        let check = check_live_gallery(&run(WINDOW, GALLERY, &[200, 404, 200, 200]), "Dark").await;

        assert!(check.is_drift());
        let report = check.report();
        assert!(report.contains("urls-resolve"), "{report}");
        assert!(report.contains("404"), "{report}");
        assert!(
            report.contains("aishot-5792.jpg"),
            "the offending card is named: {report}"
        );
    }

    #[test]
    fn every_outcome_carries_a_distinct_tag() {
        let tags = [
            DriftCheck::Parsed {
                tag: "t".into(),
                url: "u".into(),
                cards: 1,
                verified: 2,
                vocabulary: 28,
            },
            DriftCheck::Drifted {
                tag: "t".into(),
                url: "u".into(),
                failure: DriftFailure::NoCards {
                    expected: "e".into(),
                    saw: "w".into(),
                },
            },
            DriftCheck::RateLimited {
                tag: "t".into(),
                url: "u".into(),
                status: 429,
            },
            DriftCheck::Rejected {
                tag: "t".into(),
                url: "u".into(),
                status: 500,
            },
            DriftCheck::Unreachable {
                tag: "t".into(),
                url: "u".into(),
                detail: "d".into(),
            },
        ]
        .map(|c| c.tag());
        let mut unique = tags.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), tags.len(), "{tags:?}");
    }

    /// Each named question has its own label, so a report never blurs "no
    /// cards" with "a stale menu".
    #[test]
    fn every_question_carries_a_distinct_label() {
        let checks = [
            DriftFailure::NoCards {
                expected: String::new(),
                saw: String::new(),
            },
            DriftFailure::UnusableField {
                detail: String::new(),
            },
            DriftFailure::ImplausibleCount {
                cards: 0,
                min: 0,
                max: 0,
            },
            DriftFailure::VocabularyDrifted {
                added: Vec::new(),
                removed: Vec::new(),
            },
            DriftFailure::UnresolvableUrl {
                field: "f",
                card: String::new(),
                url: String::new(),
                status: 404,
            },
        ]
        .map(|f| f.check());
        let mut unique = checks.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), checks.len(), "{checks:?}");
    }
}
