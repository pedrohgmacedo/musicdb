//! Ad-hoc corpus verification for audit hypotheses across `.musicdb` snapshots and `iTunes Music Library.xml`.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use plist::Value as PlistValue;

use crate::analysis::serialize_le;
use musicdb::sections::{
    AppleTimestamp, BomaPayload, BomaSection, HsmaChild, PlaylistItem, RawLibraryFile, SectionKind,
    SlstSection, SmartRuleChild, TrackItem,
};

#[derive(Debug, Default)]
pub(crate) struct CorpusAuditReport {
    pub(crate) files_scanned: usize,
    pub(crate) file_mirror_rows: Vec<String>,
    pub(crate) encrypted_roundtrip_mirror_note: Option<String>,
    pub(crate) max_slst_depth: usize,
    pub(crate) slst_depth_hist: BTreeMap<usize, usize>,
    pub(crate) slst_arg6_nonzero: BTreeMap<i64, usize>,
    pub(crate) slst_arg7_nonzero: BTreeMap<i64, usize>,
    pub(crate) slst_raw_children: usize,
    pub(crate) ipfa_size_mismatches: Vec<String>,
    pub(crate) item_total_size_mismatches: Vec<String>,
    pub(crate) trailing_bytes_nonzero_files: Vec<String>,
    pub(crate) unknown_hsma_summary: BTreeMap<String, usize>,
    pub(crate) auto_sort_byte0_4_hist: BTreeMap<u8, usize>,
    pub(crate) auto_sort_byte5_hist: BTreeMap<u8, usize>,
    pub(crate) auto_sort_byte5_examples: Vec<String>,
    pub(crate) distinguished_bytes_hist: BTreeMap<[u8; 4], (usize, String)>,
    pub(crate) media_kind_hist: BTreeMap<u32, usize>,
    pub(crate) suggestion_itma: BTreeMap<u16, usize>,
    pub(crate) suggestion_iama: BTreeMap<u16, usize>,
    pub(crate) suggestion_i_ama: BTreeMap<u16, usize>,
    pub(crate) suggestion_lpma: BTreeMap<u16, usize>,
    pub(crate) unknown_bomas: BTreeMap<String, usize>,
    pub(crate) boma_0135_total: usize,
    pub(crate) boma_0135_plist_ok: usize,
    pub(crate) boma_0135_keys: BTreeMap<String, usize>,
    pub(crate) boma_0136_total: usize,
    pub(crate) boma_0136_encodings: BTreeMap<u32, usize>,
    pub(crate) boma_0136_rush_samples: Vec<String>,
    pub(crate) boma_0136_vs_track_genre_notes: Vec<String>,
    pub(crate) video_info_count: usize,
    pub(crate) video_info_samples: Vec<String>,
    pub(crate) single_play_tz_deltas: BTreeMap<i64, usize>,
    pub(crate) prefs_musicdb_comparisons: Vec<String>,
    pub(crate) sidecar_plist_stats: (usize, usize),
    pub(crate) xml_timestamp_notes: Vec<String>,
    pub(crate) tz_pair_notes: Vec<String>,
}

pub(crate) fn run_corpus_audit(
    paths: &[PathBuf],
    xml_path: Option<&Path>,
    tz_pair: Option<(&Path, &Path)>,
) -> Result<CorpusAuditReport> {
    let mut mdb_files = Vec::new();
    for p in paths {
        collect_musicdb_files(p, &mut mdb_files)?;
    }
    mdb_files.sort();
    mdb_files.dedup();

    let mut report = CorpusAuditReport::default();
    let mut latest_baseline_raw: Option<RawLibraryFile> = None;

    for mdb in &mdb_files {
        let disk_bytes = fs::read(mdb)?;
        let raw_bytes =
            musicdb::decrypt_and_decompress_bytes(&disk_bytes, musicdb::DEFAULT_MUSICDB_KEY)?;
        let raw = RawLibraryFile::from_raw_bytes(&raw_bytes)?;
        report.files_scanned += 1;

        inspect_envelope(mdb, disk_bytes.len(), &raw, &mut report);
        inspect_sections(mdb, &raw, &mut report);
        inspect_sibling_prefs_and_sidecars(mdb, &raw, &mut report)?;

        if mdb.to_string_lossy().contains("2026-11-14-baseline")
            && mdb.file_name().and_then(|s| s.to_str()) == Some("Library.musicdb")
        {
            check_reencrypt_mirror(&raw, &mut report)?;
            inspect_album_0136_vs_tracks(&raw, &mut report);
            latest_baseline_raw = Some(raw);
        }
    }

    if let (Some(xml), Some(raw)) = (xml_path, latest_baseline_raw.as_ref()) {
        inspect_xml_timestamps(raw, xml, &mut report)?;
    }
    if let Some((pa, pb)) = tz_pair {
        inspect_tz_pair(pa, pb, &mut report)?;
    }

    Ok(report)
}

fn collect_musicdb_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("musicdb") {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    if !path.is_dir() {
        return Ok(());
    }
    for entry_res in fs::read_dir(path)? {
        let entry = entry_res?;
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let p = entry.path();
        if ft.is_dir() {
            collect_musicdb_files(&p, out)?;
        } else if ft.is_file() && p.extension().and_then(|s| s.to_str()) == Some("musicdb") {
            out.push(p);
        }
    }
    Ok(())
}

fn short_label(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = path.strip_prefix(&cwd) {
            return rel.display().to_string();
        }
        if let Some(parent) = cwd.parent()
            && let Ok(rel) = path.strip_prefix(parent)
        {
            return rel.display().to_string();
        }
    }
    path.display().to_string()
}

