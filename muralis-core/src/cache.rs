use std::path::{Path, PathBuf};

use tracing::info;

use crate::error::Result;
use crate::paths::MuralisPaths;

/// The two populations sharing the thumbnails cache directory, told apart by
/// the `_thumb` suffix the **Library** thumbnail carries and the **Preview**
/// thumbnail deliberately omits. Eviction correctness rests on this, so it is
/// named rather than left to a string comparison at the call site.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ThumbnailKind {
    /// Written by the GUI while browsing, keyed by a digest of the remote URL.
    /// Disposable by construction: anything lost is refetched on next view.
    Preview,
    /// Written when a **Wallpaper** is kept, `<id>_thumb.jpg`, one per Library
    /// row. Lives as long as the wallpaper does.
    Library,
}

const LIBRARY_SUFFIX: &str = "_thumb";

/// Which population a cached thumbnail belongs to, from its filename alone.
pub fn thumbnail_kind(path: &Path) -> ThumbnailKind {
    match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) if stem.ends_with(LIBRARY_SUFFIX) => ThumbnailKind::Library,
        _ => ThumbnailKind::Preview,
    }
}

pub struct CacheStats {
    /// Preview thumbnails: shed first, and clearable outright.
    pub disposable_size: u64,
    pub disposable_count: usize,
    /// Library thumbnails: one per kept **Wallpaper**.
    pub library_size: u64,
    pub library_count: usize,
    pub total_size: u64,
    pub total_count: usize,
}

/// Scan the thumbnails cache and return sizes per population.
pub fn cache_stats(paths: &MuralisPaths) -> CacheStats {
    let mut stats = CacheStats {
        disposable_size: 0,
        disposable_count: 0,
        library_size: 0,
        library_count: 0,
        total_size: 0,
        total_count: 0,
    };
    for entry in cached_thumbnails(&paths.thumbnails_dir()) {
        match entry.kind {
            ThumbnailKind::Preview => {
                stats.disposable_size += entry.size;
                stats.disposable_count += 1;
            }
            ThumbnailKind::Library => {
                stats.library_size += entry.size;
                stats.library_count += 1;
            }
        }
        stats.total_size += entry.size;
        stats.total_count += 1;
    }
    stats
}

/// Prune the thumbnails cache to stay under `max_bytes`. Sheds every preview
/// thumbnail before touching a Library one — the Library's are precisely the
/// old ones, so age alone inverts the order with respect to the value of the
/// data. Within a population, oldest first. Never touches wallpaper files.
pub fn prune_cache(paths: &MuralisPaths, max_bytes: u64) -> Result<u64> {
    let mut entries = cached_thumbnails(&paths.thumbnails_dir());
    let total: u64 = entries.iter().map(|e| e.size).sum();
    if total <= max_bytes {
        return Ok(0);
    }
    let target = total - max_bytes;

    entries.sort_by_key(|e| (e.kind, e.modified));

    let mut freed = 0u64;
    for entry in entries {
        if freed >= target {
            break;
        }
        if std::fs::remove_file(&entry.path).is_ok() {
            freed += entry.size;
        }
    }

    info!(freed_bytes = freed, "cache pruned");
    Ok(freed)
}

/// Drop every preview thumbnail, whatever the cache's size — the disposable
/// population exists to stop a grid refetching, and a user who wants the
/// browsing residue gone should not have to reach the size ceiling first.
/// Library thumbnails are left untouched, as are wallpaper files.
pub fn clear_disposable(paths: &MuralisPaths) -> Result<u64> {
    let mut freed = 0u64;
    for entry in cached_thumbnails(&paths.thumbnails_dir()) {
        if entry.kind != ThumbnailKind::Preview {
            continue;
        }
        if std::fs::remove_file(&entry.path).is_ok() {
            freed += entry.size;
        }
    }
    info!(freed_bytes = freed, "disposable cache cleared");
    Ok(freed)
}

struct CachedThumbnail {
    path: PathBuf,
    size: u64,
    modified: std::time::SystemTime,
    kind: ThumbnailKind,
}

