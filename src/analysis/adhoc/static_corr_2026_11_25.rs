//! Ad-hoc probe: `2026-11-25` static/historical correlation across verified captures
//! (`corpus/library-data/` and `corpus/snapshots/` only — never `libraries-music-samples/`).
//!
//! Targets:
//! 1. `TrackNumerics.unk_16` (`16..20`) outlier breakdown
//! 2. `itma.unk_64_store_flag` (`64..65`) exact predicate check
//! 3. `SmartPlaylistRule.unk_8` (`8..12`) & `SmartPlaylistOptions.unk_14_genius_flag` (`14..15`)
//! 4. `lpma.unk_225_256` (`225..256`) & `lpma.unk_312` (`312..316`) across time
//! 5. `plma` header unknowns, `plma` `0x01F4`/`0x01F6`/`0x01FE`/`0x01FF`/`0x0201`, `hfma` & `LPma` timeline
//! 6. `TrackNumerics` `unk_42_54` & `unk_55_59` per-byte non-zero mask across verified captures

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::{
    BomaPayload, HsmaChild, LibraryMasterSection, RawLibraryFile, SlstSection, SmartRuleChild,
};

use super::helpers::{boma_str, fmt_apple_ts, load_raw, playlists_of, tracks_of};

const VERIFIED_TIMELINE: &[(&str, &str)] = &[
    (
        "2024",
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2025",
        "library-data/2025/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2025-09-28",
        "library-data/Previous Libraries/Apple Music Library [2025-09-28 20.36.49].musiclibrary/Library.musicdb",
    ),
    (
        "2026-10-28",
        "library-data/Previous Libraries/Apple Music Library [2026-10-28 22.33.41].musiclibrary/Library.musicdb",
    ),
    (
        "2026-11-01",
        "library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2026-11-07",
        "library-data/2026-11-07/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "11-07/00_base",
        "snapshots/2026-11-07-initial-bitflags/00_baseline/Library.musicdb",
    ),
    (
        "11-07/05_sett",
        "snapshots/2026-11-07-initial-bitflags/05_settings_favoriting/Library.musicdb",
    ),
    (
        "11-07/06_noop",
        "snapshots/2026-11-07-initial-bitflags/06_noop_reopen/Library.musicdb",
    ),
    (
        "11-14-offdel",
        "snapshots/2026-11-14-offline-delete/offline-delete/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2026-11-19",
        "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "11-19-prefs",
        "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library Preferences.musicdb",
    ),
    (
        "11-21-video",
        "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "11-23/00_base",
        "snapshots/2026-11-23-offline-local-probes/00_baseline/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "11-23/03_vid",
        "snapshots/2026-11-23-offline-local-probes/03_local_video_and_lossless_audio/Apple Music Library.musiclibrary/Library.musicdb",
    ),
];

pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    println!("=== 2026-11-25 STATIC CORRELATION PASS (VERIFIED CAPTURES ONLY) ===");

    let raw_1123 = load_raw(&corpus_dir.join(
        "snapshots/2026-11-23-offline-local-probes/03_local_video_and_lossless_audio/Apple Music Library.musiclibrary/Library.musicdb",
    ))?;
    let raw_1119 = load_raw(
        &corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb"),
    )?;
    let raw_smart = load_raw(
        &corpus_dir
            .join("snapshots/2026-11-07-initial-bitflags/03_smart_playlists/Library.musicdb"),
    )?;

    probe_track_numerics_unk_16(&raw_1123);
    probe_itma_unk_64(&raw_1123);
    probe_smart_rule_unk_8("2026-11-19", &raw_1119);
    probe_smart_rule_unk_8("03_smart_playlists", &raw_smart);
    probe_lpma_225_and_312(corpus_dir)?;
    probe_plma_and_hfma_timeline(corpus_dir)?;
    probe_track_numerics_byte_masks(corpus_dir)?;
    probe_followup_details(corpus_dir, &raw_1123)?;

    Ok(())
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Unk16Key {
    u16_val: u32,
    sub: u32,
    cloud: u8,
    dl: u8,
    pl_only: u8,
    fmt: u16,
    unav: u8,
    has_url: bool,
    has_dadd: bool,
}

