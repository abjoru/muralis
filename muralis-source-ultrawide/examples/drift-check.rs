//! Runs the **drift check** against the live site, once.
//!
//! Not part of `cargo test`: this reaches the network. It exists for the
//! weekly scheduled workflow, and for a maintainer who wants to ask the same
//! question by hand:
//!
//! ```text
//! cargo run -p muralis-source-ultrawide --example drift-check -- 32-9-wallpapers
//! ```
//!
//! Exit code is the answer — `0` parsed, `1` drifted, `2` transient — so the
//! workflow can file an issue for drift alone without reading prose.

use muralis_source_common::ReqwestFetch;
use muralis_source_ultrawide::check_live_category;

/// The category checked when none is named: the site's flagship slug, and the
/// one the committed fixture was captured from.
const DEFAULT_CATEGORY: &str = "32-9-wallpapers";

#[tokio::main]
async fn main() {
    let slug = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_CATEGORY.to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("an HTTP client");

    let check = check_live_category(&ReqwestFetch(client), &slug).await;

    println!("{}", check.report());
    std::process::exit(check.exit_code());
}
