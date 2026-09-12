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
//! and never under `cargo test`. Its volume obeys the **Access posture** —
//! exactly one page fetch, carrying the **muralis User-Agent**.
//!
//! A transport failure is *not* drift. A timeout, a 5xx and a rate limit are
//! transient and say nothing about the markup; only a page that arrived intact
//! and yielded no **Card** means the parser is broken.

use reqwest::StatusCode;
use scraper::{Html, Selector};
use std::sync::LazyLock;

use muralis_source_common::HttpFetch;

use super::{category_url, parse_category, user_agent, CARD_SEL};

static TITLE_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("title").expect("valid selector"));
static ANCHOR_SEL: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a").expect("valid selector"));

/// What the parser expects of a **Category page**, phrased for a maintainer
/// reading a failed run rather than for a stack trace.
const EXPECTED: &str = "at least one card matching `a[data-filename][href]` wrapping an `img[src]`";

/// The outcome of one drift check. Four distinct answers, because conflating
/// them is the failure mode this exists to prevent: only [`Drifted`] means the
/// parser is broken.
///
/// [`Drifted`]: DriftCheck::Drifted
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftCheck {
    /// The page parsed. Quiet success — nothing to file, nothing to say.
    Parsed {
        slug: String,
        url: String,
        cards: usize,
    },
    /// The page arrived intact and the production parser found no **Card**.
    /// This, and only this, is drift.
    Drifted {
        slug: String,
        url: String,
        /// What the parser looks for.
        expected: String,
        /// Enough of what actually arrived to tell a markup change apart from
        /// an error page: size, `<title>`, and how many anchors were present.
        saw: String,
    },
    /// The site asked us to slow down. Transient; the page was never seen.
    RateLimited {
        slug: String,
        url: String,
        status: u16,
    },
    /// A non-success status that is not a rate limit — a 5xx, or a redirect
    /// landing on an error page. Transient; the page was never seen.
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

impl DriftCheck {
    /// True only for a page that parsed to zero cards. Everything else is a
    /// transport story, and filing it as drift would cry wolf.
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

    /// The diagnosable report. Always names the category and its URL, so a
    /// failed run is readable without opening the code.
    pub fn report(&self) -> String {
        match self {
            DriftCheck::Parsed { slug, url, cards } => {
                format!("PARSED category '{slug}' at {url}: {cards} cards")
            }
            DriftCheck::Drifted {
                slug,
                url,
                expected,
                saw,
            } => format!(
                "DRIFTED category '{slug}' at {url}: the parser expected {expected}, \
                 and saw {saw}. The site's card markup has most likely changed; \
                 recapture the fixture by hand — a new capture is a new parser contract."
            ),
            DriftCheck::RateLimited { slug, url, status } => format!(
                "RATE_LIMITED category '{slug}' at {url}: HTTP {status}. Transient — \
                 the page was never seen, so this says nothing about the markup."
            ),
            DriftCheck::Rejected { slug, url, status } => format!(
                "REJECTED category '{slug}' at {url}: HTTP {status}. Transient — \
                 the page was never seen, so this says nothing about the markup."
            ),
            DriftCheck::Unreachable { slug, url, detail } => format!(
                "UNREACHABLE category '{slug}' at {url}: {detail}. Transient — \
                 the request never completed, so this says nothing about the markup."
            ),
        }
    }
}

/// Fetch one **Category page** and run the production parser over it.
///
/// Exactly one request, whatever the outcome: there is no retry, because a
/// retry would double the run's traffic to distinguish nothing a maintainer
/// cannot distinguish a week later.
pub async fn check_live_category(http: &dyn HttpFetch, slug: &str) -> DriftCheck {
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
    if !status.is_success() {
        return DriftCheck::Rejected {
            slug,
            url,
            status: status.as_u16(),
        };
    }

    let html = String::from_utf8_lossy(&body);
    match parse_category(&html, &slug, &url) {
        Ok(previews) if !previews.is_empty() => DriftCheck::Parsed {
            slug,
            url,
            cards: previews.len(),
        },
        _ => DriftCheck::Drifted {
            slug,
            url,
            expected: EXPECTED.to_string(),
            saw: describe(&body),
        },
    }
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
    async fn a_page_that_still_parses_is_a_quiet_success() {
        let check = check_live_category(&StubFetch::ok(CATEGORY_PAGE), "32-9-wallpapers").await;

        assert_eq!(
            check,
            DriftCheck::Parsed {
                slug: "32-9-wallpapers".to_string(),
                url: "https://www.ultrawidewallpapers.net/32-9-wallpapers".to_string(),
                cards: 23,
            }
        );
        assert!(!check.is_drift());
        assert_eq!(check.exit_code(), 0);
    }

    #[tokio::test]
    async fn the_check_costs_exactly_one_fetch_and_identifies_muralis() {
        let stub = Arc::new(StubFetch::ok(CATEGORY_PAGE));

        check_live_category(stub.as_ref(), "32-9-wallpapers").await;

        let calls = stub.calls();
        assert_eq!(calls.len(), 1, "one page fetch per run, no retry");
        assert_eq!(
            calls[0].url,
            "https://www.ultrawidewallpapers.net/32-9-wallpapers"
        );
        let ua = calls[0]
            .header_value("User-Agent")
            .expect("the check identifies itself exactly as the Source does");
        assert!(ua.starts_with("muralis/"), "{ua}");
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
            report.contains("a[data-filename][href]"),
            "and what the parser expected: {report}"
        );
        assert!(
            report.contains("bytes") && report.contains("<a> elements"),
            "and enough of what arrived to tell markup drift from an error page: {report}"
        );
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

    #[test]
    fn every_outcome_carries_a_distinct_tag() {
        let tags = [
            DriftCheck::Parsed {
                slug: "s".into(),
                url: "u".into(),
                cards: 1,
            },
            DriftCheck::Drifted {
                slug: "s".into(),
                url: "u".into(),
                expected: "e".into(),
                saw: "w".into(),
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
