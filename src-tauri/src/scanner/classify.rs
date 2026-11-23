use std::path::Path;

/// Supported image extensions (spec §6.5, TASK-1D decision c).
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "tif", "tiff", "bmp", "gif", "heic", "heif", "avif",
];

/// Tier 1 RAW extensions (spec §6.5, D15, TASK-1D decision c).
pub const RAW_EXTENSIONS: &[&str] = &["dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2"];

/// Recognized video extensions (spec §6.5, §13.5: recorded as unsupported, no asset row).
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "avi", "mkv"];

/// Always skipped file or folder names (spec §6.2 step 2, TASK-1D decision d).
pub const ALWAYS_SKIPPED_NAMES: &[&str] = &[
    ".ds_store",
    "thumbs.db",
    "@eadir",
    ".trashes",
    "system volume information",
    "$recycle.bin",
];

/// Classification outcome for a discovered file name (Decision c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    Image(String),
    Raw(String),
    Video,
    Ignored,
}

/// Normalizes image extension to canonical format name (e.g. "jpg" -> "jpeg", "tif" -> "tiff").
fn canonical_image_format(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "jpeg",
        "png" => "png",
        "webp" => "webp",
        "tif" | "tiff" => "tiff",
        "bmp" => "bmp",
        "gif" => "gif",
        "heic" => "heic",
        "heif" => "heif",
        "avif" => "avif",
        _ => "unknown",
    }
}

/// Classifies a file name by its extension (case-insensitive).
pub fn classify(name: &str) -> Class {
    let ext = match Path::new(name).extension().and_then(|s| s.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return Class::Ignored,
    };

    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Class::Image(canonical_image_format(&ext).to_string());
    }

    if RAW_EXTENSIONS.contains(&ext.as_str()) {
        return Class::Raw(ext);
    }

    if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        return Class::Video;
    }

    Class::Ignored
}

/// Checks if a file or directory name is in the always-skipped list (case-insensitive).
pub fn is_always_skipped_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    ALWAYS_SKIPPED_NAMES.contains(&lower.as_str())
}

/// Checks if a directory name ends in `.photoslibrary` (case-insensitive, Decision m).
pub fn is_skipped_package_dir(name: &str) -> bool {
    name.to_lowercase().ends_with(".photoslibrary")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_extension_lists_have_no_duplicates_and_no_overlap() {
        let mut seen = HashSet::new();

        for ext in IMAGE_EXTENSIONS {
            assert!(
                seen.insert(*ext),
                "Duplicate extension in IMAGE_EXTENSIONS: {ext}"
            );
        }

        for ext in RAW_EXTENSIONS {
            assert!(
                seen.insert(*ext),
                "Duplicate or overlapping extension in RAW_EXTENSIONS: {ext}"
            );
        }

        for ext in VIDEO_EXTENSIONS {
            assert!(
                seen.insert(*ext),
                "Duplicate or overlapping extension in VIDEO_EXTENSIONS: {ext}"
            );
        }
    }

    #[test]
    fn test_classify_all_image_extensions() {
        assert_eq!(classify("test.jpg"), Class::Image("jpeg".to_string()));
        assert_eq!(classify("test.JPEG"), Class::Image("jpeg".to_string()));
        assert_eq!(classify("test.png"), Class::Image("png".to_string()));
        assert_eq!(classify("test.PNG"), Class::Image("png".to_string()));
        assert_eq!(classify("test.webp"), Class::Image("webp".to_string()));
        assert_eq!(classify("test.tif"), Class::Image("tiff".to_string()));
        assert_eq!(classify("test.TIFF"), Class::Image("tiff".to_string()));
        assert_eq!(classify("test.bmp"), Class::Image("bmp".to_string()));
        assert_eq!(classify("test.gif"), Class::Image("gif".to_string()));
        assert_eq!(classify("test.heic"), Class::Image("heic".to_string()));
        assert_eq!(classify("test.HEIF"), Class::Image("heif".to_string()));
        assert_eq!(classify("test.avif"), Class::Image("avif".to_string()));
    }

    #[test]
    fn test_classify_raw_extensions() {
        assert_eq!(classify("photo.dng"), Class::Raw("dng".to_string()));
        assert_eq!(classify("photo.CR2"), Class::Raw("cr2".to_string()));
        assert_eq!(classify("photo.cr3"), Class::Raw("cr3".to_string()));
        assert_eq!(classify("photo.NEF"), Class::Raw("nef".to_string()));
        assert_eq!(classify("photo.arw"), Class::Raw("arw".to_string()));
        assert_eq!(classify("photo.RAF"), Class::Raw("raf".to_string()));
        assert_eq!(classify("photo.orf"), Class::Raw("orf".to_string()));
        assert_eq!(classify("photo.rw2"), Class::Raw("rw2".to_string()));
    }

    #[test]
    fn test_classify_video_extensions() {
        assert_eq!(classify("movie.mp4"), Class::Video);
        assert_eq!(classify("clip.MOV"), Class::Video);
        assert_eq!(classify("video.m4v"), Class::Video);
        assert_eq!(classify("record.avi"), Class::Video);
        assert_eq!(classify("stream.MKV"), Class::Video);
    }

    #[test]
    fn test_classify_ignored_extensions_and_no_extensions() {
        assert_eq!(classify("notes.txt"), Class::Ignored);
        assert_eq!(classify("archive.zip"), Class::Ignored);
        assert_eq!(classify("program.exe"), Class::Ignored);
        assert_eq!(classify("no_extension"), Class::Ignored);
        assert_eq!(classify(".hidden_without_ext"), Class::Ignored);
    }

    #[test]
    fn test_always_skipped_names() {
        assert!(is_always_skipped_name(".DS_Store"));
        assert!(is_always_skipped_name(".ds_store"));
        assert!(is_always_skipped_name("Thumbs.db"));
        assert!(is_always_skipped_name("@eaDir"));
        assert!(is_always_skipped_name(".Trashes"));
        assert!(is_always_skipped_name("System Volume Information"));
        assert!(is_always_skipped_name("$RECYCLE.BIN"));
        assert!(is_always_skipped_name("$recycle.bin"));

        assert!(!is_always_skipped_name("photos"));
        assert!(!is_always_skipped_name("vacation.jpg"));
    }
}