fn probe_track_numerics_unk_16(raw: &RawLibraryFile) {
    println!("\n--- 1. TrackNumerics.unk_16 (off 16..20) in 2026-11-23/03_local_video ---");
    let mut dist: BTreeMap<Unk16Key, usize> = BTreeMap::new();
    let mut zero_samples: Vec<String> = Vec::new();
    let mut zero_total = 0usize;

    for t in tracks_of(raw) {
        let has_url = boma_str(&t.bomas, 0x000B).is_some();
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload {
                let u16_val = *n.unk_16;
                let has_dadd = n.date_added.raw() != 0;
                *dist
                    .entry(Unk16Key {
                        u16_val,
                        sub: n.subrecord_kind,
                        cloud: t.header.cloud_status,
                        dl: t.header.downloaded_status,
                        pl_only: t.header.playlist_only,
                        fmt: n.container_format_kind,
                        unav: n.is_cloud_unavailable,
                        has_url,
                        has_dadd,
                    })
                    .or_default() += 1;
                if u16_val == 0 {
                    zero_total += 1;
                    if zero_samples.len() < 12 {
                        zero_samples.push(format!(
                            "sub={} cloud={} dl={} pl_only={} fmt={} unavail={} url={has_url} dadd={} dmod={} drel={} dsync={} size={} dur={} mk=0x{:X} c_sync_st={} [id={:016X}]",
                            n.subrecord_kind,
                            t.header.cloud_status,
                            t.header.downloaded_status,
                            t.header.playlist_only,
                            n.container_format_kind,
                            n.is_cloud_unavailable,
                            fmt_apple_ts(n.date_added.raw()),
                            fmt_apple_ts(n.date_modified.raw()),
                            fmt_apple_ts(n.date_released.raw()),
                            fmt_apple_ts(n.date_cloud_synced.raw()),
                            n.file_size,
                            n.duration_ms,
                            t.header.media_kind,
                            t.header.cloud_sync_state,
                            t.header.track_persistent_id,
                        ));
                    }
                }
            }
        }
    }

    println!("  Total TrackNumerics with unk_16 == 0: {zero_total}");
    println!("  (unk_16, sub, cloud, dl, pl_only, fmt, unavail, has_url, has_dadd) -> count:");
    for (k, cnt) in &dist {
        if k.u16_val != 1 || *cnt > 100 {
            println!(
                "    u16={} sub={} cloud={} dl={} pl_only={} fmt={:>2} unav={} url={:<5} dadd={:<5} -> {cnt}",
                k.u16_val, k.sub, k.cloud, k.dl, k.pl_only, k.fmt, k.unav, k.has_url, k.has_dadd,
            );
        }
    }
    println!("  Sample unk_16 == 0 records (anonymized):");
    for s in &zero_samples {
        println!("    {s}");
    }
}

fn probe_itma_unk_64(raw: &RawLibraryFile) {
    println!("\n--- 2. itma.unk_64_store_flag (off 64..65) in 2026-11-23/03_local_video ---");
    let mut dist: BTreeMap<(u8, u8, u32, bool, bool, bool), usize> = BTreeMap::new();
    let mut mismatches_vs_adam: usize = 0;
    let mut mismatch_samples: Vec<String> = Vec::new();

    for t in tracks_of(raw) {
        let u64_flag = t.header.store_media_kind;
        let adam_id = t
            .bomas
            .iter()
            .find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) if n.store_track_adam_id != 0 => {
                    Some(n.store_track_adam_id)
                }
                _ => None,
            })
            .unwrap_or(0);
        let has_adam = adam_id != 0;
        let has_sub3 = t.bomas.iter().any(|b| {
            matches!(
                &b.payload,
                BomaPayload::TrackNumerics(n) if n.subrecord_kind == 3
            )
        });
        let has_video = t
            .bomas
            .iter()
            .any(|b| matches!(&b.payload, BomaPayload::VideoInfo(_)));
        *dist
            .entry((
                u64_flag,
                t.header.cloud_status,
                t.header.media_kind,
                has_adam,
                has_sub3,
                has_video,
            ))
            .or_default() += 1;

        if (u64_flag != 0) != has_adam {
            mismatches_vs_adam += 1;
            if mismatch_samples.len() < 10 {
                mismatch_samples.push(format!(
                    "u64={u64_flag} cloud={} mk=0x{:X} adam={} sub3={has_sub3} vid={has_video} pl_only={} c_sync_st={} [id={:016X}]",
                    t.header.cloud_status,
                    t.header.media_kind,
                    adam_id,
                    t.header.playlist_only,
                    t.header.cloud_sync_state,
                    t.header.track_persistent_id,
                ));
            }
        }
    }

    for ((u64f, cs, mk, adam, sub3, vid), cnt) in &dist {
        println!(
            "  u64={u64f} cloud={cs} mk=0x{mk:02X} has_adam={adam:<5} sub3={sub3:<5} vid={vid:<5} -> {cnt}"
        );
    }
    println!(
        "  Bijection check (unk_64 != 0) == (store_track_adam_id != 0): {mismatches_vs_adam} mismatches"
    );
    for s in &mismatch_samples {
        println!("    mismatch: {s}");
    }
}

