//! The identification muralis presents to every host it fetches from.
//!
//! One definition, rendered from `assets/user-agent.tmpl`, shared by the CLI's
//! HTTP client, every **Source** that sets the header itself, and the GUI —
//! which reads the same template at build time (see `muralis-gui/src/
//! useragent.cpp`). A second literal would drift, and a request that drifts
//! out of the agreed identification is a request an operator cannot attribute.

use std::sync::LazyLock;

static USER_AGENT: LazyLock<String> = LazyLock::new(|| {
    include_str!("../../assets/user-agent.tmpl")
        .trim()
        .replace("{version}", env!("CARGO_PKG_VERSION"))
});

/// The `User-Agent` every outbound muralis request carries.
pub fn user_agent() -> &'static str {
    &USER_AGENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identification_names_this_build() {
        let ua = user_agent();
        assert!(ua.contains(env!("CARGO_PKG_VERSION")), "{ua}");
        assert!(!ua.contains('{'), "every placeholder is substituted: {ua}");
    }
}
