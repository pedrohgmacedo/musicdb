//! Ad-hoc reverse-engineering probe for verifying promoted fields across the corpus
//! (including `Previous Libraries/`) and inspecting remaining unknown candidates.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use musicdb::sections::{BomaPayload, BomaSection, HsmaChild, RawLibraryFile, TrackItem};

pub(crate) fn run_unknowns_probe(corpus_dir: &Path) -> Result<()> {
    println!("=== CROSS-CORPUS PROMOTED-FIELD VERIFICATION ===");
    verify_across_corpus(corpus_dir)?;

    let base_path = corpus_dir
        .join("snapshots/2026-11-14-baseline/Apple Music Library.musiclibrary/Library.musicdb");
    let new_path =
        corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb");
    if base_path.is_file() && new_path.is_file() {
        let base_raw = load_raw(&base_path)?;
        let new_raw = load_raw(&new_path)?;
        println!("\n=== ITMA[228..236] DIFF (2026-11-14-baseline vs 2026-11-19) ===");
        probe_itma_228_diff(&base_raw, &new_raw);
    }

    Ok(())
}

fn load_raw(path: &Path) -> Result<RawLibraryFile> {
    let disk = fs::read(path)?;
    let raw_bytes = musicdb::decrypt_and_decompress_bytes(&disk, musicdb::DEFAULT_MUSICDB_KEY)?;
    Ok(RawLibraryFile::from_raw_bytes(&raw_bytes)?)
}

fn verify_across_corpus(corpus_dir: &Path) -> Result<()> {
    let mut files = Vec::new();
    collect_musicdb_files(corpus_dir, &mut files)?;
    files.sort();
    println!("  scanning {} .musicdb files...", files.len());

    let mut counts = InvariantCounts::default();
    let mut iama_cat_types: BTreeMap<u32, usize> = BTreeMap::new();
    let mut seen_plma: BTreeSet<(String, String, u16)> = BTreeSet::new();

    for path in &files {
        let Ok(raw) = load_raw(path) else {
            continue;
        };
        check_single_file(path, &raw, &mut counts, &mut iama_cat_types, &mut seen_plma);
    }

    println!("  hfma.hsma_count mismatches = {}", counts.hfma_count);
    println!(
        "  itma.apple_store_account_id mismatches = {}",
        counts.itma_dsid
    );
    println!(
        "  TrackNumerics.apple_store_account_id mismatches = {}",
        counts.tn_dsid
    );
    println!(
        "  TrackNumerics.drm_account_id mismatches = {}",
        counts.tn_drm_dsid
    );
    println!(
        "  TrackNumerics.is_local_mp3 mismatches = {}",
        counts.tn_local_mp3
    );
    println!(
        "  lpma.apple_store_account_id mismatches = {}",
        counts.lpma_dsid
    );
    println!(
        "  lpma.is_cloud_shared mismatches = {}",
        counts.lpma_cloud_shared
    );
    println!("  DCma sections parsed = {}", counts.dcma_sections);
    println!("  iama.catalog_album_type distribution = {iama_cat_types:?}");
    Ok(())
}

#[derive(Default)]
struct InvariantCounts {
    hfma_count: usize,
    itma_dsid: usize,
    tn_dsid: usize,
    tn_drm_dsid: usize,
    tn_local_mp3: usize,
    lpma_dsid: usize,
    lpma_cloud_shared: usize,
    dcma_sections: usize,
}