fn walk_slst_unk_8(
    pl_idx: usize,
    depth: usize,
    slst: &SlstSection,
    dist: &mut BTreeMap<(u32, u32, u32, &'static str, usize), usize>,
    nz_samples: &mut Vec<String>,
) {
    for r in &slst.rules {
        let child_kind = match &r.child {
            SmartRuleChild::Nested(_) => "Nested",
            SmartRuleChild::Arguments(_) => "Args",
            SmartRuleChild::StringUtf16Be { .. } => "Utf16Be",
            SmartRuleChild::Raw(_) => "Raw",
            _ => "Other",
        };
        *dist
            .entry((
                r.is_nested_rule_group,
                r.field_id,
                r.comparison_method,
                child_kind,
                depth,
            ))
            .or_default() += 1;
        if r.is_nested_rule_group != 0 && nz_samples.len() < 20 {
            let extra = match &r.child {
                SmartRuleChild::Nested(sub) => {
                    format!(
                        "nested_conj={} nested_rules={}",
                        sub.conjunction, sub.rule_count
                    )
                }
                SmartRuleChild::Arguments(a) => format!(
                    "args=({}, {}, {}, {}, {}, {})",
                    a.arg_0, a.arg_1, a.arg_2, a.arg_3, a.arg_4, a.arg_5
                ),
                SmartRuleChild::StringUtf16Be { .. } => "str".to_string(),
                SmartRuleChild::Raw(b) => format!("raw_len={}", b.len()),
                _ => "other".to_string(),
            };
            nz_samples.push(format!(
                "pl#{pl_idx} depth={depth} field=0x{:02X}({}) op=0x{:08X} unk_8=0x{:08X} kind={child_kind} {extra}",
                r.field_id,
                r.field_name(),
                r.comparison_method,
                r.is_nested_rule_group,
            ));
        }
        if let SmartRuleChild::Nested(sub) = &r.child {
            walk_slst_unk_8(pl_idx, depth + 1, sub, dist, nz_samples);
        }
    }
}

fn probe_smart_rule_unk_8(label: &str, raw: &RawLibraryFile) {
    println!("\n--- 3. SmartPlaylistRule.unk_8 & SmartPlaylistOptions.unk_14 ({label}) ---");
    let mut dist: BTreeMap<(u32, u32, u32, &'static str, usize), usize> = BTreeMap::new();
    let mut nz_samples: Vec<String> = Vec::new();
    let mut u14_nz = Vec::new();

    for (idx, pl) in playlists_of(raw).iter().enumerate() {
        for b in &pl.bomas {
            match &b.payload {
                BomaPayload::SmartPlaylistRules(slst) => {
                    walk_slst_unk_8(idx, 0, slst, &mut dist, &mut nz_samples);
                }
                BomaPayload::SmartPlaylistOptions(o) if o.is_genius != 0 => {
                    u14_nz.push(format!(
                        "pl#{idx} dist={} own={:?} tracks={} u14={} u16=0x{:02X} live={} lim={}",
                        pl.header.distinguished_kind,
                        pl.header.ownership_flags,
                        pl.header.track_count,
                        o.is_genius,
                        *o.unk_16,
                        o.checkbox_live_updating,
                        o.checkbox_enable_limit,
                    ));
                }
                _ => {}
            }
        }
    }

    let mut by_kind: BTreeMap<(&'static str, u32), usize> = BTreeMap::new();
    for ((u8v, _, _, kind, _), cnt) in &dist {
        *by_kind.entry((*kind, *u8v)).or_default() += cnt;
    }
    println!("  Summary by (child_kind, unk_8):");
    for ((kind, u8v), cnt) in &by_kind {
        println!("    kind={kind:<8} unk_8=0x{u8v:08X} -> {cnt}");
    }
    println!("  Non-zero unk_8 rule samples:");
    for s in &nz_samples {
        println!("    {s}");
    }
    println!(
        "  SmartPlaylistOptions.unk_14_genius_flag != 0 ({}):",
        u14_nz.len()
    );
    for s in &u14_nz {
        println!("    {s}");
    }
}

fn decode_lpma_225_256(bytes: &[u8; 31]) -> (bool, [u64; 3]) {
    let head_all_zero = bytes[0..7].iter().all(|&b| b == 0);
    let w0 = u64::from_le_bytes(bytes[7..15].try_into().unwrap_or([0; 8]));
    let w1 = u64::from_le_bytes(bytes[15..23].try_into().unwrap_or([0; 8]));
    let w2 = u64::from_le_bytes(bytes[23..31].try_into().unwrap_or([0; 8]));
    (head_all_zero, [w0, w1, w2])
}

fn probe_lpma_225_and_312(corpus_dir: &Path) -> Result<()> {
    println!("\n--- 4. lpma.unk_225_256 (225..256) & lpma.unk_312 (312..316) across time ---");
    let snaps = [
        (
            "2024",
            "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "2025",
            "library-data/2025/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "2026-11-01",
            "library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "2026-11-19",
            "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "11-21-video",
            "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "11-23/03_vid",
            "snapshots/2026-11-23-offline-local-probes/03_local_video_and_lossless_audio/Apple Music Library.musiclibrary/Library.musicdb",
        ),
    ];

    let mut tracked_225: BTreeMap<u64, Vec<(&str, [u64; 3])>> = BTreeMap::new();
    let mut tracked_312: BTreeMap<u64, Vec<(&str, u32)>> = BTreeMap::new();

    for (label, rel) in snaps {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let raw = load_raw(&p)?;
        let mut head_nonzero = 0usize;
        let mut scope_dist: BTreeMap<(bool, u8, u8, u32), usize> = BTreeMap::new();
        let mut u312_nz = 0usize;

        for pl in playlists_of(&raw) {
            let (head_zero, words) = decode_lpma_225_256(&pl.header.unk_225_256);
            if !head_zero {
                head_nonzero += 1;
            }
            let any_nz = words != [0, 0, 0];
            *scope_dist
                .entry((
                    any_nz,
                    pl.header.is_subscribed_playlist,
                    pl.header.is_cloud_shared,
                    pl.header.cloud_share_mode,
                ))
                .or_default() += 1;
            if any_nz {
                tracked_225
                    .entry(pl.header.playlist_persistent_id)
                    .or_default()
                    .push((label, words));
            }
            let u312 = *pl.header.unk_312;
            if u312 != 0 {
                u312_nz += 1;
            }
            tracked_312
                .entry(pl.header.playlist_persistent_id)
                .or_default()
                .push((label, u312));
        }

        println!("  [{label}] lpma[225..232] non-zero={head_nonzero}, unk_312 non-zero={u312_nz}");
        for ((any_nz, sub, shared, mode), cnt) in &scope_dist {
            println!(
                "    232..256_nz={any_nz:<5} is_sub={sub} is_shared={shared} share_mode={mode} -> {cnt}"
            );
        }
    }

    summarize_lpma_225_history(&tracked_225);
    summarize_lpma_312_history(&tracked_312);
    Ok(())
}

fn summarize_lpma_225_history(tracked_225: &BTreeMap<u64, Vec<(&str, [u64; 3])>>) {
    let mut changing_225 = 0usize;
    let mut static_225 = 0usize;
    let mut sample_225 = Vec::new();
    for (pid, hist) in tracked_225 {
        if let Some((_, first)) = hist.first()
            && hist.len() >= 2
        {
            let changed = hist.iter().any(|(_, w)| w != first);
            if changed {
                changing_225 += 1;
                if sample_225.len() < 6 {
                    sample_225.push(format!("pid={pid:016X}: {hist:?}"));
                }
            } else {
                static_225 += 1;
            }
        }
    }
    println!(
        "  lpma[232..256] across >=2 snapshots: changing={changing_225}, invariant={static_225}"
    );
    for s in &sample_225 {
        println!("    {s}");
    }
}

fn summarize_lpma_312_history(tracked_312: &BTreeMap<u64, Vec<(&str, u32)>>) {
    let mut u312_changing = 0usize;
    let mut u312_nz_static = 0usize;
    let mut sample_312 = Vec::new();
    for (pid, hist) in tracked_312 {
        if let Some((_, first)) = hist.first()
            && hist.iter().any(|(_, v)| *v != 0)
        {
            if hist.iter().any(|(_, v)| v != first) {
                u312_changing += 1;
                if sample_312.len() < 6 {
                    let fmt_hist: Vec<String> =
                        hist.iter().map(|(l, v)| format!("{l}=0x{v:08X}")).collect();
                    sample_312.push(format!("pid={pid:016X}: {}", fmt_hist.join(", ")));
                }
            } else {
                u312_nz_static += 1;
            }
        }
    }
    println!(
        "  lpma.unk_312 (where ever non-zero): changing_across_snaps={u312_changing}, constant_nonzero={u312_nz_static}"
    );
    for s in &sample_312 {
        println!("    {s}");
    }
}

fn plma_section_of(raw: &RawLibraryFile) -> Option<&LibraryMasterSection> {
    raw.sections.iter().find_map(|s| match &s.child {
        HsmaChild::LibraryMaster(p) => Some(p),
        _ => None,
    })
}

fn lpma_padding_hex(raw: &RawLibraryFile) -> String {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::Padding(p) => {
                let first16: Vec<u8> = p.raw_bytes.iter().take(16).copied().collect();
                Some(format!(
                    "len={} head={}",
                    p.raw_bytes.len(),
                    hex::encode(first16)
                ))
            }
            _ => None,
        })
        .unwrap_or_else(|| "none".to_string())
}

fn probe_plma_and_hfma_timeline(corpus_dir: &Path) -> Result<()> {
    println!("\n--- 5. plma header, plma bomas, hfma unknowns & LPma timeline ---");
    for &(label, rel) in VERIFIED_TIMELINE {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let raw = load_raw(&p)?;
        let Some(ps) = plma_section_of(&raw) else {
            continue;
        };
        let h = &ps.header;
        let oh = &raw.outer_header;
        let ih_flags = raw
            .sections
            .iter()
            .find_map(|s| match &s.child {
                HsmaChild::InnerHfma(ih) => Some(*ih.unk_64_flags),
                _ => None,
            })
            .unwrap_or([0; 4]);
        let u160 = h
            .unk_160_182
            .as_ref()
            .map_or_else(|| "-".to_string(), |u| hex::encode(**u));
        println!(
            "  [{label:<13}] ver={:<10} rev={:<5} trk={:<5} alb={:<5} art={:<4} pl={:<3} | hfma(u60={} f={:02X?}/{ih_flags:02X?} u104={} u116={:02X?}) | plma(u54={:02X?} u68=0x{:08X} u72={} u100={} u128={} u136={} u144=0x{:08X} u160={u160}) | LPma({})",
            oh.version_string(),
            h.cloud_database_revision,
            oh.track_count,
            oh.album_count,
            oh.artist_count,
            oh.playlist_count,
            *oh.unk_60,
            *oh.unk_64_flags,
            *oh.unk_104,
            *oh.unk_116,
            *h.unk_54_58,
            *h.unk_68,
            hex::encode(*h.unk_72_84),
            *h.unk_100,
            *h.unk_128,
            *h.unk_136,
            *h.unk_144,
            lpma_padding_hex(&raw),
        );
        for b in &ps.bomas {
            if matches!(b.subtype, 0x01F4 | 0x01F6 | 0x01FE | 0x01FF | 0x0201) {
                let val = match &b.payload {
                    BomaPayload::RawStringUtf8 { text: s, .. }
                    | BomaPayload::RawStringUtf16Le { text: s, .. } => format!("str({s:?})"),
                    BomaPayload::StringSection(s) => format!("bstr({:?})", s.text),
                    BomaPayload::Unknown { raw_bytes: u, .. } => {
                        let prefix: Vec<u8> = u.iter().take(24).copied().collect();
                        format!("unk(len={}, {})", u.len(), hex::encode(prefix))
                    }
                    other => format!("{other:?}"),
                };
                if matches!(
                    label,
                    "2024"
                        | "2025-09-28"
                        | "2026-10-28"
                        | "2026-11-19"
                        | "11-19-prefs"
                        | "11-23/03_vid"
                ) {
                    println!("      plma::0x{:04X}: {val}", b.subtype);
                }
            }
        }
    }
    Ok(())
}

fn probe_track_numerics_byte_masks(corpus_dir: &Path) -> Result<()> {
    println!("\n--- 6. TrackNumerics unk_42_54 & unk_55_59 per-byte distinct values ---");
    let snaps = [
        (
            "2024",
            "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "2026-11-19",
            "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "11-23/03_vid",
            "snapshots/2026-11-23-offline-local-probes/03_local_video_and_lossless_audio/Apple Music Library.musiclibrary/Library.musicdb",
        ),
    ];
    let mut vals_42_54: [BTreeMap<u8, usize>; 12] = std::array::from_fn(|_| BTreeMap::new());
    let mut vals_55_59: [BTreeMap<u8, usize>; 4] = std::array::from_fn(|_| BTreeMap::new());

    for (_, rel) in snaps {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let raw = load_raw(&p)?;
        for t in tracks_of(&raw) {
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    for (slot, &byte) in vals_42_54.iter_mut().zip(n.unk_42_54.iter()) {
                        *slot.entry(byte).or_default() += 1;
                    }
                    for (slot, &byte) in vals_55_59.iter_mut().zip(n.unk_55_59.iter()) {
                        *slot.entry(byte).or_default() += 1;
                    }
                }
            }
        }
    }

    for (i, m) in vals_42_54.iter().enumerate() {
        println!("  TrackNumerics[{}]: {m:?}", 42 + i);
    }
    for (i, m) in vals_55_59.iter().enumerate() {
        println!("  TrackNumerics[{}]: {m:?}", 55 + i);
    }
    Ok(())
}

