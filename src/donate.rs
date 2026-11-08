use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use directories::UserDirs;
use flate2::Compression;
use flate2::write::GzEncoder;

use crate::error::{MusicDbError, Result};
use crate::model::discover_default_library_path;

/// Allowlisted files inside a `.musiclibrary` bundle to include when donating a library.
/// Explicitly excludes `artwork/`, `artwork_originals/`, and any `Media/` audio files.
pub const DONATED_BUNDLE_FILES: &[&str] = &[
    "artwork.sqlite",
    "Extras.itdb",
    "Genius.itdb",
    "Library.musicdb",
    "Library Preferences.musicdb",
    "Preferences.plist",
];

/// Allowlisted subdirectories inside a `.musiclibrary` bundle to include when donating a library.
pub const DONATED_BUNDLE_DIRS: &[&str] = &["preferences"];

/// Sidecar media preferences file located in the parent `Apple Music` / `Music` directory.
pub const MEDIA_PREFERENCES_FILENAME: &str = ".Media Preferences.plist";

/// Default output filename for `donate-library`.
pub const DEFAULT_DONATED_LIBRARY_ARCHIVE: &str = "donated-music-library.tar";

/// Default output filename for `donate-previous-libraries`.
pub const DEFAULT_DONATED_PREVIOUS_LIBRARIES_ARCHIVE: &str = "donated-previous-libraries.tar";

/// Summary of a completed `donate-library` or `donate-previous-libraries` archive operation.
#[derive(Debug, Clone)]
pub struct DonateReport {
    pub output_path: PathBuf,
    pub archive_size_bytes: u64,
    pub bundle_count: usize,
    pub archived_paths: Vec<String>,
}

/// Locates an Apple Music `.musiclibrary` bundle directory starting from `path`
/// (which may be the `.musiclibrary` bundle itself, a parent folder such as `Apple Music` or `Music`,
/// or a direct path to `Library.musicdb`).
pub fn find_musiclibrary_bundle(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        if path.file_name().and_then(|s| s.to_str()) == Some("Library.musicdb") {
            return path.parent().map(Path::to_path_buf);
        }
        return None;
    }
    if !path.is_dir() {
        return None;
    }

    // 1. `path` itself is a `.musiclibrary` bundle containing `Library.musicdb`
    if path.join("Library.musicdb").is_file() {
        return Some(path.to_path_buf());
    }

    // 2. Well-known Windows and macOS relative locations under `path`
    let well_known = [
        path.join("Apple Music Library.musiclibrary"),
        path.join("Music Library.musiclibrary"),
        path.join("Apple Music")
            .join("Apple Music Library.musiclibrary"),
        path.join("Music").join("Music Library.musiclibrary"),
    ];
    for candidate in well_known {
        if candidate.join("Library.musicdb").is_file() {
            return Some(candidate);
        }
    }

    // 3. Scan direct subdirectories for any `*.musiclibrary` or folder containing `Library.musicdb`
    if let Ok(entries) = fs::read_dir(path) {
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for dir in subdirs {
            let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name == "Previous Libraries" || name == "Media" || name == "Media.localized" {
                continue;
            }
            if dir.join("Library.musicdb").is_file() {
                return Some(dir);
            }
        }
    }

    None
}

/// Searches the user's default OS Music directory for a `Previous Libraries` folder.
pub fn discover_default_previous_libraries_path() -> Option<PathBuf> {
    if let Some(bundle) = discover_default_library_path()
        && let Some(prev) = find_previous_libraries_for_bundle(&bundle)
    {
        return Some(prev);
    }

    let user_dirs = UserDirs::new()?;
    let music_dir = user_dirs
        .audio_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| user_dirs.home_dir().join("Music"));

    let candidates = [
        music_dir.join("Apple Music").join("Previous Libraries"),
        music_dir.join("Music").join("Previous Libraries"),
        music_dir.join("Previous Libraries"),
    ];

    candidates
        .into_iter()
        .find(|p| p.is_dir() && !list_previous_library_bundles(p).is_empty())
}

