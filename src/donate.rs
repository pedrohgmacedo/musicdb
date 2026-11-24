//! Packaging utilities for donating `.musiclibrary` metadata bundles without audio or artwork files.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use directories::UserDirs;
use flate2::Compression;
use flate2::write::GzEncoder;

use musicdb::error::{MusicDbError, Result};
use musicdb::model::discover_default_library_path;

/// Allowlisted files inside a `.musiclibrary` bundle to include when donating a library.
/// Explicitly excludes `artwork/`, `artwork_originals/`, and any `Media/` audio files.
///
/// Note: `Library.musicdb` still contains account metadata such as Apple Store account IDs
/// and purchaser names/emails on purchased tracks.
pub(crate) const DONATED_BUNDLE_FILES: &[&str] = &[
    "artwork.sqlite",
    "Extras.itdb",
    "Genius.itdb",
    "Library.musicdb",
    "Library Preferences.musicdb",
    "Preferences.plist",
];

/// Allowlisted subdirectories inside a `.musiclibrary` bundle to include when donating a library.
pub(crate) const DONATED_BUNDLE_DIRS: &[&str] = &["preferences"];

/// Sidecar media preferences file located in the parent `Apple Music` / `Music` directory.
pub(crate) const MEDIA_PREFERENCES_FILENAME: &str = ".Media Preferences.plist";

/// Default output filename for `donate-library`.
pub(crate) const DEFAULT_DONATED_LIBRARY_ARCHIVE: &str = "donated-music-library.tar";

/// Default output filename for `donate-previous-libraries`.
pub(crate) const DEFAULT_DONATED_PREVIOUS_LIBRARIES_ARCHIVE: &str =
    "donated-previous-libraries.tar";

/// Summary of a completed `donate-library` or `donate-previous-libraries` archive operation.
#[derive(Debug, Clone)]
pub(crate) struct DonateReport {
    /// Destination path of the written `.tar` or `.tar.gz` archive.
    pub(crate) output_path: PathBuf,
    /// Final size of the written archive in bytes.
    pub(crate) archive_size_bytes: u64,
    /// Number of `.musiclibrary` bundles included in the archive.
    pub(crate) bundle_count: usize,
    /// Relative paths inside the archive for every packaged file.
    pub(crate) archived_paths: Vec<String>,
}

fn is_regular_file_no_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

fn is_regular_dir_no_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

/// Locates an Apple Music `.musiclibrary` bundle directory starting from `path`
/// (which may be the `.musiclibrary` bundle itself, a parent folder such as `Apple Music` or `Music`,
/// or a direct path to `Library.musicdb`).
#[must_use]
pub(crate) fn find_musiclibrary_bundle(path: &Path) -> Option<PathBuf> {
    let resolved = musicdb::RawLibraryFile::resolve_musicdb_path(path);
    if resolved.is_file()
        && resolved.file_name().and_then(|s| s.to_str()) == Some("Library.musicdb")
    {
        return resolved.parent().map(Path::to_path_buf);
    }
    None
}