fn probe_followup_details(corpus_dir: &Path, raw_1123: &RawLibraryFile) -> Result<()> {
    println!(
        "\n--- 7. Follow-up details (unk_16 timeline, 3 unk_64 outliers, plma 0x01FF full 44B, TrackNumerics[47]) ---"
    );
    probe_unk_16_and_b47_timeline(corpus_dir)?;
    probe_unk_64_outliers(raw_1123);
    probe_plma_01ff_bytes(raw_1123);
    Ok(())
}

const UNK_16_TIMELINE_SNAPS: &[(&str, &str)] = &[
    (
        "2024",
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2025",
        "library-data/2025/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2025-09-28",
        "library-data/Previous Libraries/Apple Music Library [2025-09-28 20.36.49].musiclibrary/Library.musicdb",
    ),
    (
        "2026-10-28",
        "library-data/Previous Libraries/Apple Music Library [2026-10-28 22.33.41].musiclibrary/Library.musicdb",
    ),
    (
        "2026-11-01",
        "library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2026-11-07",
        "library-data/2026-11-07/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "2026-11-19",
        "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "11-21-video",
        "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
    ),
    (
        "11-23/03_vid",
        "snapshots/2026-11-23-offline-local-probes/03_local_video_and_lossless_audio/Apple Music Library.musiclibrary/Library.musicdb",
    ),
];

