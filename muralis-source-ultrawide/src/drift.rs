//! The **drift check**: does the live site still parse?
//!
//! The **Ultrawide Source** parses markup it does not own. Its parser is
//! specified by two fixtures captured on one day, so the offline suite goes on
//! passing however far the site moves — confidence grows exactly as accuracy
//! decays. This module closes that gap by running the *production* parser
//! ([`parse_category`]) over a freshly fetched **Category page**, and saying
//! which of four things happened.
//!
//! It is not part of the suite: it reaches the network, so it runs only from a
//! scheduled workflow (`.github/workflows/ultrawide-drift.yml`), never on push
//! and never under `cargo test`. Its volume obeys the **Access posture** — one
//! page fetch plus a bounded handful of one-byte verification requests, all
//! carrying the **muralis User-Agent**.
//!
//! "Did anything parse?" is not the question. The Source shipped reading `src`
//! on a page that lazy-loads, and three quarters of every category's Previews
//! carried a 1x1 placeholder: a full complement of cards, every one unusable.
//! So the check asks whether what parsed is *usable* — fields that hold
//! locations rather than values, a plausible number of them, and a sample the
//! site will actually serve — and names which of those questions failed.
//!
//! A transport failure is *not* drift. A timeout, a 5xx and a rate limit are
//! transient and say nothing about the markup.

use reqwest::StatusCode;
use scraper::{Html, Selector};
use std::sync::LazyLock;

use muralis_core::models::WallpaperPreview;
use muralis_source_common::HttpFetch;

use super::{category_url, parse_category, user_agent, CARD_SEL};

static TITLE_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("title").expect("valid selector"));
static ANCHOR_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a").expect("valid selector"));

/// What the parser expects of a **Category page**, phrased for a maintainer
/// reading a failed run rather than for a stack trace. Only the
/// no-cards-at-all report needs it: every other failure has an offending value
/// of its own to show.
const EXPECTED: &str = "at least one card matching `a[data-filename][href]` wrapping an `img` \
     whose `src` or `data-src` holds a fetchable thumbnail URL";