/// Given a `.musiclibrary` bundle path, checks if a sibling or child `Previous Libraries`
/// directory exists with at least one previous library bundle.
pub fn find_previous_libraries_for_bundle(bundle_dir: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(parent) = bundle_dir.parent() {
        candidates.push(parent.join("Previous Libraries"));
    }
    candidates.push(bundle_dir.join("Previous Libraries"));

    candidates
        .into_iter()
        .find(|p| p.is_dir() && !list_previous_library_bundles(p).is_empty())
}

/// Resolves a `Previous Libraries` directory from an arbitrary `path` (which may be the
/// `Previous Libraries` folder itself, its parent `Apple Music` / `Music` folder, or a `.musiclibrary` bundle).
pub fn find_previous_libraries_dir(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }

    let direct_candidates = [
        path.to_path_buf(),
        path.join("Previous Libraries"),
        path.join("Apple Music").join("Previous Libraries"),
        path.join("Music").join("Previous Libraries"),
    ];
    for candidate in direct_candidates {
        if candidate.is_dir() && !list_previous_library_bundles(&candidate).is_empty() {
            return Some(candidate);
        }
    }

    if let Some(parent) = path.parent() {
        let sibling = parent.join("Previous Libraries");
        if sibling.is_dir() && !list_previous_library_bundles(&sibling).is_empty() {
            return Some(sibling);
        }
    }

    None
}

/// Lists all `.musiclibrary` subdirectories inside a `Previous Libraries` directory
/// that contain at least one allowlisted library file.
pub fn list_previous_library_bundles(prev_libs_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(prev_libs_dir) else {
        return Vec::new();
    };
    let mut bundles: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && (p.join("Library.musicdb").is_file()
                    || p.extension().and_then(|s| s.to_str()) == Some("musiclibrary"))
        })
        .collect();
    bundles.sort();
    bundles
}

/// Locates `.Media Preferences.plist` associated with a `.musiclibrary` bundle or `Previous Libraries` folder.
pub fn find_media_preferences_plist(dir: &Path) -> Option<PathBuf> {
    let mut search_dirs = vec![dir.to_path_buf()];
    if let Some(parent) = dir.parent() {
        search_dirs.push(parent.to_path_buf());
        search_dirs.push(parent.join("Media"));
        search_dirs.push(parent.join("Media.localized"));
    }
    search_dirs.push(dir.join("Media"));
    search_dirs.push(dir.join("Media.localized"));

    for d in search_dirs {
        let candidate = d.join(MEDIA_PREFERENCES_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Packages the allowlisted files of a single `.musiclibrary` bundle (plus `.Media Preferences.plist` if found)
/// into `output_archive` (`.tar` or `.tar.gz`), preserving the `Apple Music/<BundleName>/...` file tree.
pub fn package_donated_library(bundle_dir: &Path, output_archive: &Path) -> Result<DonateReport> {
    let bundle_name = bundle_dir
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Apple Music Library.musiclibrary");

    let parent_name = bundle_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .filter(|&name| name == "Apple Music" || name == "Music")
        .unwrap_or("Apple Music");

    let archive_bundle_prefix = Path::new(parent_name).join(bundle_name);
    let mut archived_paths = Vec::new();

    with_tar_builder(output_archive, |builder| {
        if let Some(media_plist) = find_media_preferences_plist(bundle_dir) {
            let rel = Path::new(parent_name).join(MEDIA_PREFERENCES_FILENAME);
            append_file_to_tar(builder, &media_plist, &rel, &mut archived_paths)?;
        }
        append_bundle_to_tar(
            builder,
            bundle_dir,
            &archive_bundle_prefix,
            &mut archived_paths,
        )?;
        Ok(())
    })?;

    let archive_size_bytes = fs::metadata(output_archive).map(|m| m.len()).unwrap_or(0);
    Ok(DonateReport {
        output_path: output_archive.to_path_buf(),
        archive_size_bytes,
        bundle_count: 1,
        archived_paths,
    })
}

/// Packages all `.musiclibrary` folders inside `prev_libs_dir` into `output_archive` (`.tar` or `.tar.gz`),
/// preserving the `Previous Libraries/<BundleName>/...` file tree.
pub fn package_donated_previous_libraries(
    prev_libs_dir: &Path,
    output_archive: &Path,
) -> Result<DonateReport> {
    let bundles = list_previous_library_bundles(prev_libs_dir);
    if bundles.is_empty() {
        return Err(MusicDbError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "No .musiclibrary bundles found inside {}",
                prev_libs_dir.display()
            ),
        )));
    }

    let mut archived_paths = Vec::new();
    with_tar_builder(output_archive, |builder| {
        if let Some(media_plist) = find_media_preferences_plist(prev_libs_dir) {
            let rel = Path::new("Previous Libraries").join(MEDIA_PREFERENCES_FILENAME);
            append_file_to_tar(builder, &media_plist, &rel, &mut archived_paths)?;
        }
        for bundle in &bundles {
            let Some(bundle_name) = bundle.file_name() else {
                continue;
            };
            let prefix = Path::new("Previous Libraries").join(bundle_name);
            append_bundle_to_tar(builder, bundle, &prefix, &mut archived_paths)?;
        }
        Ok(())
    })?;

    let archive_size_bytes = fs::metadata(output_archive).map(|m| m.len()).unwrap_or(0);
    Ok(DonateReport {
        output_path: output_archive.to_path_buf(),
        archive_size_bytes,
        bundle_count: bundles.len(),
        archived_paths,
    })
}