fn probe_unk_16_and_b47_timeline(corpus_dir: &Path) -> Result<()> {
    for &(label, rel) in UNK_16_TIMELINE_SNAPS {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let raw = load_raw(&p)?;
        let mut total = 0usize;
        let mut z_sub1 = 0usize;
        let mut z_sub3 = 0usize;
        let mut b47_zero = 0usize;
        let mut b47_zero_cloud0 = 0usize;
        let mut min_dsync_z = u32::MAX;
        for t in tracks_of(&raw) {
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    total += 1;
                    if *n.unk_16 == 0 {
                        if n.subrecord_kind == 1 {
                            z_sub1 += 1;
                        } else {
                            z_sub3 += 1;
                        }
                        let ds = n.date_cloud_synced.raw();
                        if ds > 86_400 && ds < min_dsync_z {
                            min_dsync_z = ds;
                        }
                    }
                    if n.unk_42_54.get(5) == Some(&0) {
                        b47_zero += 1;
                        if t.header.cloud_status == 0 {
                            b47_zero_cloud0 += 1;
                        }
                    }
                }
            }
        }
        let min_ts = if min_dsync_z == u32::MAX {
            "-".to_string()
        } else {
            fmt_apple_ts(min_dsync_z)
        };
        println!(
            "  [{label:<12}] total={total} unk_16==0: sub1={z_sub1} sub3={z_sub3} (min_dsync={min_ts}) | b47==0: {b47_zero} (cloud0={b47_zero_cloud0})"
        );
    }
    Ok(())
}

