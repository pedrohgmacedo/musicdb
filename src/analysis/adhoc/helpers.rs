//! Shared corpus-loading and section-extraction helpers for ad-hoc RE probes.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use musicdb::sections::{
    AlbumItem, AppleTimestamp, ArtistItem, BomaSection, HsmaChild, LibraryMasterHeader,
    PlaylistItem, RawLibraryFile, SocialProfileItem, TrackItem,
};

pub(crate) fn load_raw(path: &Path) -> Result<RawLibraryFile> {
    let dec =
        musicdb::decrypt_and_decompress_bytes(&fs::read(path)?, musicdb::DEFAULT_MUSICDB_KEY)?;
    Ok(RawLibraryFile::from_raw_bytes(&dec)?)
}

pub(crate) fn load_1119(corpus_dir: &Path) -> Result<RawLibraryFile> {
    let mdb =
        corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb");
    load_raw(&mdb)
}

pub(crate) fn collect_mdb(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_mdb(&p, out)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some("musicdb") {
            out.push(p);
        }
    }
    Ok(())
}

pub(crate) fn plma_of(raw: &RawLibraryFile) -> Option<&LibraryMasterHeader> {
    raw.sections.iter().find_map(|s| match &s.child {
        HsmaChild::LibraryMaster(p) => Some(&p.header),
        _ => None,
    })
}

pub(crate) fn albums_of(raw: &RawLibraryFile) -> &[AlbumItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::AlbumList(a) => Some(a.albums.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

pub(crate) fn artists_of(raw: &RawLibraryFile) -> &[ArtistItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::ArtistList(a) => Some(a.artists.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

pub(crate) fn tracks_of(raw: &RawLibraryFile) -> &[TrackItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::TrackList(t) => Some(t.tracks.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

pub(crate) fn playlists_of(raw: &RawLibraryFile) -> &[PlaylistItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::PlaylistList(p) => Some(p.playlists.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

pub(crate) fn socials_of(raw: &RawLibraryFile) -> &[SocialProfileItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::SocialProfileList(p) => Some(p.profiles.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

pub(crate) fn boma_str(bomas: &[BomaSection], subtype: u32) -> Option<&str> {
    bomas
        .iter()
        .find(|b| b.subtype == subtype)
        .and_then(BomaSection::as_str)
}

pub(crate) fn fmt_apple_ts(raw: u32) -> String {
    if raw == 0 || raw == 2 {
        format!("{raw}")
    } else {
        AppleTimestamp::new(raw)
            .to_utc()
            .map_or_else(|| format!("0x{raw:08X}"), |dt| dt.to_string())
    }
}
