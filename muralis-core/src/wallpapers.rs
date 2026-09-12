use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::ImageReader;
use sha2::{Digest, Sha256};

use crate::db::Database;
use crate::error::{IoAt as _, MuralisError, Result};
use crate::models::{SourceType, Wallpaper, WallpaperPreview};
use crate::paths::MuralisPaths;

const THUMBNAIL_WIDTH: u32 = 300;

pub struct WallpaperManager {
    paths: MuralisPaths,
}

impl WallpaperManager {
    pub fn new(paths: MuralisPaths) -> Self {
        Self { paths }
    }

    /// Favorite a wallpaper: download, hash, save, generate thumbnail, insert to DB.
    /// Returns the wallpaper ID (SHA-256 hash).
    pub fn favorite(
        &self,
        db: &Database,
        preview: &WallpaperPreview,
        data: &[u8],
    ) -> Result<String> {
        let hash = sha256_hex(data);

        // dedup check
        if db.wallpaper_exists(&hash)? {
            return Ok(hash);
        }

        let ext = guess_extension(data);
        let file_path = self.paths.wallpapers_dir().join(format!("{hash}.{ext}"));
        std::fs::write(&file_path, data).at("write", &file_path)?;

        // generate thumbnail
        self.generate_thumbnail(data, &hash)?;

        let now = chrono::Utc::now().to_rfc3339();
        let wp = Wallpaper {
            id: hash.clone(),
            source_type: preview.source_type.clone(),
            source_id: preview.source_id.clone(),
            source_url: Some(preview.source_url.clone()),
            width: preview.width,
            height: preview.height,
            tags: preview.tags.clone(),
            file_path: file_path.to_string_lossy().to_string(),
            added_at: now,
            last_used: None,
            use_count: 0,
        };

        // if dimensions unknown (e.g. feed), read from image
        let wp = if wp.width == 0 || wp.height == 0 {
            match ImageReader::new(Cursor::new(data)).with_guessed_format() {
                Ok(reader) => match reader.decode() {
                    Ok(img) => Wallpaper {
                        width: img.width(),
                        height: img.height(),
                        ..wp
                    },
                    Err(_) => wp,
                },
                Err(_) => wp,
            }
        } else {
            wp
        };

        db.insert_wallpaper(&wp)?;
        Ok(hash)
    }

    /// Unfavorite: remove from DB and delete files.
    pub fn unfavorite(&self, db: &Database, id: &str) -> Result<()> {
        let wp = db.get_wallpaper(id)?;

        // delete wallpaper file
        let wp_path = Path::new(&wp.file_path);
        if wp_path.exists() {
            std::fs::remove_file(wp_path).at("remove", wp_path)?;
        }

        // delete thumbnail
        let thumb_path = self.thumbnail_path(id);
        if thumb_path.exists() {
            std::fs::remove_file(&thumb_path).at("remove", &thumb_path)?;
        }

        db.delete_wallpaper(id)?;
        Ok(())
    }

    /// List all favorited wallpapers.
    pub fn list(&self, db: &Database) -> Result<Vec<Wallpaper>> {
        db.list_wallpapers()
    }

    /// Get a single wallpaper by ID.
    pub fn get(&self, db: &Database, id: &str) -> Result<Wallpaper> {
        db.get_wallpaper(id)
    }

    /// Get the path where a wallpaper file should be.
    pub fn wallpaper_path(&self, id: &str, ext: &str) -> PathBuf {
        self.paths.wallpapers_dir().join(format!("{id}.{ext}"))
    }

    /// Get the thumbnail path for a wallpaper.
    pub fn thumbnail_path(&self, id: &str) -> PathBuf {
        self.paths.thumbnails_dir().join(format!("{id}_thumb.jpg"))
    }