fn cached_thumbnails(dir: &Path) -> Vec<CachedThumbnail> {
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            if let Ok(meta) = entry.metadata() {
                if !meta.is_file() {
                    continue;
                }
                let path = entry.path();
                entries.push(CachedThumbnail {
                    size: meta.len(),
                    modified: meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    kind: thumbnail_kind(&path),
                    path,
                });
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn paths_in(tmp: &tempfile::TempDir) -> MuralisPaths {
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        paths.ensure_dirs().unwrap();
        paths
    }

    /// Write a thumbnail of `size` bytes, modified `age_secs` ago.
    fn write_thumb(paths: &MuralisPaths, name: &str, size: usize, age_secs: u64) {
        let path = paths.thumbnails_dir().join(name);
        std::fs::write(&path, vec![0u8; size]).unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_secs);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(when))
            .unwrap();
    }

    #[test]
    fn eviction_sheds_every_preview_thumbnail_before_any_library_one() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(&tmp);

        // Library thumbnails are the old ones: kept months ago.
        write_thumb(&paths, "aaa_thumb.jpg", 1000, 86_400 * 90);
        write_thumb(&paths, "bbb_thumb.jpg", 1000, 86_400 * 60);
        // Preview thumbnails are always recent: written while browsing.
        write_thumb(&paths, "deadbeef.jpg", 1000, 10);
        write_thumb(&paths, "cafebabe.jpg", 1000, 5);

        // 4000 bytes on disk, ceiling of 2500: 1500 must go.
        let freed = prune_cache(&paths, 2500).unwrap();
        assert_eq!(freed, 2000, "sheds whole files until under the ceiling");

        let stats = cache_stats(&paths);
        assert_eq!(stats.disposable_count, 0, "every preview thumbnail shed");
        assert_eq!(stats.library_count, 2, "no Library thumbnail touched");
    }

    #[test]
    fn a_library_only_cache_over_its_ceiling_still_sheds_oldest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(&tmp);

        write_thumb(&paths, "old_thumb.jpg", 1000, 86_400 * 90);
        write_thumb(&paths, "mid_thumb.jpg", 1000, 86_400 * 30);
        write_thumb(&paths, "new_thumb.jpg", 1000, 60);

        let freed = prune_cache(&paths, 2000).unwrap();
        assert_eq!(freed, 1000);

        assert!(!paths.thumbnails_dir().join("old_thumb.jpg").exists());
        assert!(paths.thumbnails_dir().join("mid_thumb.jpg").exists());
        assert!(paths.thumbnails_dir().join("new_thumb.jpg").exists());
    }

    #[test]
    fn clearing_drops_every_preview_thumbnail_and_no_library_one() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(&tmp);

        write_thumb(&paths, "aaa_thumb.jpg", 1000, 86_400);
        write_thumb(&paths, "deadbeef.jpg", 500, 10);
        write_thumb(&paths, "cafebabe.png", 500, 5);

        // Far below any ceiling: clearing is not gated on one.
        let freed = clear_disposable(&paths).unwrap();
        assert_eq!(freed, 1000);

        let stats = cache_stats(&paths);
        assert_eq!(stats.disposable_count, 0);
        assert_eq!(stats.library_count, 1);
        assert!(paths.thumbnails_dir().join("aaa_thumb.jpg").exists());
    }

    #[test]
    fn stats_split_the_two_populations_and_sum_to_the_total() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_in(&tmp);

        let empty = cache_stats(&paths);
        assert_eq!(empty.total_size, 0);
        assert_eq!(empty.total_count, 0);

        write_thumb(&paths, "aaa_thumb.jpg", 1000, 86_400);
        write_thumb(&paths, "bbb_thumb.jpg", 1000, 86_400);
        write_thumb(&paths, "deadbeef.jpg", 300, 10);

        let stats = cache_stats(&paths);
        assert_eq!((stats.library_size, stats.library_count), (2000, 2));
        assert_eq!((stats.disposable_size, stats.disposable_count), (300, 1));
        assert_eq!(stats.disposable_size + stats.library_size, stats.total_size);
        assert_eq!(
            stats.disposable_count + stats.library_count,
            stats.total_count
        );
    }
}