fn inspect_envelope(
    path: &Path,
    disk_len: usize,
    raw: &RawLibraryFile,
    report: &mut CorpusAuditReport,
) {
    let h = &raw.outer_header;
    let label = short_label(path);
    if label.starts_with("library-data/")
        || label.starts_with("existing-tools/")
        || label.contains("2026-11-14")
    {
        report.file_mirror_rows.push(format!(
            "{label} | app={} fmt={}.{} type={} tz={} disk={} file_size={} mirror={}",
            h.version_string(),
            h.major_version,
            h.minor_version,
            h.musicdb_file_type,
            h.timezone_offset_seconds,
            disk_len,
            h.file_size,
            h.file_size_mirror
        ));
    }
    if !raw.trailing_bytes.is_empty() {
        report.trailing_bytes_nonzero_files.push(format!(
            "{}: {} bytes (hex={})",
            short_label(path),
            raw.trailing_bytes.len(),
            hex::encode(&raw.trailing_bytes)
        ));
    }
}

fn check_reencrypt_mirror(raw: &RawLibraryFile, report: &mut CorpusAuditReport) -> Result<()> {
    let enc = raw.to_encrypted_bytes()?;
    let fs_slot = u32::from_le_bytes(enc.get(8..12).unwrap_or(&[0; 4]).try_into()?);
    let mir_slot = u32::from_le_bytes(enc.get(128..132).unwrap_or(&[0; 4]).try_into()?);
    report.encrypted_roundtrip_mirror_note = Some(format!(
        "2026-11-14-baseline to_encrypted_bytes(): len={}, header[8..12](file_size)={fs_slot}, header[128..132](file_size_mirror)={mir_slot} (mismatch={})",
        enc.len(),
        fs_slot != mir_slot
    ));
    Ok(())
}

fn check_item_total(
    label: &str,
    kind: &str,
    header_size: u32,
    total_size: u32,
    bomas: &[BomaSection],
    mismatches: &mut Vec<String>,
) {
    let boma_sum: u64 = bomas.iter().map(|b| u64::from(b.total_size)).sum();
    let expected = u64::from(header_size).saturating_add(boma_sum);
    if u64::from(total_size) != expected && mismatches.len() < 20 {
        mismatches.push(format!(
            "{label} {kind}: total_size={total_size} != header_size({header_size}) + bomas({boma_sum}) = {expected}"
        ));
    }
}

fn inspect_sections(path: &Path, raw: &RawLibraryFile, report: &mut CorpusAuditReport) {
    let label = short_label(path);
    let tz = i64::from(raw.outer_header.timezone_offset_seconds);
    for hsma in &raw.sections {
        match &hsma.child {
            HsmaChild::AlbumList(lama) => {
                for a in &lama.albums {
                    check_item_total(
                        &label,
                        "iama",
                        a.header.header_size,
                        a.header.total_size,
                        &a.bomas,
                        &mut report.item_total_size_mismatches,
                    );
                    *report
                        .suggestion_iama
                        .entry(a.header.suggestion_flag)
                        .or_insert(0) += 1;
                    inspect_album_bomas(a, report);
                }
            }
            HsmaChild::ArtistList(l_ama) => {
                for a in &l_ama.artists {
                    check_item_total(
                        &label,
                        "iAma",
                        a.header.header_size,
                        a.header.total_size,
                        &a.bomas,
                        &mut report.item_total_size_mismatches,
                    );
                    *report
                        .suggestion_i_ama
                        .entry(a.header.suggestion_flag)
                        .or_insert(0) += 1;
                }
            }
            HsmaChild::TrackList(ltma) => {
                for t in &ltma.tracks {
                    inspect_track(&label, t, tz, report);
                }
            }
            HsmaChild::PlaylistList(l_pma) => {
                for p in &l_pma.playlists {
                    inspect_playlist(&label, p, report);
                }
            }
            HsmaChild::SocialProfileList(lsma) => {
                for s in &lsma.profiles {
                    check_item_total(
                        &label,
                        "isma",
                        s.header.header_size,
                        s.header.total_size,
                        &s.bomas,
                        &mut report.item_total_size_mismatches,
                    );
                }
            }
            HsmaChild::Unknown {
                kind,
                signature,
                raw_bytes,
            } => record_unknown_hsma(&label, *kind, *signature, raw_bytes.len(), report),
            _ => {}
        }
    }
}

fn record_unknown_hsma(
    label: &str,
    kind: SectionKind,
    signature: [u8; 4],
    len: usize,
    report: &mut CorpusAuditReport,
) {
    let key = if matches!(kind, SectionKind::Unknown(_)) {
        format!(
            "{label}: kind={kind} sig={} ({:?}) len={len}",
            hex::encode(signature),
            String::from_utf8_lossy(&signature),
        )
    } else {
        format!("kind={kind} sig={} len={len}", hex::encode(signature))
    };
    *report.unknown_hsma_summary.entry(key).or_insert(0) += 1;
}