fn with_tar_builder<F>(output_archive: &Path, f: F) -> Result<()>
where
    F: FnOnce(&mut tar::Builder<Box<dyn Write>>) -> Result<()>,
{
    let file = File::create(output_archive)?;
    let is_gzip = output_archive
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.ends_with(".tar.gz") || n.ends_with(".tgz"));

    let writer: Box<dyn Write> = if is_gzip {
        Box::new(GzEncoder::new(file, Compression::default()))
    } else {
        Box::new(file)
    };

    let mut builder = tar::Builder::new(writer);
    f(&mut builder)?;
    let mut inner = builder.into_inner()?;
    inner.flush()?;
    Ok(())
}

fn append_bundle_to_tar(
    builder: &mut tar::Builder<Box<dyn Write>>,
    bundle_dir: &Path,
    archive_prefix: &Path,
    archived_paths: &mut Vec<String>,
) -> Result<()> {
    for &filename in DONATED_BUNDLE_FILES {
        let src = bundle_dir.join(filename);
        if src.is_file() {
            let rel = archive_prefix.join(filename);
            append_file_to_tar(builder, &src, &rel, archived_paths)?;
        }
    }

    for &dirname in DONATED_BUNDLE_DIRS {
        let src_dir = bundle_dir.join(dirname);
        if src_dir.is_dir() {
            append_dir_recursive_to_tar(
                builder,
                &src_dir,
                &archive_prefix.join(dirname),
                archived_paths,
            )?;
        }
    }

    Ok(())
}

fn append_dir_recursive_to_tar(
    builder: &mut tar::Builder<Box<dyn Write>>,
    src_dir: &Path,
    archive_prefix: &Path,
    archived_paths: &mut Vec<String>,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(src_dir)?.flatten().map(|e| e.path()).collect();
    entries.sort();

    for entry in entries {
        let Some(name) = entry.file_name() else {
            continue;
        };
        let rel = archive_prefix.join(name);
        if entry.is_dir() {
            append_dir_recursive_to_tar(builder, &entry, &rel, archived_paths)?;
        } else if entry.is_file() {
            append_file_to_tar(builder, &entry, &rel, archived_paths)?;
        }
    }
    Ok(())
}

fn append_file_to_tar(
    builder: &mut tar::Builder<Box<dyn Write>>,
    src_file: &Path,
    archive_rel_path: &Path,
    archived_paths: &mut Vec<String>,
) -> Result<()> {
    let mut f = File::open(src_file)?;
    builder.append_file(archive_rel_path, &mut f)?;
    archived_paths.push(archive_rel_path.to_string_lossy().replace('\\', "/"));
    Ok(())
}
