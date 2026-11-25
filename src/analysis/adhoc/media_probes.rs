//! Ad-hoc probe: cross-checking `ffprobe`/`exiftool`/`.movpkg` discoveries across the corpus.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::BomaPayload;

use super::helpers::{boma_str, collect_mdb, load_1119, load_raw, tracks_of};

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc verification of ffprobe/exiftool/movpkg discoveries"
)]
pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    let raw_1119 = load_1119(corpus_dir)?;
    println!("=== FFPROBE / EXIFTOOL / MOVPKG DISCOVERY VERIFICATION ===");

    // 8.1 TrackNumerics[45] (unk_42_54[3]) vs (sub==3 || date_released != 0 || u116 != 0)
    let mut b45_vs_formula: BTreeMap<(u32, u8, bool, bool), usize> = BTreeMap::new();
    let mut b45_mismatches = Vec::new();
    for t in tracks_of(&raw_1119) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload {
                let b45 = n.unk_42_54.get(3).copied().unwrap_or(0);
                let has_rel = n.date_released.raw() != 0;
                let has_u116 = *n.unk_116_cloud_timestamp != 0;
                *b45_vs_formula
                    .entry((n.subrecord_kind, b45, has_rel, has_u116))
                    .or_default() += 1;
                let expected = n.subrecord_kind == 3 || has_rel || has_u116;
                if (b45 == 1) != expected && b45_mismatches.len() < 15 {
                    b45_mismatches.push(format!(
                        "[{:016X}] sub={} b45={} drel={} u116={} cloud={} {} - {:?}",
                        t.header.track_persistent_id,
                        n.subrecord_kind,
                        b45,
                        n.date_released.raw(),
                        *n.unk_116_cloud_timestamp,
                        t.header.cloud_status,
                        artist,
                        title
                    ));
                }
            }
        }
    }
    println!("--- 8.1 TrackNumerics[45] vs (sub==3 || drel!=0 || u116!=0) on 2026-11-19 ---");
    for ((sub, b45, has_rel, has_u116), cnt) in &b45_vs_formula {
        println!("  sub={sub} b45={b45} drel={has_rel} u116={has_u116} -> {cnt}");
    }
    println!("  Formula mismatches ({}):", b45_mismatches.len());
    for m in &b45_mismatches {
        println!("    {m}");
    }

    // 8.1b Also check 2025 (pre-u116) for sub=1 b45 vs drel!=0
    let mdb_2025 =
        corpus_dir.join("library-data/2025/Apple Music Library.musiclibrary/Library.musicdb");
    if mdb_2025.exists() {
        let raw_2025 = load_raw(&mdb_2025)?;
        let mut c25: BTreeMap<(u32, u8, bool, bool), usize> = BTreeMap::new();
        for t in tracks_of(&raw_2025) {
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let b45 = n.unk_42_54.get(3).copied().unwrap_or(0);
                    *c25.entry((
                        n.subrecord_kind,
                        b45,
                        n.date_released.raw() != 0,
                        *n.unk_116_cloud_timestamp != 0,
                    ))
                    .or_default() += 1;
                }
            }
        }
        println!("--- 8.1b TrackNumerics[45] on 2025 (v1.5.4.96, pre-u116) ---");
        for ((sub, b45, has_rel, has_u116), cnt) in &c25 {
            println!("  sub={sub} b45={b45} drel={has_rel} u116={has_u116} -> {cnt}");
        }
    }

    // 8.1c Check Monterey itma[54] == 1 vs artwork_cache_state/has_embedded_artwork
    let mdb_mont = corpus_dir.join("libraries-music-samples/Monterey/SmallerLibrary.musicdb");
    if mdb_mont.exists() {
        let raw_mont = load_raw(&mdb_mont)?;
        let mut cm: BTreeMap<(u8, u8, u8, i16, bool), usize> = BTreeMap::new();
        for t in tracks_of(&raw_mont) {
            let (b53, b54) = (
                t.header.has_extended_text_metadata,
                t.header.has_asset_info_plist,
            );
            let n0 = t.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) => Some(n),
                _ => None,
            });
            let emb = n0.map_or(0, |n| n.has_embedded_artwork);
            let art_sz = n0.is_some_and(|n| n.artwork_total_size_bytes > 0);
            *cm.entry((b53, b54, t.header.artwork_cache_state, emb, art_sz))
                .or_default() += 1;
        }
        println!(
            "--- 8.1c Monterey itma[53,54] vs artwork_cache_state/has_embedded_artwork/art_sz ---"
        );
        for ((b53, b54, acs, emb, asz), cnt) in &cm {
            println!(
                "  b53={b53} b54={b54} art_cache_state={acs} emb={emb} art_sz>0={asz} -> {cnt}"
            );
        }
    }

    // 8.2 Target tracks detail for b45 & b48
    println!(
        "\n--- 8.2 Target tracks detail (Beatles, AC/DC, Camel, Ringing Hollow, Borboletas) ---"
    );
    for t in tracks_of(&raw_1119) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let album = boma_str(&t.bomas, 0x0003).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        let hit = (title == "Back in the U.S.S.R." && album == "The Beatles")
            || (title == "It Won't Be Long" && album == "With The Beatles")
            || (artist == "AC/DC" && (title == "Highway to Hell" || title == "Hells Bells"))
            || (artist == "Camel" && album == "Nude" && (title == "City Life" || title == "Nude"))
            || title == "Ringing Hollow"
            || title == "Borboletas";
        if hit {
            println!(
                "  [{:016X}] {artist} - {title:?} ({album:?}) cloud={}",
                t.header.track_persistent_id, t.header.cloud_status
            );
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let b45 = n.unk_42_54.get(3).copied().unwrap_or(0);
                    let b48 = n.unk_42_54.get(6).copied().unwrap_or(0);
                    println!(
                        "    sub={} b45={b45} b48={b48} drel={} ({:?}) dmod={} dadd={} dl_traits=0x{:02X} drm=(u96=0x{:08X}, plat={}, vers=0x{:08X})",
                        n.subrecord_kind,
                        n.date_released.raw(),
                        n.date_released.to_utc().map(|d| d.to_string()),
                        n.date_modified.raw(),
                        n.date_added.raw(),
                        n.downloaded_audio_traits,
                        *n.unk_96_100,
                        n.drm_platform,
                        n.drm_version_restrictions,
                    );
                }
            }
        }
    }

    // 8.3 Corpus-wide scan across all .musicdb files
    println!("\n--- 8.3 Corpus-wide scan across all .musicdb files ---");
    let mut files = Vec::new();
    collect_mdb(corpus_dir, &mut files)?;
    files.sort();
    let mut all_b45_mismatch_files = 0usize;
    let mut all_b45_total = 0usize;
    let mut all_b45_mismatches = 0usize;
    let mut all_b48: BTreeMap<(u8, String), usize> = BTreeMap::new();
    let mut drm_tuples: BTreeMap<(u32, u32, u32), usize> = BTreeMap::new();
    let mut dl_trait_pairs: BTreeMap<(u32, u8), usize> = BTreeMap::new();
    let mut drm_key_blobs: BTreeMap<String, usize> = BTreeMap::new();

    for path in &files {
        let Ok(raw) = load_raw(path) else { continue };
        let rel_path = path
            .strip_prefix(corpus_dir)
            .unwrap_or(path)
            .display()
            .to_string();
        let mut file_mis = 0usize;
        for t in tracks_of(&raw) {
            let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
            let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    all_b45_total += 1;
                    let b45 = n.unk_42_54.get(3).copied().unwrap_or(0);
                    let has_rel = n.date_released.raw() != 0;
                    if (b45 == 1) != has_rel {
                        all_b45_mismatches += 1;
                        file_mis += 1;
                    }
                    let b48 = n.unk_42_54.get(6).copied().unwrap_or(0);
                    if b48 != 0 {
                        *all_b48
                            .entry((b48, format!("{artist} - {title:?}")))
                            .or_default() += 1;
                    }
                    let w0 = *n.unk_96_100;
                    let w1 = n.drm_platform;
                    let w2 = n.drm_version_restrictions;
                    if w0 != 0 || w1 != 0 || w2 != 0 {
                        *drm_tuples.entry((w0, w1, w2)).or_default() += 1;
                    }
                    let dl_traits = n.downloaded_audio_traits;
                    if dl_traits != 0 {
                        *dl_trait_pairs
                            .entry((dl_traits, t.header.audio_traits))
                            .or_default() += 1;
                    }
                    let raw216 = *n.unk_216_248;
                    if raw216.iter().any(|&x| x != 0) {
                        *drm_key_blobs.entry(hex::encode(raw216)).or_default() += 1;
                    }
                }
            }
        }
        if file_mis > 0 {
            all_b45_mismatch_files += 1;
            println!("  b45 != has_date_released in {rel_path}: {file_mis} records");
        }
    }
    println!(
        "  Corpus-wide b45 vs (date_released != 0): {all_b45_mismatches}/{all_b45_total} mismatches across {all_b45_mismatch_files} files"
    );
    println!("  Corpus-wide b48 != 0 (distinct byte48, track):");
    for ((b48, trk), cnt) in &all_b48 {
        println!("    b48={b48} {trk} (across {cnt} snapshots)");
    }
    println!("  Corpus-wide DRM fields != 0 (w96, drm_platform, drm_version_restrictions):");
    for ((w0, w1, w2), cnt) in &drm_tuples {
        println!("    w96=0x{w0:08X} plat=0x{w1:08X} vers=0x{w2:08X} -> {cnt}");
    }
    println!("  Corpus-wide downloaded_audio_traits != 0 (downloaded, itma.audio_traits):");
    for ((dl_t, at), cnt) in &dl_trait_pairs {
        println!("    downloaded={dl_t} (0x{dl_t:02X}) itma.audio_traits=0x{at:02X} -> {cnt}");
    }
    println!("  Corpus-wide unk_216_248 != 0:");
    for (h, cnt) in &drm_key_blobs {
        println!("    {h} -> {cnt}");
    }

    Ok(())
}
