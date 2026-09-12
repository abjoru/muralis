//! Runs the **drift check** against the live site, once.
//!
//! Not part of `cargo test`: this reaches the network. It exists for the
//! weekly scheduled workflow, and for a maintainer who wants to ask the same
//! question by hand:
//!
//! ```text
//! cargo run -p muralis-source-ultrawide --example drift-check -- Dark
//! ```
//!
//! Exit code is the answer — `0` parsed, `1` drifted, `2` transient — so the
//! workflow can file an issue for drift alone without reading prose. The
//! report names which question failed: `cards-present`, `fields-usable`,
//! `card-count`, `tag-vocabulary`, `urls-resolve` or `urls-refused`.
//!
//! One window of the gallery endpoint, one gallery page, plus two one-byte
//! verification requests per sampled card. Nothing here is a crawl.

use muralis_source_common::ReqwestFetch;
use muralis_source_ultrawide::check_live_gallery;

/// The tag checked when none is named. A populous one on purpose: the card
/// count is part of the answer, so a tag with a handful of images would make
/// the plausible band say nothing.
const DEFAULT_TAG: &str = "Dark";

#[tokio::main]
async fn main() {
    let tag = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_TAG.to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("an HTTP client");

    let check = check_live_gallery(&ReqwestFetch(client), &tag).await;

    println!("{}", check.report());
    std::process::exit(check.exit_code());
}