fn probe_unk_64_outliers(raw_1123: &RawLibraryFile) {
    println!("  3 itma.unk_64==1 && adam==0 outliers:");
    for t in tracks_of(raw_1123) {
        if matches!(
            t.header.track_persistent_id,
            0x3701_4491_88A1_FD11 | 0x907E_6858_F66B_73F8 | 0x15AF_2537_9F95_13B3
        ) {
            let sub1 = t.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) if n.subrecord_kind == 1 => Some(n),
                _ => None,
            });
            let sub3 = t.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) if n.subrecord_kind == 3 => Some(n),
                _ => None,
            });
            println!(
                "    [{:016X}] cloud={} dl={} sync_st={} itma_matched_id=0x{:X} itma_art_id={} | sub1_alb_id={:?} sub1_art_id={:?} sub1_dpurch={:?} sub1_sf={:?} | sub3_alb_id={:?} sub3_art_id={:?} sub3_dpurch={:?} sub3_sf={:?}",
                t.header.track_persistent_id,
                t.header.cloud_status,
                t.header.downloaded_status,
                t.header.cloud_sync_state,
                t.header.itunes_store_matched_id,
                t.header.apple_music_store_artist_id,
                sub1.map(|n| n.apple_music_store_album_id),
                sub1.map(|n| n.apple_music_store_artist_id),
                sub1.map(|n| fmt_apple_ts(n.date_purchased.raw())),
                sub1.map(|n| n.storefront_id),
                sub3.map(|n| n.apple_music_store_album_id),
                sub3.map(|n| n.apple_music_store_artist_id),
                sub3.map(|n| fmt_apple_ts(n.date_purchased.raw())),
                sub3.map(|n| n.storefront_id),
            );
        }
    }
}

fn probe_plma_01ff_bytes(raw_1123: &RawLibraryFile) {
    if let Some(ps) = plma_section_of(raw_1123) {
        println!(
            "  plma library_persistent_id = 0x{:016X} (LE bytes: {})",
            ps.header.library_persistent_id,
            hex::encode(ps.header.library_persistent_id.to_le_bytes()),
        );
        for b in &ps.bomas {
            if b.subtype == 0x01FF
                && let BomaPayload::Unknown { raw_bytes, .. } = &b.payload
            {
                println!(
                    "  plma::0x01FF full ({} B): {}",
                    raw_bytes.len(),
                    hex::encode(raw_bytes)
                );
                for (i, chunk) in raw_bytes.chunks(4).enumerate() {
                    let off = i * 4;
                    let w = <[u8; 4]>::try_from(chunk).map_or(0, u32::from_le_bytes);
                    println!(
                        "    off {off:>2}..{:>2}: {} (u32={w} / 0x{w:08X})",
                        off + chunk.len(),
                        hex::encode(chunk)
                    );
                }
            }
        }
    }
}