fn check_single_file(
    path: &Path,
    raw: &RawLibraryFile,
    counts: &mut InvariantCounts,
    iama_cat_types: &mut BTreeMap<u32, usize>,
    seen_plma: &mut BTreeSet<(String, String, u16)>,
) {
    let dsid = raw.outer_header.apple_store_account_id;
    if usize::try_from(raw.outer_header.hsma_count).ok() != Some(raw.sections.len()) {
        counts.hfma_count += 1;
    }

    for t in tracks_of(raw) {
        if t.header.apple_store_account_id != 0 && t.header.apple_store_account_id != dsid {
            counts.itma_dsid += 1;
        }
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload {
                if n.apple_store_account_id != 0 && n.apple_store_account_id != dsid {
                    counts.tn_dsid += 1;
                }
                if n.drm_account_id != 0 && n.drm_account_id != dsid {
                    counts.tn_drm_dsid += 1;
                }
                let is_local_mp3 = n.subrecord_kind == 1 && n.container_format_kind == 12;
                if (n.is_local_mp3 != 0) != is_local_mp3 {
                    counts.tn_local_mp3 += 1;
                }
            }
        }
    }

    for sec in &raw.sections {
        match &sec.child {
            HsmaChild::LibraryMaster(plma) => {
                let h = &plma.header;
                let fname = path
                    .file_name()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let ver = raw.outer_header.version_string();
                if seen_plma.insert((fname.clone(), ver.clone(), h.cloud_database_revision)) {
                    println!(
                        "  plma {fname} v={ver}: rev={} synced={:?} mod={:?} lease={:?}",
                        h.cloud_database_revision,
                        h.date_cloud_synced.to_utc(),
                        h.date_last_modified.to_utc(),
                        h.date_cloud_lease_expires.to_utc(),
                    );
                }
            }
            HsmaChild::AlbumList(lama) => {
                for a in &lama.albums {
                    if let Some(val) = a.header.catalog_album_type {
                        *iama_cat_types.entry(val).or_default() += 1;
                    }
                }
            }
            HsmaChild::PlaylistList(l_pma) => {
                for p in &l_pma.playlists {
                    if p.header.apple_store_account_id != 0
                        && p.header.apple_store_account_id != dsid
                    {
                        counts.lpma_dsid += 1;
                    }
                    if (p.header.is_cloud_shared != 0) != (p.header.cloud_share_mode != 0) {
                        counts.lpma_cloud_shared += 1;
                    }
                }
            }
            HsmaChild::PendingCloudDeletions(dcma) => {
                counts.dcma_sections += 1;
                println!(
                    "  DCma in {}: {} pending deletions",
                    path.display(),
                    dcma.items.len()
                );
            }
            _ => {}
        }
    }
}

fn probe_itma_228_diff(base: &RawLibraryFile, new: &RawLibraryFile) {
    let base_map: BTreeMap<u64, &TrackItem> = tracks_of(base)
        .iter()
        .map(|t| (t.header.track_persistent_id, t))
        .collect();
    let mut changed = 0usize;
    let mut by_cloud: BTreeMap<u8, usize> = BTreeMap::new();
    for t in tracks_of(new) {
        if let Some(o) = base_map.get(&t.header.track_persistent_id)
            && o.header.itunes_store_matched_id != t.header.itunes_store_matched_id
        {
            changed += 1;
            *by_cloud.entry(t.header.cloud_status).or_default() += 1;
            if changed <= 10 {
                println!(
                    "  [{:016X}] {:?} - {:?}: 0x{:016X} -> 0x{:016X} (cloud={})",
                    t.header.track_persistent_id,
                    boma_str(&t.bomas, 0x0004),
                    boma_str(&t.bomas, 0x0002),
                    o.header.itunes_store_matched_id,
                    t.header.itunes_store_matched_id,
                    t.header.cloud_status,
                );
            }
        }
    }
    println!("  total tracks with itma[228..236] changed: {changed}, by_cloud: {by_cloud:?}");
}

fn collect_musicdb_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_musicdb_files(&p, out)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some("musicdb") {
            out.push(p);
        }
    }
    Ok(())
}

fn tracks_of(raw: &RawLibraryFile) -> &[TrackItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::TrackList(list) => Some(list.tracks.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

fn boma_str(bomas: &[BomaSection], subtype: u32) -> Option<&str> {
    bomas
        .iter()
        .find(|b| b.subtype == subtype)
        .and_then(BomaSection::as_str)
}
