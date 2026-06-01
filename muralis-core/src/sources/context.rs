//! Global cross-cutting context handed to every source plugin's
//! `create_sources`. See ADR 0002 (content-safety ceiling) and ADR 0003
//! (SourceContext plugin contract).

use serde::{Deserialize, Serialize};

/// Global content-safety ceiling — an ordered level acting as a cap, not an
/// override (ADR 0002). Effective safety per source is the *stricter* of this
/// global ceiling and the source's native control, so a source's native knob
/// (wallhaven `purity`, booru `rating:`) can only tighten below the ceiling,
/// never loosen past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContentSafety {
    /// SFW only — the default and the panic switch.
    #[default]
    Safe,
    /// Suggestive/sensitive content allowed, no explicit.
    Moderate,
    /// Explicit content allowed, up to each source's native cap.
    Nsfw,
}

/// Global cross-cutting knobs passed to every plugin's `create_sources`
/// (ADR 0003). Designed to absorb future global settings without re-churning
/// the plugin contract. Plugins see this plus their own `[sources]` table,
/// never the whole `Config`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SourceContext {
    pub content_safety: ContentSafety,
    pub min_width: u32,
    pub min_height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_safety_is_ordered_safe_lt_moderate_lt_nsfw() {
        assert!(ContentSafety::Safe < ContentSafety::Moderate);
        assert!(ContentSafety::Moderate < ContentSafety::Nsfw);
        assert!(ContentSafety::Safe < ContentSafety::Nsfw);
    }

    #[test]
    fn content_safety_default_is_safe() {
        assert_eq!(ContentSafety::default(), ContentSafety::Safe);
    }

    #[test]
    fn content_safety_deserializes_lowercase() {
        let v: ContentSafety = toml::from_str("v = \"moderate\"")
            .map(|t: toml::Table| t["v"].clone().try_into().unwrap())
            .unwrap();
        assert_eq!(v, ContentSafety::Moderate);
    }
}