fn inspect_album_bomas(album: &musicdb::sections::AlbumItem, report: &mut CorpusAuditReport) {
    let mut title = None;
    let mut artist = None;
    for b in &album.bomas {
        if b.subtype == 0x012C {
            title = b.as_str();
        } else if b.subtype == 0x012D {
            artist = b.as_str();
        }
    }
    for b in &album.bomas {
        if b.subtype_name() == "unknown_boma" {
            let key = format!("iama::0x{:04X}", b.subtype);
            *report.unknown_bomas.entry(key).or_insert(0) += 1;
        }
        if b.subtype == 0x0135 {
            report.boma_0135_total += 1;
            if let BomaPayload::BinaryPlist { raw_bytes, .. }
            | BomaPayload::Unknown { raw_bytes, .. } = &b.payload
                && let Ok(PlistValue::Dictionary(dict)) =
                    PlistValue::from_reader(std::io::Cursor::new(raw_bytes))
            {
                report.boma_0135_plist_ok += 1;
                for k in dict.keys() {
                    *report.boma_0135_keys.entry(k.clone()).or_insert(0) += 1;
                }
            }
        } else if b.subtype == 0x0136 {
            report.boma_0136_total += 1;
            if let BomaPayload::StringSection(s) = &b.payload {
                *report.boma_0136_encodings.entry(s.encoding).or_insert(0) += 1;
                if artist == Some("Rush") && report.boma_0136_rush_samples.len() < 8 {
                    report.boma_0136_rush_samples.push(format!(
                        "Rush - {:?} (store_id={:?}): 0x0136={:?}",
                        title.unwrap_or("?"),
                        album.header.apple_music_store_album_id,
                        s.text
                    ));
                }
            }
        }
    }
}

type AlbumTrackGenresAndClouds = (
    BTreeMap<u64, std::collections::BTreeSet<String>>,
    BTreeMap<u64, std::collections::BTreeSet<u8>>,
);

fn collect_track_genres_and_clouds_by_album(raw: &RawLibraryFile) -> AlbumTrackGenresAndClouds {
    let mut track_genres_by_album: BTreeMap<u64, std::collections::BTreeSet<String>> =
        BTreeMap::new();
    let mut track_cloud_by_album: BTreeMap<u64, std::collections::BTreeSet<u8>> = BTreeMap::new();
    for hsma in &raw.sections {
        if let HsmaChild::TrackList(ltma) = &hsma.child {
            for t in &ltma.tracks {
                let album_pid = t.header.album_persistent_id;
                if album_pid == 0 {
                    continue;
                }
                let genre = t
                    .bomas
                    .iter()
                    .find(|b| b.subtype == 0x0005)
                    .and_then(BomaSection::as_str);
                if let Some(g) = genre {
                    track_genres_by_album
                        .entry(album_pid)
                        .or_default()
                        .insert(g.to_owned());
                }
                track_cloud_by_album
                    .entry(album_pid)
                    .or_default()
                    .insert(t.header.cloud_status);
            }
        }
    }
    (track_genres_by_album, track_cloud_by_album)
}

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc diagnostic aggregation across iama 0x0136 and track 0x0005"
)]
fn inspect_album_0136_vs_tracks(raw: &RawLibraryFile, report: &mut CorpusAuditReport) {
    let (track_genres_by_album, track_cloud_by_album) =
        collect_track_genres_and_clouds_by_album(raw);

    let (mut total_albums, mut with_0136, mut without_0136) = (0usize, 0usize, 0usize);
    let (mut exact_match, mut differs, mut with_0136_has_cloud, mut without_0136_has_cloud) =
        (0usize, 0usize, 0usize, 0usize);
    let mut rush_rows = Vec::new();
    let mut diff_samples = Vec::new();
    let mut missing_samples = Vec::new();

    for hsma in &raw.sections {
        let HsmaChild::AlbumList(lama) = &hsma.child else {
            continue;
        };
        for a in &lama.albums {
            total_albums += 1;
            let pid = a.header.album_persistent_id;
            let title = a
                .bomas
                .iter()
                .find(|b| b.subtype == 0x012C)
                .and_then(BomaSection::as_str);
            let artist = a
                .bomas
                .iter()
                .find(|b| b.subtype == 0x012D)
                .and_then(BomaSection::as_str);
            let cloud_id = a
                .bomas
                .iter()
                .find(|b| b.subtype == 0x0133)
                .and_then(BomaSection::as_str);
            let val_0136 = a
                .bomas
                .iter()
                .find(|b| b.subtype == 0x0136)
                .and_then(BomaSection::as_str);
            let store_id = a.header.apple_music_store_album_id.unwrap_or(0);
            let has_cloud_or_store = cloud_id.is_some() || store_id != 0;
            let t_genres = track_genres_by_album.get(&pid).cloned().unwrap_or_default();
            let t_clouds = track_cloud_by_album.get(&pid).cloned().unwrap_or_default();

            if let Some(g_album) = val_0136 {
                with_0136 += 1;
                if has_cloud_or_store {
                    with_0136_has_cloud += 1;
                }
                if t_genres.len() == 1 && t_genres.contains(g_album) {
                    exact_match += 1;
                } else {
                    differs += 1;
                    if diff_samples.len() < 12 {
                        diff_samples.push(format!(
                            "DIFF [{pid:016X}] {:?} - {:?} | store={store_id} cloud={cloud_id:?} | iama::0x0136={g_album:?} vs track::0x0005={t_genres:?} (cloud_states={t_clouds:?})",
                            artist.unwrap_or("?"),
                            title.unwrap_or("?"),
                        ));
                    }
                }
            } else {
                without_0136 += 1;
                if has_cloud_or_store {
                    without_0136_has_cloud += 1;
                }
                if missing_samples.len() < 8 {
                    missing_samples.push(format!(
                        "NO_0136 [{pid:016X}] {:?} - {:?} | store={store_id} cloud={cloud_id:?} | track::0x0005={t_genres:?} (cloud_states={t_clouds:?})",
                        artist.unwrap_or("?"),
                        title.unwrap_or("?"),
                    ));
                }
            }

            if artist.is_some_and(|ar| ar.contains("Rush")) {
                rush_rows.push(format!(
                    "RUSH [{pid:016X}] {:?} | store={store_id} cloud={cloud_id:?} | iama::0x0136={val_0136:?} vs track::0x0005={t_genres:?} (cloud_states={t_clouds:?})",
                    title.unwrap_or("?"),
                ));
            }
        }
    }

    let out = &mut report.boma_0136_vs_track_genre_notes;
    out.push(format!(
        "Baseline albums={total_albums}: with_0x0136={with_0136} (has_cloud_or_store={with_0136_has_cloud}), without_0x0136={without_0136} (has_cloud_or_store={without_0136_has_cloud})"
    ));
    out.push(format!(
        "Among with_0x0136 ({with_0136}): exact_match_track_genre={exact_match}, differs_from_track_genre={differs}"
    ));
    out.extend(rush_rows);
    out.extend(diff_samples);
    out.extend(missing_samples);
}