/// The outcome of one drift check. Distinct answers, because conflating them
/// is the failure mode this exists to prevent: only [`Drifted`] means the
/// parser is broken.
///
/// [`Drifted`]: DriftCheck::Drifted
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftCheck {
    /// The page parsed into usable **Previews**. Quiet success — nothing to
    /// file, nothing to say.
    Parsed {
        slug: String,
        url: String,
        cards: usize,
        /// How many URLs were fetched to confirm the site still serves them.
        verified: usize,
    },
    /// The page arrived intact and what the production parser made of it is
    /// not usable. This, and only this, is drift.
    Drifted {
        slug: String,
        url: String,
        /// Which usability check tripped, and on what value.
        failure: DriftFailure,
    },
    /// The site asked us to slow down. Transient; nothing was learned.
    RateLimited {
        slug: String,
        url: String,
        status: u16,
    },
    /// A non-success status that is not a rate limit — a 5xx, or a redirect
    /// landing on an error page. Transient; nothing was learned.
    Rejected {
        slug: String,
        url: String,
        status: u16,
    },
    /// The request never completed: DNS, TLS, connection, timeout.
    Unreachable {
        slug: String,
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
    /// Nothing on the page matched the card selector.
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
                 a page yielding a handful where it used to yield scores is a parser \
                 reading something that is no longer the card list"
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

impl DriftCheck {
    /// True only for a page whose parse yielded nothing usable. Everything
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

    /// The diagnosable report. Always names the category and the URL being
    /// fetched, so a failed run is readable without opening the code.
    pub fn report(&self) -> String {
        match self {
            DriftCheck::Parsed {
                slug,
                url,
                cards,
                verified,
            } => format!(
                "PARSED category '{slug}' at {url}: {cards} cards, \
                 {verified} sampled URLs served"
            ),
            DriftCheck::Drifted { slug, url, failure } => format!(
                "DRIFTED category '{slug}' at {url}: check '{check}' failed — {detail}. \
                 The site's card markup has most likely changed; recapture the fixture \
                 by hand — a new capture is a new parser contract.",
                check = failure.check(),
                detail = failure.detail(),
            ),
            DriftCheck::RateLimited { slug, url, status } => format!(
                "RATE_LIMITED category '{slug}' while fetching {url}: HTTP {status}. \
                 Transient — the site asked us to slow down, so this says nothing \
                 about the markup."
            ),
            DriftCheck::Rejected { slug, url, status } => format!(
                "REJECTED category '{slug}' while fetching {url}: HTTP {status}. \
                 Transient — nothing arrived to read, so this says nothing about \
                 the markup."
            ),
            DriftCheck::Unreachable { slug, url, detail } => format!(
                "UNREACHABLE category '{slug}' while fetching {url}: {detail}. \
                 Transient — the request never completed, so this says nothing \
                 about the markup."
            ),
        }
    }
}

/// What the check considers plausible, and how much it is willing to spend
/// finding out. One category page plus `2 * sample` verification requests is
/// the whole weekly budget, and the **Access posture** is why that is a
/// constant rather than a crawl.
pub struct DriftLimits {
    /// Fewest distinct **Cards** a live **Category page** may yield and still
    /// be believed. The site lays its cards out twelve to a carousel page, so
    /// a category yielding less than one full carousel page is a parser
    /// reading something that is no longer the card list — the 32:9 page
    /// published 106 distinct cards when this was written.
    pub min_cards: usize,
    /// Most it may yield. Not a tight bound and not a catalogue size: its job
    /// is to catch a selector that has started matching something which is
    /// not a card at all.
    pub max_cards: usize,
    /// How many **Previews** get both of their URLs fetched.
    pub sample: usize,
}

impl Default for DriftLimits {
    fn default() -> Self {
        Self {
            min_cards: 12,
            max_cards: 2_000,
            sample: 2,
        }
    }
}

/// Verification asks only whether the site serves the URL, so it asks for one
/// byte rather than a 7680x2160 master. A server that ignores `Range` answers
/// `200` instead of `206`; both are success and both answer the question.
const ONE_BYTE: &str = "bytes=0-0";

/// Fetch one **Category page**, run the production parser over it, and ask
/// whether what came out is usable.
///
/// One page fetch plus a bounded handful of verification requests, whatever
/// the outcome: there is no retry, because a retry would double the run's
/// traffic to distinguish nothing a maintainer cannot distinguish a week later.
pub async fn check_live_category(http: &dyn HttpFetch, slug: &str) -> DriftCheck {
    check_live_category_with(http, slug, &DriftLimits::default()).await
}

/// [`check_live_category`] with the budget spelled out.
pub async fn check_live_category_with(
    http: &dyn HttpFetch,
    slug: &str,
    limits: &DriftLimits,
) -> DriftCheck {
    let url = category_url(slug);
    let slug = slug.to_string();

    let (status, body) = match http.get(&url, &[("User-Agent", user_agent())], &[]).await {
        Ok(response) => response,
        Err(e) => {
            return DriftCheck::Unreachable {
                slug,
                url,
                detail: e.to_string(),
            }
        }
    };
    if status == StatusCode::TOO_MANY_REQUESTS {
        return DriftCheck::RateLimited {
            slug,
            url,
            status: status.as_u16(),
        };
    }
    // Any non-success for the page itself is transient: the page was never
    // seen, so there is nothing to conclude about the markup.
    if !status.is_success() {
        return DriftCheck::Rejected {
            slug,
            url,
            status: status.as_u16(),
        };
    }

    let html = String::from_utf8_lossy(&body);
    let previews = match parse_category(&html, &slug, &url) {
        Ok(previews) if !previews.is_empty() => previews,
        refused => {
            // Which usability check tripped is decided by whether the page had
            // cards at all, counted with the parser's own selector rather than
            // by a second parser.
            let failure = if count_cards(&html) == 0 {
                DriftFailure::NoCards {
                    expected: EXPECTED.to_string(),
                    saw: describe(&body),
                }
            } else {
                DriftFailure::UnusableField {
                    detail: match refused {
                        Err(e) => format!("the parser refused the page: {e}"),
                        Ok(_) => "the parser returned no card and no error".to_string(),
                    },
                }
            };
            return DriftCheck::Drifted { slug, url, failure };
        }
    };

    // Every card, not a sample: an unusable field costs nothing to spot and
    // means the same thing wherever on the page it turns up. This is the
    // check the Source lacked when it shipped reading `src` on a page that
    // lazy-loads — a full complement of Previews, every one unusable.
    for preview in &previews {
        if let Some(failure) = unusable_field(preview, &url) {
            return DriftCheck::Drifted { slug, url, failure };
        }
    }

    // Decided from the page already in hand, before a single verification
    // request is spent: a page that yields a handful where it used to yield
    // scores is drift whatever those few cards resolve to.
    let cards = previews.len();
    if cards < limits.min_cards || cards > limits.max_cards {
        return DriftCheck::Drifted {
            slug,
            url,
            failure: DriftFailure::ImplausibleCount {
                cards,
                min: limits.min_cards,
                max: limits.max_cards,
            },
        };
    }

    // A sample, not every card: this is the part that costs requests.
    let mut verified = 0;
    for (field, card, target) in sample_urls(&previews, limits.sample) {
        match verify(http, &slug, field, card, target).await {
            Some(outcome) => return outcome,
            None => verified += 1,
        }
    }

    DriftCheck::Parsed {
        slug,
        url,
        cards,
        verified,
    }
}

/// One verification request. `None` means the site served it; `Some` is the
/// outcome to report and stop on — drift for a URL the site does not have,
/// transient for a rate limit, a 5xx or a request that never completed.
async fn verify(
    http: &dyn HttpFetch,
    slug: &str,
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
                slug: slug.to_string(),
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
            slug: slug.to_string(),
            url: target.to_string(),
            status: status.as_u16(),
        });
    }
    if status.is_server_error() {
        return Some(DriftCheck::Rejected {
            slug: slug.to_string(),
            url: target.to_string(),
            status: status.as_u16(),
        });
    }
    // A 404 or a 403 on a URL the parser just extracted is not a transport
    // story: the parser built a URL the site does not have.
    Some(DriftCheck::Drifted {
        slug: slug.to_string(),
        url: category_url(slug),
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
fn unusable_field(preview: &WallpaperPreview, page_url: &str) -> Option<DriftFailure> {
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
        } else if trimmed == page_url {
            // An empty `href` resolves to the page it was found on, which is a
            // location — just not of an image.
            "the category page itself rather than an image"
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

/// Both URLs of each sampled **Preview**, in page order.
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

/// Evenly spread indices, so a sample of two is the first card and the last.
/// That is deliberate rather than incidental: a category page opens with its
/// eager cards and closes with its lazy ones, so both **Card** forms are
/// covered by the smallest sample there is.
fn spread(len: usize, sample: usize) -> Vec<usize> {
    let sample = sample.min(len);
    match sample {
        0 => Vec::new(),
        1 => vec![0],
        n => (0..n).map(|k| k * (len - 1) / (n - 1)).collect(),
    }
}

/// How many cards the page has, by the parser's own selector.
fn count_cards(html: &str) -> usize {
    Html::parse_document(html).select(&CARD_SEL).count()
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

    const CATEGORY_PAGE: &str = include_str!("../tests/fixtures/category-32-9.html");
    const DRIFTED_PAGE: &str = include_str!("../tests/fixtures/category-drifted.html");

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
    async fn a_page_whose_cards_are_usable_and_served_is_a_quiet_success() {
        let check =
            check_live_category(&page_then(CATEGORY_PAGE, &[200; 4]), "32-9-wallpapers").await;

        assert_eq!(
            check,
            DriftCheck::Parsed {
                slug: "32-9-wallpapers".to_string(),
                url: "https://www.ultrawidewallpapers.net/32-9-wallpapers".to_string(),
                cards: 23,
                verified: 4,
            }
        );
        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 0, "quiet: nothing filed, nothing said");
    }

    #[tokio::test]
    async fn the_run_costs_one_page_fetch_plus_a_bounded_sample_all_identifying_muralis() {
        let limits = DriftLimits {
            sample: 2,
            ..DriftLimits::default()
        };
        let stub = Arc::new(page_then(CATEGORY_PAGE, &[200, 200, 200, 200]));

        check_live_category_with(stub.as_ref(), "32-9-wallpapers", &limits).await;

        let calls = stub.calls();
        assert_eq!(
            calls.len(),
            1 + 2 * limits.sample,
            "one page fetch, then both URLs of each sampled card — and no retry"
        );
        assert_eq!(
            calls[0].url,
            "https://www.ultrawidewallpapers.net/32-9-wallpapers"
        );
        for call in &calls {
            let ua = call
                .header_value("User-Agent")
                .expect("every request identifies itself exactly as the Source does");
            assert!(ua.starts_with("muralis/"), "{ua}");
        }
        for call in &calls[1..] {
            assert_eq!(
                call.header_value("Range"),
                Some("bytes=0-0"),
                "verification asks whether the site serves the URL, not for the master"
            );
        }
    }

    #[tokio::test]
    async fn the_sample_covers_both_card_forms_the_page_publishes() {
        let stub = Arc::new(page_then(CATEGORY_PAGE, &[200, 200, 200, 200]));

        check_live_category(stub.as_ref(), "32-9-wallpapers").await;

        let sampled: Vec<String> = stub.calls()[1..].iter().map(|c| c.url.clone()).collect();
        assert!(
            sampled.iter().any(|u| u.contains("aishot-5774")),
            "the first card, eager: {sampled:?}"
        );
        assert!(
            sampled.iter().any(|u| u.contains("aishot-5759")),
            "the last card, lazy — the form that shipped broken: {sampled:?}"
        );
    }

    #[tokio::test]
    async fn a_page_that_parses_to_no_cards_is_drift_and_says_what_it_saw() {
        let check = check_live_category(&StubFetch::ok(DRIFTED_PAGE), "32-9-wallpapers").await;

        assert!(check.is_drift());
        assert_eq!(check.exit_code(), 1);
        let report = check.report();
        assert!(report.contains("32-9-wallpapers"), "{report}");
        assert!(
            report.contains("https://www.ultrawidewallpapers.net/32-9-wallpapers"),
            "the failure names the URL it fetched: {report}"
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

    #[tokio::test]
    async fn a_card_count_outside_the_plausible_band_is_drift_even_though_cards_parsed() {
        let thinned = DriftLimits {
            min_cards: 50,
            ..DriftLimits::default()
        };

        let check = check_live_category_with(
            &page_then(CATEGORY_PAGE, &[200; 4]),
            "32-9-wallpapers",
            &thinned,
        )
        .await;

        assert!(check.is_drift(), "{}", check.report());
        let report = check.report();
        assert!(report.contains("card-count"), "{report}");
        assert!(
            report.contains("23 cards") && report.contains("50"),
            "the report shows what it counted and the band it wanted: {report}"
        );
    }

    #[tokio::test]
    async fn the_default_band_admits_the_capture_and_costs_no_request_to_decide() {
        let stub = Arc::new(page_then(CATEGORY_PAGE, &[200; 4]));

        let check = check_live_category(stub.as_ref(), "32-9-wallpapers").await;

        assert_eq!(check.tag(), "PARSED", "{}", check.report());

        let thinned = DriftLimits {
            min_cards: 50,
            ..DriftLimits::default()
        };
        let stub = Arc::new(page_then(CATEGORY_PAGE, &[200; 4]));
        check_live_category_with(stub.as_ref(), "32-9-wallpapers", &thinned).await;
        assert_eq!(
            stub.calls().len(),
            1,
            "the band is decided from the page already fetched, before any \
             verification request is spent"
        );
    }

    /// The capture, with one card's master link rewritten. Not a new fixture:
    /// the committed capture stays the parser's only input, and this is the
    /// same edit the drift copy makes — one attribute, by hand.
    fn with_first_master(href: &str) -> String {
        CATEGORY_PAGE.replacen(
            r#"href="wallpapers/329/highres/aishot-5774.jpg""#,
            &format!(r#"href="{href}""#),
            1,
        )
    }

    #[tokio::test]
    async fn a_master_url_that_is_a_placeholder_rather_than_a_location_is_drift() {
        let page = with_first_master(
            "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
        );

        let check = check_live_category(&page_then(&page, &[200; 4]), "32-9-wallpapers").await;

        assert!(check.is_drift(), "{}", check.report());
        let report = check.report();
        assert!(report.contains("fields-usable"), "{report}");
        assert!(report.contains("master URL"), "{report}");
        assert!(
            report.contains("aishot-5774.jpg") && report.contains("data:image/gif"),
            "the report shows the offending card and its value: {report}"
        );
    }

    #[tokio::test]
    async fn an_empty_master_link_resolving_to_the_category_page_is_drift_not_a_preview() {
        let check = check_live_category(
            &page_then(&with_first_master(""), &[200; 4]),
            "32-9-wallpapers",
        )
        .await;

        assert!(check.is_drift(), "{}", check.report());
        assert!(
            check.report().contains("fields-usable"),
            "{}",
            check.report()
        );
    }

    #[tokio::test]
    async fn an_unusable_field_is_found_before_a_single_verification_request_is_spent() {
        let stub = Arc::new(page_then(
            &with_first_master("data:image/gif;base64,AAAA"),
            &[200; 4],
        ));

        check_live_category(stub.as_ref(), "32-9-wallpapers").await;

        assert_eq!(stub.calls().len(), 1, "a free check runs before a paid one");
    }

    #[tokio::test]
    async fn cards_the_parser_refuses_read_differently_from_a_page_with_no_cards() {
        // The capture's lazy cards keep their real URL in `data-src`; rename
        // that one attribute and every one of them is a card with nothing but
        // a placeholder — cards present, none of them usable.
        let page = CATEGORY_PAGE.replace("data-src=", "data-lazy=");

        let refused = check_live_category(&page_then(&page, &[200; 4]), "32-9-wallpapers").await;
        let empty =
            check_live_category(&page_then(DRIFTED_PAGE, &[200; 4]), "32-9-wallpapers").await;

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
            refused.report().contains("aishot-5771.jpg"),
            "the refusal names the card it choked on: {}",
            refused.report()
        );
    }

    #[tokio::test]
    async fn a_verification_request_that_is_rate_limited_or_fails_is_never_called_drift() {
        for (status, tag) in [(429, "RATE_LIMITED"), (503, "REJECTED")] {
            let check =
                check_live_category(&page_then(CATEGORY_PAGE, &[status]), "32-9-wallpapers").await;

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
        let check = check_live_category(&StubFetch::status(429), "space-wallpapers").await;

        assert_eq!(
            check,
            DriftCheck::RateLimited {
                slug: "space-wallpapers".to_string(),
                url: "https://www.ultrawidewallpapers.net/space-wallpapers".to_string(),
                status: 429,
            }
        );
        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 2);
        assert!(check.report().contains("Transient"), "{}", check.report());
    }

    #[tokio::test]
    async fn a_server_error_is_its_own_outcome_and_is_never_reported_as_drift() {
        let check = check_live_category(&StubFetch::status(503), "space-wallpapers").await;

        assert_eq!(
            check,
            DriftCheck::Rejected {
                slug: "space-wallpapers".to_string(),
                url: "https://www.ultrawidewallpapers.net/space-wallpapers".to_string(),
                status: 503,
            }
        );
        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 2);
    }

    #[tokio::test]
    async fn a_request_that_never_completes_is_its_own_outcome_and_is_never_drift() {
        let check = check_live_category(&BrokenFetch, "32-9-wallpapers").await;

        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 2);
        assert_eq!(check.tag(), "UNREACHABLE");
        let report = check.report();
        assert!(report.contains("connection timed out"), "{report}");
        assert!(report.contains("32-9-wallpapers"), "{report}");
    }

    /// The category page, then one canned response per verification request.
    fn page_then(page: &str, verifications: &[u16]) -> StubFetch {
        let mut responses = vec![(StatusCode::OK, bytes::Bytes::from(page.to_string()))];
        responses.extend(verifications.iter().map(|c| {
            (
                StatusCode::from_u16(*c).expect("a status"),
                bytes::Bytes::new(),
            )
        }));
        StubFetch::new(responses)
    }

    #[tokio::test]
    async fn a_sampled_url_the_site_does_not_serve_is_drift_and_names_that_check() {
        let check = check_live_category(
            &page_then(CATEGORY_PAGE, &[200, 404, 200, 200]),
            "32-9-wallpapers",
        )
        .await;

        assert!(check.is_drift());
        let report = check.report();
        assert!(report.contains("urls-resolve"), "{report}");
        assert!(report.contains("404"), "{report}");
        assert!(
            report.contains("aishot-5774.jpg"),
            "the offending card is named: {report}"
        );
    }

    #[test]
    fn every_outcome_carries_a_distinct_tag() {
        let tags = [
            DriftCheck::Parsed {
                slug: "s".into(),
                url: "u".into(),
                cards: 1,
                verified: 2,
            },
            DriftCheck::Drifted {
                slug: "s".into(),
                url: "u".into(),
                failure: DriftFailure::NoCards {
                    expected: "e".into(),
                    saw: "w".into(),
                },
            },
            DriftCheck::RateLimited {
                slug: "s".into(),
                url: "u".into(),
                status: 429,
            },
            DriftCheck::Rejected {
                slug: "s".into(),
                url: "u".into(),
                status: 500,
            },
            DriftCheck::Unreachable {
                slug: "s".into(),
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
}