/// Searches the user's default OS Music directory for a `Previous Libraries` folder.
#[must_use]
pub(crate) fn discover_default_previous_libraries_path() -> Option<PathBuf> {
    if let Some(bundle) = discover_default_library_path()
        && let Some(prev) = find_previous_libraries_for_bundle(&bundle)
    {
        return Some(prev);
    }

    let user_dirs = UserDirs::new()?;
    let music_dir = user_dirs
        .audio_dir()
        .map_or_else(|| user_dirs.home_dir().join("Music"), Path::to_path_buf);

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
#[must_use]
pub(crate) fn find_previous_libraries_for_bundle(bundle_dir: &Path) -> Option<PathBuf> {
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
#[must_use]
pub(crate) fn find_previous_libraries_dir(path: &Path) -> Option<PathBuf> {
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
/// that contain at least one allowlisted library file (ignoring symlinks).
#[must_use]
pub(crate) fn list_previous_library_bundles(prev_libs_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(prev_libs_dir) else {
        return Vec::new();
    };
    let mut bundles = Vec::new();
    for entry_res in entries {
        if let Ok(entry) = entry_res
            && entry.file_type().is_ok_and(|ft| ft.is_dir())
        {
            let p = entry.path();
            if is_regular_file_no_symlink(&p.join("Library.musicdb"))
                || p.extension().and_then(|s| s.to_str()) == Some("musiclibrary")
            {
                bundles.push(p);
            }
        }
    }
    bundles.sort();
    bundles
}

/// Locates `.Media Preferences.plist` associated with a `.musiclibrary` bundle or `Previous Libraries` folder.
#[must_use]
pub(crate) fn find_media_preferences_plist(dir: &Path) -> Option<PathBuf> {
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
        if is_regular_file_no_symlink(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Packages the allowlisted files of a single `.musiclibrary` bundle (plus `.Media Preferences.plist` if found)
/// into `output_archive` (`.tar` or `.tar.gz`), preserving the `Apple Music/<BundleName>/...` file tree.
///
/// Writes to a temporary file in the destination directory first and renames atomically on success.
///
/// # Privacy
/// While audio files and cached artwork images are excluded, `Library.musicdb` still contains personal
/// metadata including purchaser names, Apple ID email addresses, and Apple Store account IDs.
///
/// # Errors
/// Returns [`MusicDbError`] if reading the bundle files or writing the archive fails.
pub(crate) fn package_donated_library(
    bundle_dir: &Path,
    output_archive: &Path,
) -> Result<DonateReport> {
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

    with_tar_builder_atomic(output_archive, |builder| {
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

    let archive_size_bytes = fs::metadata(output_archive)?.len();
    Ok(DonateReport {
        output_path: output_archive.to_path_buf(),
        archive_size_bytes,
        bundle_count: 1,
        archived_paths,
    })
}

/// Packages all `.musiclibrary` folders inside `prev_libs_dir` into `output_archive` (`.tar` or `.tar.gz`),
/// preserving the `Previous Libraries/<BundleName>/...` file tree.
///
/// Writes to a temporary file in the destination directory first and renames atomically on success.
///
/// # Privacy
/// While audio files and cached artwork images are excluded, `Library.musicdb` still contains personal
/// metadata including purchaser names, Apple ID email addresses, and Apple Store account IDs.
///
/// # Errors
/// Returns [`MusicDbError`] if no bundles are found or if reading/writing the archive fails.
pub(crate) fn package_donated_previous_libraries(
    prev_libs_dir: &Path,
    output_archive: &Path,
) -> Result<DonateReport> {
    let bundles = list_previous_library_bundles(prev_libs_dir);
    if bundles.is_empty() {
        return Err(MusicDbError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "no .musiclibrary bundles found inside {}",
                prev_libs_dir.display()
            ),
        )));
    }

    let mut archived_paths = Vec::new();
    with_tar_builder_atomic(output_archive, |builder| {
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

    let archive_size_bytes = fs::metadata(output_archive)?.len();
    Ok(DonateReport {
        output_path: output_archive.to_path_buf(),
        archive_size_bytes,
        bundle_count: bundles.len(),
        archived_paths,
    })
}

fn temp_sibling_path(output_archive: &Path) -> PathBuf {
    static TEMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let parent = output_archive
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = output_archive
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("archive.tar");
    let pid = std::process::id();
    let seq = TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    parent.join(format!(".{file_name}.{pid}.{seq}.tmp"))
}

fn with_tar_builder_atomic<F>(output_archive: &Path, mut f: F) -> Result<()>
where
    F: FnMut(&mut dyn ArchiveSink) -> Result<()>,
{
    let temp_path = temp_sibling_path(output_archive);
    let write_res = write_tar_to_path(&temp_path, output_archive, &mut f);
    if let Err(err) = write_res {
        // Best-effort cleanup of partial temporary archive on failure.
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }
    if let Err(err) = fs::rename(&temp_path, output_archive) {
        // Best-effort cleanup if rename fails.
        let _ = fs::remove_file(&temp_path);
        return Err(MusicDbError::Io(err));
    }
    Ok(())
}

trait ArchiveSink {
    fn append_file(&mut self, archive_rel_path: &Path, file: &mut File) -> Result<()>;
}

impl<W: Write> ArchiveSink for tar::Builder<W> {
    fn append_file(&mut self, archive_rel_path: &Path, file: &mut File) -> Result<()> {
        tar::Builder::append_file(self, archive_rel_path, file)?;
        Ok(())
    }
}

fn write_tar_to_path<F>(temp_path: &Path, final_path: &Path, f: &mut F) -> Result<()>
where
    F: FnMut(&mut dyn ArchiveSink) -> Result<()>,
{
    let file = File::create(temp_path)?;
    let is_gzip = final_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gz") || ext.eq_ignore_ascii_case("tgz"));

    if is_gzip {
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        f(&mut builder)?;
        let encoder = builder.into_inner()?;
        let mut file = encoder.finish()?;
        file.flush()?;
    } else {
        let mut builder = tar::Builder::new(file);
        f(&mut builder)?;
        let mut file = builder.into_inner()?;
        file.flush()?;
    }
    Ok(())
}

fn append_bundle_to_tar(
    builder: &mut dyn ArchiveSink,
    bundle_dir: &Path,
    archive_prefix: &Path,
    archived_paths: &mut Vec<String>,
) -> Result<()> {
    for &filename in DONATED_BUNDLE_FILES {
        let src = bundle_dir.join(filename);
        if is_regular_file_no_symlink(&src) {
            let rel = archive_prefix.join(filename);
            append_file_to_tar(builder, &src, &rel, archived_paths)?;
        }
    }

    for &dirname in DONATED_BUNDLE_DIRS {
        let src_dir = bundle_dir.join(dirname);
        if is_regular_dir_no_symlink(&src_dir) {
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
    builder: &mut dyn ArchiveSink,
    src_dir: &Path,
    archive_prefix: &Path,
    archived_paths: &mut Vec<String>,
) -> Result<()> {
    let mut entries = Vec::new();
    for entry_res in fs::read_dir(src_dir)? {
        let entry = entry_res?;
        let ft = entry.file_type()?;
        // Never follow symlinks: skip any symlink entry.
        if ft.is_symlink() {
            continue;
        }
        entries.push((entry.path(), ft));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (entry_path, ft) in entries {
        let Some(name) = entry_path.file_name() else {
            continue;
        };
        let rel = archive_prefix.join(name);
        if ft.is_dir() {
            append_dir_recursive_to_tar(builder, &entry_path, &rel, archived_paths)?;
        } else if ft.is_file() {
            append_file_to_tar(builder, &entry_path, &rel, archived_paths)?;
        }
    }
    Ok(())
}

fn append_file_to_tar(
    builder: &mut dyn ArchiveSink,
    src_file: &Path,
    archive_rel_path: &Path,
    archived_paths: &mut Vec<String>,
) -> Result<()> {
    let mut f = File::open(src_file)?;
    builder.append_file(archive_rel_path, &mut f)?;
    archived_paths.push(archive_rel_path.to_string_lossy().replace('\\', "/"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_donate_library_and_previous_libraries() {
        let tmp_dir = tempfile::tempdir().expect("create tempdir");
        let tmp_root = tmp_dir.path();
        let apple_music_dir = tmp_root.join("Apple Music");
        let bundle_dir = apple_music_dir.join("Apple Music Library.musiclibrary");
        let prefs_dir = bundle_dir.join("preferences");
        let artwork_dir = bundle_dir.join("artwork");
        let media_dir = apple_music_dir.join("Media");
        let prev_dir = apple_music_dir
            .join("Previous Libraries")
            .join("Apple Music Library [2026-10-28].musiclibrary");

        std::fs::create_dir_all(&prefs_dir).unwrap();
        std::fs::create_dir_all(&artwork_dir).unwrap();
        std::fs::create_dir_all(&media_dir).unwrap();
        std::fs::create_dir_all(&prev_dir).unwrap();

        std::fs::write(apple_music_dir.join(".Media Preferences.plist"), b"plist").unwrap();
        std::fs::write(bundle_dir.join("Library.musicdb"), b"mdb").unwrap();
        std::fs::write(bundle_dir.join("Library Preferences.musicdb"), b"pmdb").unwrap();
        std::fs::write(bundle_dir.join("Preferences.plist"), b"prefs").unwrap();
        std::fs::write(bundle_dir.join("artwork.sqlite"), b"sqlite").unwrap();
        std::fs::write(bundle_dir.join("Extras.itdb"), b"extras").unwrap();
        std::fs::write(bundle_dir.join("Genius.itdb"), b"genius").unwrap();
        std::fs::write(prefs_dir.join("Songs.plist"), b"songs").unwrap();
        // Should be excluded (artwork, media, and symlinks):
        std::fs::write(artwork_dir.join("big_image.jpeg"), b"excluded").unwrap();
        std::fs::write(media_dir.join("song.mp3"), b"excluded").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            media_dir.join("song.mp3"),
            prefs_dir.join("symlink_secret.plist"),
        )
        .unwrap();

        std::fs::write(prev_dir.join("Library.musicdb"), b"prev_mdb").unwrap();
        std::fs::write(prev_dir.join("Preferences.plist"), b"prev_prefs").unwrap();

        let found_bundle = find_musiclibrary_bundle(&apple_music_dir).expect("find bundle");
        assert_eq!(found_bundle, bundle_dir);

        let tar_lib = tmp_root.join(DEFAULT_DONATED_LIBRARY_ARCHIVE);
        let lib_report = package_donated_library(&found_bundle, &tar_lib).expect("package library");
        assert_eq!(lib_report.bundle_count, 1);
        assert!(
            lib_report
                .archived_paths
                .contains(&"Apple Music/.Media Preferences.plist".to_string())
        );
        assert!(
            lib_report.archived_paths.contains(
                &"Apple Music/Apple Music Library.musiclibrary/Library.musicdb".to_string()
            )
        );
        assert!(lib_report.archived_paths.contains(
            &"Apple Music/Apple Music Library.musiclibrary/preferences/Songs.plist".to_string()
        ));
        assert!(
            !lib_report
                .archived_paths
                .iter()
                .any(|p| p.contains("big_image.jpeg")
                    || p.contains("song.mp3")
                    || p.contains("symlink_secret.plist"))
        );

        let found_prev = find_previous_libraries_for_bundle(&found_bundle).expect("find prev");
        let tar_prev = tmp_root.join(DEFAULT_DONATED_PREVIOUS_LIBRARIES_ARCHIVE);
        let prev_report =
            package_donated_previous_libraries(&found_prev, &tar_prev).expect("package prev");
        assert_eq!(prev_report.bundle_count, 1);
        assert!(
            prev_report.archived_paths.contains(
                &"Previous Libraries/Apple Music Library [2026-10-28].musiclibrary/Library.musicdb"
                    .to_string()
            )
        );
    }
}