fn inspect_track(label: &str, t: &TrackItem, tz_secs: i64, report: &mut CorpusAuditReport) {
    check_item_total(
        label,
        "itma",
        t.header.header_size,
        t.header.total_size,
        &t.bomas,
        &mut report.item_total_size_mismatches,
    );
    for &b in t.header.auto_sort_flags.get(0..5).unwrap_or(&[]) {
        *report.auto_sort_byte0_4_hist.entry(b).or_insert(0) += 1;
    }
    let b5 = t.header.auto_sort_flags[5];
    *report.auto_sort_byte5_hist.entry(b5).or_insert(0) += 1;
    if b5 != 0 && report.auto_sort_byte5_examples.len() < 10 {
        let subtypes: Vec<String> = t
            .bomas
            .iter()
            .map(|b| format!("0x{:04X}:{}", b.subtype, b.subtype_name()))
            .collect();
        report.auto_sort_byte5_examples.push(format!(
            "{label} pid={:016X} b5=0x{b5:02X} media=0x{:X} bomas={subtypes:?}",
            t.header.track_persistent_id, t.header.media_kind
        ));
    }
    *report
        .media_kind_hist
        .entry(t.header.media_kind)
        .or_insert(0) += 1;
    *report
        .suggestion_itma
        .entry(t.header.suggestion_flag)
        .or_insert(0) += 1;

    for b in &t.bomas {
        if b.subtype_name() == "unknown_boma" {
            *report
                .unknown_bomas
                .entry(format!("itma::0x{:04X}", b.subtype))
                .or_insert(0) += 1;
        }
        match &b.payload {
            BomaPayload::VideoInfo(v) => {
                report.video_info_count += 1;
                if report.video_info_samples.len() < 8 {
                    let fourcc = v.codec_fourcc.to_be_bytes();
                    report.video_info_samples.push(format!(
                        "{label} pid={:016X}: {}x{} codec={:?} fps_milli={}",
                        t.header.track_persistent_id,
                        v.width,
                        v.height,
                        String::from_utf8_lossy(&fourcc),
                        v.frame_rate_milli
                    ));
                }
            }
            BomaPayload::TrackPlaysSkips(ps)
                if ps.play_count == 1
                    && ps.true_play_count == 1
                    && !ps.date_last_played.is_zero()
                    && !ps.date_first_played_utc.is_zero() =>
            {
                let _ = tz_secs;
                let diff = i64::from(ps.date_last_played.raw())
                    - i64::from(ps.date_first_played_utc.raw());
                if diff.abs() <= 86_400 {
                    *report.single_play_tz_deltas.entry(diff).or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }
}

fn inspect_playlist(label: &str, p: &PlaylistItem, report: &mut CorpusAuditReport) {
    check_item_total(
        label,
        "lpma",
        p.header.header_size,
        p.header.total_size,
        &p.bomas,
        &mut report.item_total_size_mismatches,
    );
    *report
        .suggestion_lpma
        .entry(p.header.suggestion_flag)
        .or_insert(0) += 1;

    let name = p
        .bomas
        .iter()
        .find(|b| b.subtype == 0x00C8)
        .and_then(BomaSection::as_str)
        .unwrap_or("<unnamed>");
    let dk = p.header.distinguished_kind.to_le_bytes();
    let dk_bytes = [*p.header.unk_78, dk[0], dk[1], *p.header.unk_81];
    let entry = report
        .distinguished_bytes_hist
        .entry(dk_bytes)
        .or_insert_with(|| (0, name.to_string()));
    entry.0 += 1;

    for b in &p.bomas {
        let payload_len = b.total_size.saturating_sub(b.header_size);
        match &b.payload {
            BomaPayload::PlaylistItem(ipfa) if ipfa.section_size != payload_len => {
                report.ipfa_size_mismatches.push(format!(
                    "{label}: ipfa section_size={} != payload_len={payload_len}",
                    ipfa.section_size
                ));
            }
            BomaPayload::PlaylistItemCompact(ipfa) if ipfa.section_size != payload_len => {
                report.ipfa_size_mismatches.push(format!(
                    "{label}: IPfa section_size={} != payload_len={payload_len}",
                    ipfa.section_size
                ));
            }
            BomaPayload::SmartPlaylistRules(slst) => {
                walk_slst(slst, 1, report);
            }
            _ => {}
        }
    }
}

fn walk_slst(slst: &SlstSection, depth: usize, report: &mut CorpusAuditReport) {
    report.max_slst_depth = report.max_slst_depth.max(depth);
    *report.slst_depth_hist.entry(depth).or_insert(0) += 1;
    for r in &slst.rules {
        match &r.child {
            SmartRuleChild::Nested(sub) => walk_slst(sub, depth + 1, report),
            SmartRuleChild::Arguments(args) => {
                if *args.unk_48_56 != 0 {
                    *report.slst_arg6_nonzero.entry(*args.unk_48_56).or_insert(0) += 1;
                }
                if *args.unk_56_64 != 0 {
                    *report.slst_arg7_nonzero.entry(*args.unk_56_64).or_insert(0) += 1;
                }
            }
            SmartRuleChild::Raw(_) => {
                report.slst_raw_children += 1;
            }
            _ => {}
        }
    }
}

fn inspect_sibling_prefs_and_sidecars(
    mdb_path: &Path,
    raw: &RawLibraryFile,
    report: &mut CorpusAuditReport,
) -> Result<()> {
    if mdb_path.file_name().and_then(|s| s.to_str()) != Some("Library.musicdb") {
        return Ok(());
    }
    let Some(dir) = mdb_path.parent() else {
        return Ok(());
    };
    let prefs_mdb = dir.join("Library Preferences.musicdb");
    if prefs_mdb.is_file() {
        let prefs_raw = RawLibraryFile::open(&prefs_mdb)?;
        compare_library_and_prefs_mdb(dir, raw, &prefs_raw, report);
    }
    for candidate in [
        dir.join("Preferences.plist"),
        dir.join("preferences").join("Songs.plist"),
    ] {
        if candidate.is_file() {
            if PlistValue::from_file(&candidate).is_ok() {
                report.sidecar_plist_stats.0 += 1;
            } else {
                report.sidecar_plist_stats.1 += 1;
            }
        }
    }
    let prefs_sub = dir.join("preferences");
    if prefs_sub.is_dir() {
        for entry_res in fs::read_dir(prefs_sub)? {
            let p = entry_res?.path();
            if p.extension().and_then(|s| s.to_str()) == Some("plist")
                && p.file_name().and_then(|s| s.to_str()) != Some("Songs.plist")
            {
                if PlistValue::from_file(&p).is_ok() {
                    report.sidecar_plist_stats.0 += 1;
                } else {
                    report.sidecar_plist_stats.1 += 1;
                }
            }
        }
    }
    Ok(())
}

fn compare_library_and_prefs_mdb(
    dir: &Path,
    lib_raw: &RawLibraryFile,
    prefs_raw: &RawLibraryFile,
    report: &mut CorpusAuditReport,
) {
    let lib_plma = lib_raw.sections.iter().find_map(|s| match &s.child {
        HsmaChild::LibraryMaster(p) => Some(serialize_le(p)),
        _ => None,
    });
    let prefs_plma = prefs_raw.sections.iter().find_map(|s| match &s.child {
        HsmaChild::LibraryMaster(p) => Some(serialize_le(p)),
        _ => None,
    });
    let lib_trl = lib_raw.sections.iter().find_map(|s| match &s.child {
        HsmaChild::Unknown {
            kind: SectionKind::Trailer,
            raw_bytes,
            ..
        } => Some(raw_bytes.as_slice()),
        _ => None,
    });
    let prefs_trl = prefs_raw.sections.iter().find_map(|s| match &s.child {
        HsmaChild::Unknown {
            kind: SectionKind::Trailer,
            raw_bytes,
            ..
        } => Some(raw_bytes.as_slice()),
        _ => None,
    });
    let plma_eq = lib_plma == prefs_plma;
    let rev_eq = lib_raw.outer_header.revision_hash == prefs_raw.outer_header.revision_hash;
    let trl_eq = lib_trl == prefs_trl;
    if !plma_eq || !rev_eq || report.prefs_musicdb_comparisons.len() < 5 {
        let diff_count = match (&lib_plma, &prefs_plma) {
            (Some(a), Some(b)) => a.iter().zip(b.iter()).filter(|(x, y)| x != y).count(),
            _ => 9999,
        };
        report.prefs_musicdb_comparisons.push(format!(
            "{}: plma_identical={plma_eq} (diff_bytes={diff_count}, lengths={:?}/{:?}), rev_hash_eq={rev_eq}, trailer_eq={trl_eq}",
            short_label(dir),
            lib_plma.as_ref().map(Vec::len),
            prefs_plma.as_ref().map(Vec::len),
        ));
    }
}

fn inspect_xml_timestamps(
    raw: &RawLibraryFile,
    xml_path: &Path,
    report: &mut CorpusAuditReport,
) -> Result<()> {
    let root = PlistValue::from_file(xml_path)?;
    let Some(tracks_dict) = root
        .as_dictionary()
        .and_then(|d| d.get("Tracks"))
        .and_then(PlistValue::as_dictionary)
    else {
        return Ok(());
    };
    let mut mdb_map = std::collections::HashMap::new();
    for hsma in &raw.sections {
        if let HsmaChild::TrackList(ltma) = &hsma.child {
            for t in &ltma.tracks {
                mdb_map.insert(format!("{:016X}", t.header.track_persistent_id), t);
            }
        }
    }
    let mut added_deltas: BTreeMap<i64, usize> = BTreeMap::new();
    let mut mod_deltas: BTreeMap<i64, usize> = BTreeMap::new();
    let mut play_utc_deltas: BTreeMap<i64, usize> = BTreeMap::new();

    for val in tracks_dict.values() {
        let Some(td) = val.as_dictionary() else {
            continue;
        };
        let Some(pid) = td.get("Persistent ID").and_then(PlistValue::as_string) else {
            continue;
        };
        let Some(&itma) = mdb_map.get(&pid.to_uppercase()) else {
            continue;
        };
        record_xml_track_ts_deltas(
            td,
            itma,
            &mut added_deltas,
            &mut mod_deltas,
            &mut play_utc_deltas,
        );
    }

    report.xml_timestamp_notes.push(format!(
        "Date Added (mdb.to_utc - xml_utc_sec) top deltas: {:?}",
        top_n(&added_deltas, 6)
    ));
    report.xml_timestamp_notes.push(format!(
        "Date Modified (mdb.to_utc - xml_utc_sec) top deltas: {:?}",
        top_n(&mod_deltas, 6)
    ));
    report.xml_timestamp_notes.push(format!(
        "Play Date UTC (mdb_last_played.to_utc - xml_play_date_utc_sec) top deltas: {:?}",
        top_n(&play_utc_deltas, 6)
    ));
    Ok(())
}

fn plist_date_unix_secs(v: &PlistValue) -> Option<i64> {
    let xml_str = v.as_date()?.to_xml_format();
    xml_str
        .parse::<jiff::Timestamp>()
        .ok()
        .map(jiff::Timestamp::as_second)
}

fn record_xml_track_ts_deltas(
    td: &plist::Dictionary,
    itma: &TrackItem,
    added_deltas: &mut BTreeMap<i64, usize>,
    mod_deltas: &mut BTreeMap<i64, usize>,
    play_utc_deltas: &mut BTreeMap<i64, usize>,
) {
    let num = itma.bomas.iter().find_map(|b| match &b.payload {
        BomaPayload::TrackNumerics(n) => Some(n.as_ref()),
        _ => None,
    });
    if let (Some(n), Some(xml_added)) = (num, td.get("Date Added").and_then(plist_date_unix_secs))
        && let Some(mdb_added) = n.date_added.to_utc()
    {
        *added_deltas
            .entry(mdb_added.as_second() - xml_added)
            .or_insert(0) += 1;
    }
    if let (Some(n), Some(xml_mod)) = (num, td.get("Date Modified").and_then(plist_date_unix_secs))
        && let Some(mdb_mod) = n.date_modified.to_utc()
    {
        let diff = mdb_mod.as_second() - xml_mod;
        if diff.abs() <= 86_400 {
            *mod_deltas.entry(diff).or_insert(0) += 1;
        }
    }
    let ps = itma.bomas.iter().find_map(|b| match &b.payload {
        BomaPayload::TrackPlaysSkips(p) => Some(p),
        _ => None,
    });
    if let (Some(p), Some(xml_play_utc)) =
        (ps, td.get("Play Date UTC").and_then(plist_date_unix_secs))
        && let Some(mdb_lp) = AppleTimestamp::new(p.date_last_played.raw()).to_utc()
    {
        let diff = mdb_lp.as_second() - xml_play_utc;
        if diff.abs() <= 86_400 {
            *play_utc_deltas.entry(diff).or_insert(0) += 1;
        }
    }
}

fn top_n(map: &BTreeMap<i64, usize>, n: usize) -> Vec<(i64, usize)> {
    let mut v: Vec<(i64, usize)> = map.iter().map(|(&k, &c)| (k, c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

fn record_ts_delta(
    acc: &mut BTreeMap<&'static str, BTreeMap<i64, usize>>,
    field: &'static str,
    a: Option<AppleTimestamp>,
    b: Option<AppleTimestamp>,
) {
    let (Some(ta), Some(tb)) = (a, b) else {
        return;
    };
    if ta.raw() < 86_400 || tb.raw() < 86_400 {
        return;
    }
    let delta = i64::from(tb.raw()) - i64::from(ta.raw());
    if delta.abs() <= 86_400 {
        *acc.entry(field).or_default().entry(delta).or_insert(0) += 1;
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "exhaustive per-field AppleTimestamp comparison across two snapshots"
)]
fn inspect_tz_pair(path_a: &Path, path_b: &Path, report: &mut CorpusAuditReport) -> Result<()> {
    let raw_a = RawLibraryFile::open(path_a)?;
    let raw_b = RawLibraryFile::open(path_b)?;
    let tz_delta = i64::from(raw_b.outer_header.timezone_offset_seconds)
        - i64::from(raw_a.outer_header.timezone_offset_seconds);
    report.tz_pair_notes.push(format!(
        "TZ shift A->B: {} -> {} (expected local-ts delta = {:+})",
        raw_a.outer_header.timezone_offset_seconds,
        raw_b.outer_header.timezone_offset_seconds,
        tz_delta
    ));

    let mut deltas: BTreeMap<&'static str, BTreeMap<i64, usize>> = BTreeMap::new();
    let mut albums_a = std::collections::HashMap::new();
    let mut artists_a = std::collections::HashMap::new();
    let mut tracks_a = std::collections::HashMap::new();
    let mut playlists_a = std::collections::HashMap::new();
    for s in &raw_a.sections {
        match &s.child {
            HsmaChild::AlbumList(l) => {
                for x in &l.albums {
                    albums_a.insert(x.header.album_persistent_id, x);
                }
            }
            HsmaChild::ArtistList(l) => {
                for x in &l.artists {
                    artists_a.insert(x.header.artist_persistent_id, x);
                }
            }
            HsmaChild::TrackList(l) => {
                for x in &l.tracks {
                    tracks_a.insert(x.header.track_persistent_id, x);
                }
            }
            HsmaChild::PlaylistList(l) => {
                for x in &l.playlists {
                    playlists_a.insert(x.header.playlist_persistent_id, x);
                }
            }
            _ => {}
        }
    }
    for s in &raw_b.sections {
        match &s.child {
            HsmaChild::AlbumList(l) => {
                for b in &l.albums {
                    if let Some(a) = albums_a.get(&b.header.album_persistent_id) {
                        record_ts_delta(
                            &mut deltas,
                            "iama.date_suggestion_flag_modified",
                            Some(a.header.date_suggestion_flag_modified),
                            Some(b.header.date_suggestion_flag_modified),
                        );
                        record_ts_delta(
                            &mut deltas,
                            "iama.date_last_played",
                            Some(a.header.date_last_played),
                            Some(b.header.date_last_played),
                        );
                        record_ts_delta(
                            &mut deltas,
                            "iama.date_cloud_synced",
                            a.header.date_cloud_synced,
                            b.header.date_cloud_synced,
                        );
                        record_ts_delta(
                            &mut deltas,
                            "iama.date_released",
                            a.header.date_released,
                            b.header.date_released,
                        );
                    }
                }
            }
            HsmaChild::ArtistList(l) => {
                for b in &l.artists {
                    if let Some(a) = artists_a.get(&b.header.artist_persistent_id) {
                        record_ts_delta(
                            &mut deltas,
                            "iAma.date_suggestion_flag_modified",
                            Some(a.header.date_suggestion_flag_modified),
                            Some(b.header.date_suggestion_flag_modified),
                        );
                        record_ts_delta(
                            &mut deltas,
                            "iAma.date_cloud_synced",
                            a.header.date_cloud_synced,
                            b.header.date_cloud_synced,
                        );
                    }
                }
            }
            HsmaChild::TrackList(l) => {
                for b in &l.tracks {
                    if let Some(a) = tracks_a.get(&b.header.track_persistent_id) {
                        compare_track_ts(&mut deltas, a, b);
                    }
                }
            }
            HsmaChild::PlaylistList(l) => {
                for b in &l.playlists {
                    if let Some(a) = playlists_a.get(&b.header.playlist_persistent_id) {
                        record_ts_delta(
                            &mut deltas,
                            "lpma.date_created",
                            Some(a.header.date_created),
                            Some(b.header.date_created),
                        );
                        record_ts_delta(
                            &mut deltas,
                            "lpma.date_modified",
                            Some(a.header.date_modified),
                            Some(b.header.date_modified),
                        );
                        record_ts_delta(
                            &mut deltas,
                            "lpma.date_last_played",
                            Some(a.header.date_last_played),
                            Some(b.header.date_last_played),
                        );
                        record_ts_delta(
                            &mut deltas,
                            "lpma.date_cloud_published",
                            Some(a.header.date_cloud_published),
                            Some(b.header.date_cloud_published),
                        );
                        record_ts_delta(
                            &mut deltas,
                            "lpma.date_suggestion_flag_modified",
                            a.header.date_suggestion_flag_modified,
                            b.header.date_suggestion_flag_modified,
                        );
                        record_ts_delta(
                            &mut deltas,
                            "lpma.date_cloud_synced_2",
                            a.header.date_cloud_synced_2,
                            b.header.date_cloud_synced_2,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    for (field, hist) in deltas {
        report
            .tz_pair_notes
            .push(format!("  {field:<38}: {:?}", top_n(&hist, 4)));
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "flat timestamp field comparison table across TrackItem and subrecords"
)]
fn compare_track_ts(
    deltas: &mut BTreeMap<&'static str, BTreeMap<i64, usize>>,
    a: &TrackItem,
    b: &TrackItem,
) {
    record_ts_delta(
        deltas,
        "itma.date_suggestion_flag_modified",
        Some(a.header.date_suggestion_flag_modified),
        Some(b.header.date_suggestion_flag_modified),
    );
    record_ts_delta(
        deltas,
        "itma.date_added_most_recent_track",
        Some(a.header.date_added_most_recent_track),
        Some(b.header.date_added_most_recent_track),
    );
    for (na, nb) in a
        .bomas
        .iter()
        .filter_map(|x| match &x.payload {
            BomaPayload::TrackNumerics(n) => Some(n.as_ref()),
            _ => None,
        })
        .zip(b.bomas.iter().filter_map(|x| match &x.payload {
            BomaPayload::TrackNumerics(n) => Some(n.as_ref()),
            _ => None,
        }))
    {
        record_ts_delta(
            deltas,
            "TrackNumerics.date_added",
            Some(na.date_added),
            Some(nb.date_added),
        );
        let mod_label = match (
            na.subrecord_kind,
            na.is_apple_music_catalog,
            a.header.cloud_status,
        ) {
            (1, _, _) => "TrackNumerics[sub=1].date_modified",
            (3, 1, _) => "TrackNumerics[sub=3,cat=1].date_modified",
            (3, 0, 2) => "TrackNumerics[sub=3,matched].date_modified",
            (3, 0, 3) => "TrackNumerics[sub=3,uploaded].date_modified",
            _ => "TrackNumerics[other].date_modified",
        };
        record_ts_delta(
            deltas,
            mod_label,
            Some(na.date_modified),
            Some(nb.date_modified),
        );
        record_ts_delta(
            deltas,
            "TrackNumerics.date_purchased",
            Some(na.date_purchased),
            Some(nb.date_purchased),
        );
        record_ts_delta(
            deltas,
            "TrackNumerics.date_released",
            Some(na.date_released),
            Some(nb.date_released),
        );
        record_ts_delta(
            deltas,
            "TrackNumerics.date_cloud_synced",
            Some(na.date_cloud_synced),
            Some(nb.date_cloud_synced),
        );
    }
    let pa = a.bomas.iter().find_map(|x| match &x.payload {
        BomaPayload::TrackPlaysSkips(p) => Some(p),
        _ => None,
    });
    let pb = b.bomas.iter().find_map(|x| match &x.payload {
        BomaPayload::TrackPlaysSkips(p) => Some(p),
        _ => None,
    });
    if let (Some(pa), Some(pb)) = (pa, pb) {
        record_ts_delta(
            deltas,
            "TrackPlaysSkips.date_last_played",
            Some(pa.date_last_played),
            Some(pb.date_last_played),
        );
        record_ts_delta(
            deltas,
            "TrackPlaysSkips.date_first_played_utc",
            Some(pa.date_first_played_utc),
            Some(pb.date_first_played_utc),
        );
        record_ts_delta(
            deltas,
            "TrackPlaysSkips.date_last_skipped",
            Some(pa.date_last_skipped),
            Some(pb.date_last_skipped),
        );
    }
}

impl fmt::Display for CorpusAuditReport {
    #[expect(
        clippy::too_many_lines,
        reason = "flat report formatter for ad-hoc corpus verification"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "=== Corpus Verification ({} .musicdb files scanned) ===",
            self.files_scanned
        )?;

        writeln!(f, "\n[1] Envelope & Size Mirrors:")?;
        for row in &self.file_mirror_rows {
            writeln!(f, "  {row}")?;
        }
        if let Some(note) = &self.encrypted_roundtrip_mirror_note {
            writeln!(f, "  >> {note}")?;
        }

        writeln!(f, "\n[2] SLst & Section Boundary Invariants:")?;
        writeln!(
            f,
            "  max_slst_depth={} hist={:?}",
            self.max_slst_depth, self.slst_depth_hist
        )?;
        writeln!(
            f,
            "  slst_arg6_nonzero={:?} slst_arg7_nonzero={:?} slst_raw_children={}",
            self.slst_arg6_nonzero, self.slst_arg7_nonzero, self.slst_raw_children
        )?;
        writeln!(
            f,
            "  ipfa/IPfa size mismatches: {} {:?}",
            self.ipfa_size_mismatches.len(),
            self.ipfa_size_mismatches
        )?;
        writeln!(
            f,
            "  item total_size mismatches: {} {:?}",
            self.item_total_size_mismatches.len(),
            self.item_total_size_mismatches
        )?;
        writeln!(
            f,
            "  trailing_bytes non-empty files: {:?}",
            self.trailing_bytes_nonzero_files
        )?;
        writeln!(
            f,
            "  unknown_hsma_sections: {:?}",
            self.unknown_hsma_summary
        )?;

        writeln!(f, "\n[3] Partially-Decoded Arrays & Undecoded Bytes:")?;
        writeln!(f, "  auto_sort_flags[0..5] (hex->count):")?;
        for (&b, &cnt) in &self.auto_sort_byte0_4_hist {
            writeln!(f, "    0x{b:02X}: {cnt}")?;
        }
        writeln!(f, "  auto_sort_flags[5] (offset 71, hex->count):")?;
        for (&b, &cnt) in &self.auto_sort_byte5_hist {
            writeln!(f, "    0x{b:02X}: {cnt}")?;
        }
        for ex in &self.auto_sort_byte5_examples {
            writeln!(f, "    ex: {ex}")?;
        }
        writeln!(f, "  lpma distinguished_kind_bytes (offset 78..82):")?;
        for (bytes, (cnt, sample_name)) in &self.distinguished_bytes_hist {
            writeln!(
                f,
                "    [{:02X}, {:02X}, {:02X}, {:02X}] (b1_b2_u16={}): {cnt}x (e.g. {sample_name:?})",
                bytes[0],
                bytes[1],
                bytes[2],
                bytes[3],
                u16::from_le_bytes([bytes[1], bytes[2]])
            )?;
        }

        writeln!(f, "\n[4] Enum Raw Distributions & Suggestion Flags:")?;
        writeln!(f, "  media_kind (hex->count):")?;
        for (&mk, &cnt) in &self.media_kind_hist {
            writeln!(f, "    0x{mk:08X}: {cnt}")?;
        }
        writeln!(f, "  suggestion_flag (hex->count):")?;
        writeln!(f, "    itma (tracks)   : {:04X?}", self.suggestion_itma)?;
        writeln!(f, "    iama (albums)   : {:04X?}", self.suggestion_iama)?;
        writeln!(f, "    iAma (artists)  : {:04X?}", self.suggestion_i_ama)?;
        writeln!(f, "    lpma (playlists): {:04X?}", self.suggestion_lpma)?;

        writeln!(
            f,
            "\n[5] Unhandled / Unrecognized Boma Subtypes & VideoInfo:"
        )?;
        writeln!(f, "  unknown_boma counts: {:?}", self.unknown_bomas)?;
        writeln!(
            f,
            "  iama::0x0135: total={}, parsed_as_plist={}, keys={:?}",
            self.boma_0135_total, self.boma_0135_plist_ok, self.boma_0135_keys
        )?;
        writeln!(
            f,
            "  iama::0x0136: total={}, encodings={:?}",
            self.boma_0136_total, self.boma_0136_encodings
        )?;
        for s in &self.boma_0136_rush_samples {
            writeln!(f, "    {s}")?;
        }
        for n in &self.boma_0136_vs_track_genre_notes {
            writeln!(f, "    {n}")?;
        }
        writeln!(f, "  VideoInfo (0x0024): count={}", self.video_info_count)?;
        for v in &self.video_info_samples {
            writeln!(f, "    {v}")?;
        }

        writeln!(f, "\n[6] Timestamp Ground Truth (Local vs UTC):")?;
        writeln!(
            f,
            "  Single-play tracks (date_last_played - date_first_played_utc) seconds: {:?}",
            top_n(&self.single_play_tz_deltas, 8)
        )?;
        for note in &self.xml_timestamp_notes {
            writeln!(f, "  {note}")?;
        }
        for note in &self.tz_pair_notes {
            writeln!(f, "  {note}")?;
        }

        writeln!(f, "\n[7] Library Preferences.musicdb & Sidecar .plists:")?;
        writeln!(
            f,
            "  sidecar .plist files: {} ok, {} failed",
            self.sidecar_plist_stats.0, self.sidecar_plist_stats.1
        )?;
        for cmp in &self.prefs_musicdb_comparisons {
            writeln!(f, "  {cmp}")?;
        }
        Ok(())
    }
}