    /// Favorite from a local file path.
    pub fn favorite_local(&self, db: &Database, path: &Path) -> Result<String> {
        if !path.exists() {
            return Err(MuralisError::FileNotFound(path.to_path_buf()));
        }

        let data = std::fs::read(path).at("read", path)?;
        let hash = sha256_hex(&data);

        if db.wallpaper_exists(&hash)? {
            return Ok(hash);
        }

        let ext = guess_extension(&data);
        let dest = self.paths.wallpapers_dir().join(format!("{hash}.{ext}"));
        std::fs::copy(path, &dest).at("write", &dest)?;

        self.generate_thumbnail(&data, &hash)?;

        let img = ImageReader::new(Cursor::new(&data))
            .with_guessed_format()?
            .decode()?;

        let now = chrono::Utc::now().to_rfc3339();
        let wp = Wallpaper {
            id: hash.clone(),
            source_type: SourceType::new("local"),
            source_id: path.to_string_lossy().to_string(),
            source_url: None,
            width: img.width(),
            height: img.height(),
            tags: Vec::new(),
            file_path: dest.to_string_lossy().to_string(),
            added_at: now,
            last_used: None,
            use_count: 0,
        };

        db.insert_wallpaper(&wp)?;
        Ok(hash)
    }

    fn generate_thumbnail(&self, data: &[u8], hash: &str) -> Result<()> {
        let img = ImageReader::new(Cursor::new(data))
            .with_guessed_format()?
            .decode()?;

        let thumb_height =
            (THUMBNAIL_WIDTH as f64 / img.width() as f64 * img.height() as f64) as u32;
        let thumb = img.resize_exact(THUMBNAIL_WIDTH, thumb_height, FilterType::Lanczos3);

        // The encoder's own IO error carries no path, so an unwritable
        // thumbnails directory would report as bare an errno as a raw
        // `fs::write` did. Encoding failures stay image errors.
        let thumb_path = self.thumbnail_path(hash);
        thumb.save(&thumb_path).map_err(|e| match e {
            image::ImageError::IoError(cause) => MuralisError::IoAt {
                op: "write",
                path: thumb_path.clone(),
                cause,
            },
            other => other.into(),
        })?;
        Ok(())
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn guess_extension(data: &[u8]) -> &'static str {
    if data.starts_with(b"\x89PNG") {
        "png"
    } else if data.starts_with(b"\xff\xd8") {
        "jpg"
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        "webp"
    } else {
        "jpg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// A throwaway XDG root, with nothing under it. Nothing calls
    /// `ensure_dirs` here on purpose: this is the machine the daemon has
    /// never run on.
    fn bare_root() -> (MuralisPaths, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        (paths, tmp)
    }

    fn a_jpeg() -> Vec<u8> {
        let img = image::RgbImage::new(64, 32);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();
        buf
    }

    fn a_preview() -> WallpaperPreview {
        WallpaperPreview {
            source_type: SourceType::new("wallhaven"),
            source_id: "test_001".into(),
            source_url: "https://example.com".into(),
            thumbnail_url: "https://example.com/thumb.jpg".into(),
            full_url: "https://example.com/full.jpg".into(),
            width: 64,
            height: 32,
            tags: vec!["test".into()],
        }
    }

    /// The defect's headline symptom: keeping on a machine whose directories
    /// do not exist yet failed with a bare `No such file or directory (os
    /// error 2)` — naming neither the operation nor the location, and
    /// mistaken each time for a fault in whatever tripped it.
    #[test]
    fn a_keep_that_cannot_write_names_the_file_it_meant_to_write() {
        let (paths, _tmp) = bare_root();
        let wallpapers_dir = paths.wallpapers_dir();
        let db = Database::open_in_memory().unwrap();
        let manager = WallpaperManager::new(paths);

        let err = manager
            .favorite(&db, &a_preview(), &a_jpeg())
            .expect_err("there is no directory to write into");

        let message = err.to_string();
        assert!(
            message.contains(&wallpapers_dir.display().to_string()),
            "a user must be able to see which directory is missing: {message}"
        );
    }

    /// The keep path writes twice, and the second write goes through the
    /// image encoder — whose own IO error is just as unqualified as the
    /// first's was.
    #[test]
    fn a_thumbnail_that_cannot_be_written_names_its_path() {
        let (paths, _tmp) = bare_root();
        std::fs::create_dir_all(paths.wallpapers_dir()).unwrap();
        let thumbnails_dir = paths.thumbnails_dir();
        let db = Database::open_in_memory().unwrap();
        let manager = WallpaperManager::new(paths);

        let err = manager
            .favorite(&db, &a_preview(), &a_jpeg())
            .expect_err("there is no thumbnails directory to write into");

        let message = err.to_string();
        assert!(
            message.contains(&thumbnails_dir.display().to_string()),
            "an encoder's errno is no more actionable than the filesystem's: {message}"
        );
    }

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_guess_extension() {
        assert_eq!(guess_extension(&[0x89, b'P', b'N', b'G']), "png");
        assert_eq!(guess_extension(&[0xff, 0xd8, 0xff, 0xe0]), "jpg");
        assert_eq!(guess_extension(b"RIFF\x00\x00\x00\x00WEBP"), "webp");
        assert_eq!(guess_extension(b"unknown"), "jpg");
    }

    /// Both keep paths — by URL and by **Preview** — hash the same downloaded
    /// bytes, so the same image kept twice is one **Library** row no matter
    /// which Preview described it.
    #[test]
    fn the_same_image_kept_from_two_descriptions_is_one_library_row() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        paths.ensure_dirs().unwrap();
        let db = Database::open_in_memory().unwrap();
        let manager = WallpaperManager::new(paths);

        let img = image::RgbImage::new(64, 32);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();

        // What a browsed result carries: the category page as source_url, no
        // resolvable per-image page.
        let browsed = WallpaperPreview {
            source_type: SourceType::new("ultrawide"),
            source_id: "aishot-5774.jpg".into(),
            source_url: "https://www.ultrawidewallpapers.net/space-wallpapers".into(),
            thumbnail_url: "https://www.ultrawidewallpapers.net/thumb.php?image=a".into(),
            full_url: "https://www.ultrawidewallpapers.net/wallpapers/329/highres/a.jpg".into(),
            width: 7680,
            height: 2160,
            tags: vec!["Space Wallpapers".into()],
        };
        // What `resolve_url` reconstructs from the master URL: same image,
        // different description.
        let resolved = WallpaperPreview {
            source_url: "https://www.ultrawidewallpapers.net/".into(),
            tags: vec!["ultrawidewallpapers.net".into()],
            ..browsed.clone()
        };

        let by_preview = manager.favorite(&db, &browsed, &buf).unwrap();
        let by_url = manager.favorite(&db, &resolved, &buf).unwrap();

        assert_eq!(by_preview, by_url, "identity is the bytes, not the URL");
        assert_eq!(db.wallpaper_count().unwrap(), 1);
    }

    #[test]
    fn test_favorite_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = MuralisPaths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        paths.ensure_dirs().unwrap();

        let db = Database::open_in_memory().unwrap();
        let manager = WallpaperManager::new(paths.clone());

        // create a tiny valid JPEG-like test image using the image crate
        let img = image::RgbImage::new(100, 100);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();

        let preview = WallpaperPreview {
            source_type: SourceType::new("wallhaven"),
            source_id: "test_001".into(),
            source_url: "https://example.com".into(),
            thumbnail_url: "https://example.com/thumb.jpg".into(),
            full_url: "https://example.com/full.jpg".into(),
            width: 100,
            height: 100,
            tags: vec!["test".into()],
        };

        // favorite
        let id = manager.favorite(&db, &preview, &buf).unwrap();
        assert!(!id.is_empty());

        // verify in DB
        let wp = db.get_wallpaper(&id).unwrap();
        assert_eq!(wp.source_id, "test_001");
        assert_eq!(wp.tags, vec!["test"]);
        assert!(Path::new(&wp.file_path).exists());

        // thumbnail exists
        assert!(manager.thumbnail_path(&id).exists());

        // dedup: same data returns same hash, no error
        let id2 = manager.favorite(&db, &preview, &buf).unwrap();
        assert_eq!(id, id2);
        assert_eq!(db.wallpaper_count().unwrap(), 1);

        // unfavorite
        manager.unfavorite(&db, &id).unwrap();
        assert_eq!(db.wallpaper_count().unwrap(), 0);
        assert!(!Path::new(&wp.file_path).exists());
        assert!(!manager.thumbnail_path(&id).exists());
    }
}
