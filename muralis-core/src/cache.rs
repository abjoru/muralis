use std::path::Path;

use tracing::info;

use crate::error::Result;
use crate::paths::MuralisPaths;

pub struct CacheStats {
    pub thumbnails_size: u64,
    pub previews_size: u64,
    pub total_size: u64,
    pub thumbnail_count: usize,
    pub preview_count: usize,
}

/// Scan cache directories and return size stats.
pub fn cache_stats(paths: &MuralisPaths) -> CacheStats {
    let (thumbnails_size, thumbnail_count) = dir_stats(&paths.thumbnails_dir());
    let (previews_size, preview_count) = dir_stats(&paths.previews_dir());
    CacheStats {
        thumbnails_size,
        previews_size,
        total_size: thumbnails_size + previews_size,
        thumbnail_count,
        preview_count,
    }
}

/// Prune cache to stay under max_bytes. Deletes oldest preview files first,
/// then oldest thumbnails. Never touches wallpaper files (those are favorites).
pub fn prune_cache(paths: &MuralisPaths, max_bytes: u64) -> Result<u64> {
    let stats = cache_stats(paths);
    if stats.total_size <= max_bytes {
        return Ok(0);
    }

    let mut freed = 0u64;
    let target = stats.total_size - max_bytes;

    // prune previews first (less important)
    freed += prune_dir(&paths.previews_dir(), target)?;

    if freed < target {
        // prune thumbnails if still over
        freed += prune_dir(&paths.thumbnails_dir(), target - freed)?;
    }

    info!(freed_bytes = freed, "cache pruned");
    Ok(freed)
}

fn prune_dir(dir: &Path, target: u64) -> Result<u64> {
    let mut entries: Vec<(std::path::PathBuf, u64, std::time::SystemTime)> = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            if let Ok(meta) = entry.metadata() {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                entries.push((entry.path(), meta.len(), modified));
            }
        }
    }

    // sort by modified time ascending (oldest first)
    entries.sort_by_key(|(_, _, time)| *time);

    let mut freed = 0u64;
    for (path, size, _) in entries {
        if freed >= target {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            freed += size;
        }
    }

    Ok(freed)
}

fn dir_stats(dir: &Path) -> (u64, usize) {
    let mut total = 0u64;
    let mut count = 0usize;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
                count += 1;
            }
        }
    }
    (total, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_stats_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        paths.ensure_dirs().unwrap();

        let stats = cache_stats(&paths);
        assert_eq!(stats.total_size, 0);
        assert_eq!(stats.thumbnail_count, 0);
        assert_eq!(stats.preview_count, 0);
    }

    #[test]
    fn test_prune_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        paths.ensure_dirs().unwrap();

        // create some preview files
        for i in 0..5 {
            let path = paths.previews_dir().join(format!("preview_{i}.jpg"));
            std::fs::write(&path, vec![0u8; 1000]).unwrap();
            // stagger modification times
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let stats = cache_stats(&paths);
        assert_eq!(stats.preview_count, 5);
        assert_eq!(stats.previews_size, 5000);

        // prune to 3000 bytes (should delete 2 oldest)
        let freed = prune_cache(&paths, 3000).unwrap();
        assert_eq!(freed, 2000);

        let stats = cache_stats(&paths);
        assert_eq!(stats.preview_count, 3);
    }
}
