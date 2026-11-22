//! Ad-hoc probe: `2026-11-21` local corpus verification checks (`itma[53]`, `TrackPlaysSkips::unk_40`,
//! `itma[33]`, `iama[128..136]`, `plma[182..186]`, `ipfa[44..52]`, `lpma[192]`, `SmartPlaylistOptions[16]`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::{AppleTimestamp, BomaPayload, TrackItem};

use super::helpers::{
    albums_of, boma_str, fmt_apple_ts, load_raw, playlists_of, plma_of, tracks_of,
};

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc diagnostic covering 7 local corpus checks"
)]
pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    println!(
        "=== 2026-11-21 LOCAL CORPUS CHECKS (itma[53], TrackPlaysSkips::unk_40, itma[33], iama[128..136], plma[182..186], ipfa[44..52], lpma[192], SmartPlaylistOptions[16]) ==="
    );

    let vid_p = corpus_dir.join(
        "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
    );
    let raw_vid = load_raw(&vid_p)?;

    // 10.1 itma[53] across 00_baseline -> 01_working_man_mutations -> 02_mystery_outliers + distribution
    println!("--- 10.1 itma[53] (unk_53_55[0]) in 00_baseline -> 01 -> 02 & 2026-11-21-video ---");
    let s00 = load_raw(
        &corpus_dir.join("snapshots/2026-11-07-initial-bitflags/00_baseline/Library.musicdb"),
    )?;
    let s01 = load_raw(
        &corpus_dir
            .join("snapshots/2026-11-07-initial-bitflags/01_working_man_mutations/Library.musicdb"),
    )?;
    let s02 = load_raw(
        &corpus_dir
            .join("snapshots/2026-11-07-initial-bitflags/02_mystery_outliers/Library.musicdb"),
    )?;

    let map00: BTreeMap<u64, &TrackItem> = tracks_of(&s00)
        .iter()
        .map(|t| (t.header.track_persistent_id, t))
        .collect();
    let map01: BTreeMap<u64, &TrackItem> = tracks_of(&s01)
        .iter()
        .map(|t| (t.header.track_persistent_id, t))
        .collect();

    println!(
        "  Diff 00_baseline -> 01_working_man_mutations (any unk_53_55 change OR Working Man album):"
    );
    for t in tracks_of(&s01) {
        let pid = t.header.track_persistent_id;
        let album = boma_str(&t.bomas, 0x0003).unwrap_or("");
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        if let Some(old) = map00.get(&pid)
            && (old.header.unk_53_55 != t.header.unk_53_55 || album == "Working Man")
        {
            let [o53, o54] = *old.header.unk_53_55;
            let [n53, n54] = *t.header.unk_53_55;
            println!(
                "    [{pid:016X}] {artist} - {title:?} (alb={album:?}) | off51..55: [{},{},{o53},{o54}] -> [{},{},{n53},{n54}] | cloud={} play_only={} wm={}/{}",
                old.header.checkbox_show_composer_in_all_views,
                old.header.checkbox_use_work_and_movement,
                t.header.checkbox_show_composer_in_all_views,
                t.header.checkbox_use_work_and_movement,
                t.header.cloud_status,
                t.header.playlist_only,
                boma_str(&t.bomas, 0x003F).is_some(),
                boma_str(&t.bomas, 0x0040).is_some(),
            );
        }
    }

    println!(
        "  Diff 01_working_man_mutations -> 02_mystery_outliers (any unk_53_55 change or header change):"
    );
    for t in tracks_of(&s02) {
        let pid = t.header.track_persistent_id;
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        if let Some(old) = map01.get(&pid)
            && (old.header.unk_53_55 != t.header.unk_53_55
                || old.header.checkbox_album_is_compilation
                    != t.header.checkbox_album_is_compilation
                || old.header.checkbox_use_work_and_movement
                    != t.header.checkbox_use_work_and_movement
                || old.header.suggestion_flag != t.header.suggestion_flag)
        {
            let [o53, o54] = *old.header.unk_53_55;
            let [n53, n54] = *t.header.unk_53_55;
            println!(
                "    [{pid:016X}] {artist} - {title:?} | off51..55: [{},{},{o53},{o54}] -> [{},{},{n53},{n54}] | comp:{}->{} sugg:{}->{}",
                old.header.checkbox_show_composer_in_all_views,
                old.header.checkbox_use_work_and_movement,
                t.header.checkbox_show_composer_in_all_views,
                t.header.checkbox_use_work_and_movement,
                old.header.checkbox_album_is_compilation,
                t.header.checkbox_album_is_compilation,
                old.header.suggestion_flag,
                t.header.suggestion_flag,
            );
        }
    }

    let mut b53_dist: BTreeMap<(u8, u8, bool, bool, bool), usize> = BTreeMap::new();
    let mut b53_examples_local: Vec<String> = Vec::new();
    let mut b53_examples_cat0: Vec<String> = Vec::new();
    for t in tracks_of(&raw_vid) {
        let [b53, _] = *t.header.unk_53_55;
        let has_copyright = boma_str(&t.bomas, 0x002E).is_some();
        let has_work = boma_str(&t.bomas, 0x003F).is_some();
        let has_sort_title = boma_str(&t.bomas, 0x001E).is_some();
        *b53_dist
            .entry((
                t.header.cloud_status,
                b53,
                has_copyright,
                has_work,
                has_sort_title,
            ))
            .or_default() += 1;
        if b53 == 1 && matches!(t.header.cloud_status, 0 | 2 | 3) && b53_examples_local.len() < 15 {
            b53_examples_local.push(format!(
                "[{:016X}] {} - {:?} (cloud={} copy={} work={} mov={}/{} b51={} b52={})",
                t.header.track_persistent_id,
                boma_str(&t.bomas, 0x0004).unwrap_or(""),
                boma_str(&t.bomas, 0x0002).unwrap_or(""),
                t.header.cloud_status,
                has_copyright,
                has_work,
                t.header.movement_number,
                t.header.movement_total,
                t.header.checkbox_show_composer_in_all_views,
                t.header.checkbox_use_work_and_movement,
            ));
        }
        if b53 == 0 && matches!(t.header.cloud_status, 8 | 9) && b53_examples_cat0.len() < 15 {
            b53_examples_cat0.push(format!(
                "[{:016X}] {} - {:?} (cloud={} copy={} work={} dl={} pl_only={})",
                t.header.track_persistent_id,
                boma_str(&t.bomas, 0x0004).unwrap_or(""),
                boma_str(&t.bomas, 0x0002).unwrap_or(""),
                t.header.cloud_status,
                has_copyright,
                has_work,
                t.header.downloaded_status,
                t.header.playlist_only,
            ));
        }
    }
    println!(
        "  2026-11-21-video itma[53] breakdown (cloud, b53, has_copyright, has_work, has_sort_title):"
    );
    for ((cs, b53, hc, hw, hs), cnt) in &b53_dist {
        println!("    cloud={cs} b53={b53} copy={hc:<5} work={hw:<5} sort_t={hs:<5} -> {cnt}");
    }
    println!("  Local/Matched/Uploaded tracks with itma[53] == 1 (first 15):");
    for s in &b53_examples_local {
        println!("    {s}");
    }
    println!("  Catalog (cloud=8/9) tracks with itma[53] == 0 (first 15):");
    for s in &b53_examples_cat0 {
        println!("    {s}");
    }

    // 10.2 TrackPlaysSkips unk_24 / unk_40 / unk_44_52
    println!("\n--- 10.2 TrackPlaysSkips unk_24 / unk_40 / unk_44_52 ---");
    for t in tracks_of(&raw_vid) {
        for b in &t.bomas {
            if let BomaPayload::TrackPlaysSkips(p) = &b.payload
                && (*p.unk_24 != 0 || *p.unk_40 != 0 || *p.unk_44_52 != [0u8; 8])
            {
                println!(
                    "  [{:016X}] {} - {:?} | plays={}/{} last_play={} first_play={} | skips={}/{} last_skip={} | u24={} u40={} u44_52={} | rem_pos={} ever={} bounds={} start={} stop={} cloud={} dl={}",
                    t.header.track_persistent_id,
                    boma_str(&t.bomas, 0x0004).unwrap_or(""),
                    boma_str(&t.bomas, 0x0002).unwrap_or(""),
                    p.play_count,
                    p.true_play_count,
                    fmt_apple_ts(p.date_last_played.raw()),
                    fmt_apple_ts(p.date_first_played_utc.raw()),
                    p.skip_count,
                    p.true_skip_count,
                    fmt_apple_ts(p.date_last_skipped.raw()),
                    *p.unk_24,
                    *p.unk_40,
                    hex::encode(*p.unk_44_52),
                    t.header.checkbox_remember_playback_position,
                    t.header.has_ever_been_played,
                    t.header.has_playback_or_bounds_activity,
                    t.header.playback_start_ms,
                    t.header.playback_stop_ms,
                    t.header.cloud_status,
                    t.header.downloaded_status,
                );
            }
        }
    }

    // 10.3 itma[32..38] outliers in 2026-11-21-video
    println!(
        "\n--- 10.3 itma[32..38] (unk_32, is_cloud_synced, unk_34_38) outliers in 2026-11-21-video ---"
    );
    for t in tracks_of(&raw_vid) {
        if *t.header.unk_32 != 0
            || t.header.is_cloud_synced != 1
            || *t.header.unk_34_38 != [0, 1, 0, 0]
        {
            let p_email = boma_str(&t.bomas, 0x003B).is_some();
            let p_name = boma_str(&t.bomas, 0x003C).is_some();
            let sub1 = t.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) if n.subrecord_kind == 1 => Some(n),
                _ => None,
            });
            let sub3 = t.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) if n.subrecord_kind == 3 => Some(n),
                _ => None,
            });
            println!(
                "  [{:016X}] {} - {:?} (alb={:?}) | u32={} synced={} u34_38={:02X?} cloud={} dl={} sync_st={} pl_only={} mk={} p_email={p_email} p_name={p_name} | sub1_fmt={:?} sub1_drm={:?} sub3_dpurch={:?}",
                t.header.track_persistent_id,
                boma_str(&t.bomas, 0x0004).unwrap_or(""),
                boma_str(&t.bomas, 0x0002).unwrap_or(""),
                boma_str(&t.bomas, 0x0003).unwrap_or(""),
                *t.header.unk_32,
                t.header.is_cloud_synced,
                *t.header.unk_34_38,
                t.header.cloud_status,
                t.header.downloaded_status,
                t.header.cloud_sync_state,
                t.header.playlist_only,
                t.header.media_kind,
                sub1.map(|n| n.container_format_kind),
                sub1.map(|n| format!("0x{:08X}", n.drm_version_restrictions)),
                sub3.map(|n| fmt_apple_ts(n.date_purchased.raw())),
            );
        }
    }

    // 10.4 iama[128..136] non-zero albums
    println!("\n--- 10.4 iama[128..136] (unk_128_136) non-zero albums in 2026-11-21-video ---");
    let mut album_tracks: BTreeMap<u64, Vec<&TrackItem>> = BTreeMap::new();
    for t in tracks_of(&raw_vid) {
        album_tracks
            .entry(t.header.album_persistent_id)
            .or_default()
            .push(t);
    }
    for a in albums_of(&raw_vid) {
        if let Some(u) = &a.header.unk_128_136
            && **u != [0u8; 8]
        {
            let [b0, b1, b2, b3, b4, b5, b6, b7] = **u;
            let w0 = u32::from_le_bytes([b0, b1, b2, b3]);
            let w1 = u32::from_le_bytes([b4, b5, b6, b7]);
            let title = boma_str(&a.bomas, 0x012C).unwrap_or("");
            let artist = boma_str(&a.bomas, 0x012D).unwrap_or("");
            let trks = album_tracks
                .get(&a.header.album_persistent_id)
                .map_or(0, Vec::len);
            let first_dadd = album_tracks
                .get(&a.header.album_persistent_id)
                .and_then(|v| v.first())
                .and_then(|t| {
                    t.bomas.iter().find_map(|b| match &b.payload {
                        BomaPayload::TrackNumerics(n) if n.date_added.raw() != 0 => {
                            Some(fmt_apple_ts(n.date_added.raw()))
                        }
                        _ => None,
                    })
                })
                .unwrap_or_else(|| "none".to_string());
            println!(
                "  [{:016X}] {artist} - {title:?} | u128_w0=0x{w0:08X} ({}) u128_w1=0x{w1:08X} | drel={} dsync={} dplay={} cat_type={:?} pl_only={} comp={} store_id={:?} tracks={trks} first_dadd={first_dadd}",
                a.header.album_persistent_id,
                fmt_apple_ts(w0),
                a.header
                    .date_released
                    .map_or("None".to_string(), |d| fmt_apple_ts(d.raw())),
                a.header
                    .date_cloud_synced
                    .map_or("None".to_string(), |d| fmt_apple_ts(d.raw())),
                fmt_apple_ts(a.header.date_last_played.raw()),
                a.header.catalog_album_type,
                a.header.playlist_only,
                a.header.is_compilation,
                a.header.apple_music_store_album_id,
            );
        }
    }

    // 10.5 plma[160..] across chronological snapshots & macOS libraries
    println!("\n--- 10.5 plma[160..] across corpus snapshots ---");
    let plma_targets = [
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/2025/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/Previous Libraries/Apple Music Library [2025-09-28 20.36.49].musiclibrary/Library.musicdb",
        "library-data/Previous Libraries/Apple Music Library [2026-10-28 22.33.41].musiclibrary/Library.musicdb",
        "snapshots/2026-11-07-initial-bitflags/00_baseline/Library.musicdb",
        "snapshots/2026-11-07-initial-bitflags/05_settings_favoriting/Library.musicdb",
        "snapshots/2026-11-07-initial-bitflags/06_noop_reopen/Library.musicdb",
        "snapshots/2026-11-14-offline-delete/offline-delete/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-21-city-life-and-video/baseline/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-21-city-life-and-video/mutated/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
        "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
    ];
    for rel in plma_targets {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let Ok(raw) = load_raw(&p) else { continue };
        if let Some(plma) = plma_of(&raw) {
            let lease_ts = plma.date_cloud_lease_expires.raw();
            let local_lease = plma.date_local_lease_expires.map(AppleTimestamp::raw);
            let delta = local_lease.map(|w| i64::from(w) - i64::from(lease_ts));
            let u160_hex = plma
                .unk_160_182
                .as_ref()
                .map_or_else(String::new, |u| hex::encode(**u));
            println!(
                "  {rel:<88} | hsz={} sync={} mod={} lease={} (0x{lease_ts:08X}) | local_lease(off182)={:?} (delta={:?}) | u160_182={u160_hex} tail={}",
                plma.header_size,
                fmt_apple_ts(plma.date_cloud_synced.raw()),
                fmt_apple_ts(plma.date_last_modified.raw()),
                fmt_apple_ts(lease_ts),
                local_lease.map(|w| format!("0x{w:08X} ({})", fmt_apple_ts(w))),
                delta,
                hex::encode(&*plma.unk_tail),
            );
        }
    }

    // 10.6 ipfa[44..52] in format <= 21.1
    println!("\n--- 10.6 ipfa[44..52] (item_persistent_id_2) in format <= 21.1 ---");
    for rel in [
        "library-data/Previous Libraries/Apple Music Library [2025-09-28 20.36.49].musiclibrary/Library.musicdb",
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
    ] {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let Ok(raw) = load_raw(&p) else { continue };
        let mut total_ipfa = 0usize;
        let mut nonzero_id2 = 0usize;
        let mut eq_self = 0usize;
        let mut eq_next = 0usize;
        let mut eq_prev = 0usize;
        let mut is_last = 0usize;
        let mut is_first = 0usize;
        let mut other_samples: Vec<String> = Vec::new();
        let mut nonzero_playlists: usize = 0;
        let mut total_nonempty_playlists: usize = 0;

        for pl in playlists_of(&raw) {
            let items: Vec<_> = pl
                .bomas
                .iter()
                .filter_map(|b| match &b.payload {
                    BomaPayload::PlaylistItem(it) => Some(it),
                    _ => None,
                })
                .collect();
            if !items.is_empty() {
                total_nonempty_playlists += 1;
            }
            let mut pl_has_nz = false;
            for (idx, it) in items.iter().enumerate() {
                total_ipfa += 1;
                if it.item_persistent_id_2 != 0 {
                    pl_has_nz = true;
                    nonzero_id2 += 1;
                    let m_self = it.item_persistent_id_2 == it.item_persistent_id;
                    let m_next = items
                        .get(idx + 1)
                        .is_some_and(|n| n.item_persistent_id == it.item_persistent_id_2);
                    let m_prev = idx
                        .checked_sub(1)
                        .and_then(|i| items.get(i))
                        .is_some_and(|pr| pr.item_persistent_id == it.item_persistent_id_2);
                    if m_self {
                        eq_self += 1;
                    }
                    if m_next {
                        eq_next += 1;
                    }
                    if m_prev {
                        eq_prev += 1;
                    }
                    if idx + 1 == items.len() {
                        is_last += 1;
                    }
                    if idx == 0 {
                        is_first += 1;
                    }
                    if other_samples.len() < 8 {
                        let pl_name = boma_str(&pl.bomas, 0x00C8).unwrap_or("");
                        other_samples.push(format!(
                            "pl={pl_name:?} idx={idx}/{} id={:016X} id2={:016X} self={m_self} prev={m_prev} next={m_next}",
                            items.len(),
                            it.item_persistent_id,
                            it.item_persistent_id_2
                        ));
                    }
                }
            }
            if pl_has_nz {
                nonzero_playlists += 1;
            }
        }
        println!(
            "  {rel}: total_ipfa={total_ipfa} nonzero_id2={nonzero_id2} (in {nonzero_playlists}/{total_nonempty_playlists} playlists) | eq_self={eq_self} eq_prev={eq_prev} eq_next={eq_next} is_first={is_first} is_last={is_last}"
        );
        for s in &other_samples {
            println!("    {s}");
        }
    }

    // 10.7 lpma[192] & SmartPlaylistOptions[14,16]
    println!("\n--- 10.7 lpma[192] & SmartPlaylistOptions[14,16] in 2026-11-21-video ---");
    println!("  Playlists with lpma.unk_192 != 0 (or unk_46_smart_flag != 0):");
    for pl in playlists_of(&raw_vid) {
        let name = boma_str(&pl.bomas, 0x00C8).unwrap_or("");
        if *pl.header.unk_192 != 0 || *pl.header.unk_46_smart_flag != 0 {
            println!(
                "    [{:016X}] {name:?} | unk_192={} unk_46=0x{:04X} dist={} own={:?} folder={} master={} sub={} shared={} tracks={}",
                pl.header.playlist_persistent_id,
                *pl.header.unk_192,
                *pl.header.unk_46_smart_flag,
                pl.header.distinguished_kind,
                pl.header.ownership_flags,
                pl.header.is_folder,
                pl.header.is_master_playlist,
                pl.header.is_subscribed_playlist,
                pl.header.is_cloud_shared,
                pl.header.track_count,
            );
        }
    }
    println!("  All Smart Playlists (SmartPlaylistOptions):");
    for pl in playlists_of(&raw_vid) {
        let name = boma_str(&pl.bomas, 0x00C8).unwrap_or("");
        let opts = pl.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::SmartPlaylistOptions(o) => Some(o),
            _ => None,
        });
        let slst = pl.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::SmartPlaylistRules(s) => Some(s),
            _ => None,
        });
        if let Some(o) = opts {
            println!(
                "    [{:016X}] {name:<32} | live={} match={} lim_en={} unit={} sel=0x{:02X} cnt={} chk={} neg={} | u14={} u15={} u16={} (0x{:02X}) | rules={} conj={} | dist={} own={:?}",
                pl.header.playlist_persistent_id,
                o.checkbox_live_updating,
                o.checkbox_enable_matching_rules,
                o.checkbox_enable_limit,
                o.limit_unit,
                o.limit_selection_method,
                o.limit_count,
                o.checkbox_match_only_checked_items,
                o.negate_limit_selection_method,
                *o.unk_14_genius_flag,
                *o.unk_15,
                *o.unk_16,
                *o.unk_16,
                slst.map_or(0, |s| s.rule_count),
                slst.map_or(0, |s| s.conjunction),
                pl.header.distinguished_kind,
                pl.header.ownership_flags,
            );
        }
    }

    // 10.8 Follow-up exact checks
    println!("\n--- 10.8 Follow-up exact checks ---");
    for rel in [
        "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
        "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
    ] {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let Ok(raw) = load_raw(&p) else { continue };
        let mut mismatch_b53_1: Vec<String> = Vec::new();
        let mut mismatch_b53_0: Vec<String> = Vec::new();
        let mut total = 0usize;
        for t in tracks_of(&raw) {
            total += 1;
            let [b53, _] = *t.header.unk_53_55;
            let has_ext = t.bomas.iter().any(|b| {
                matches!(
                    b.subtype,
                    0x0012 | 0x002E | 0x003F | 0x0040 | 0x0010 | 0x0013 | 0x0016
                )
            });
            if (b53 == 1) && !has_ext && mismatch_b53_1.len() < 5 {
                let subs: Vec<String> = t
                    .bomas
                    .iter()
                    .map(|b| format!("0x{:04X}", b.subtype))
                    .collect();
                mismatch_b53_1.push(format!(
                    "[{:016X}] {:?} bomas={subs:?}",
                    t.header.track_persistent_id,
                    boma_str(&t.bomas, 0x0002).unwrap_or("")
                ));
            } else if (b53 == 0) && has_ext && mismatch_b53_0.len() < 5 {
                let subs: Vec<String> = t
                    .bomas
                    .iter()
                    .map(|b| format!("0x{:04X}", b.subtype))
                    .collect();
                mismatch_b53_0.push(format!(
                    "[{:016X}] {:?} bomas={subs:?}",
                    t.header.track_persistent_id,
                    boma_str(&t.bomas, 0x0002).unwrap_or("")
                ));
            }
        }
        println!(
            "  itma[53] bijection ({rel}, {total} tracks): b53=1_without_ext={} b53=0_with_ext={}",
            mismatch_b53_1.len(),
            mismatch_b53_0.len()
        );
        for s in &mismatch_b53_1 {
            println!("    b53=1_no_ext: {s}");
        }
        for s in &mismatch_b53_0 {
            println!("    b53=0_with_ext: {s}");
        }
    }

    let raw_mut = load_raw(&corpus_dir.join(
        "snapshots/2026-11-21-city-life-and-video/mutated/Apple Music Library.musiclibrary/Library.musicdb",
    ))?;
    println!("  itma[32..38] on City Life A/B in 2026-11-21-mutated:");
    for t in tracks_of(&raw_mut) {
        if matches!(
            t.header.track_persistent_id,
            0x0CB6_197D_EA26_44FA | 0x7758_8155_0568_70D3
        ) {
            println!(
                "    [{:016X}] {:?} | u32={} synced={} u34_38={:02X?} cloud={} sync_st={} pl_only={}",
                t.header.track_persistent_id,
                boma_str(&t.bomas, 0x0002).unwrap_or(""),
                *t.header.unk_32,
                t.header.is_cloud_synced,
                *t.header.unk_34_38,
                t.header.cloud_status,
                t.header.cloud_sync_state,
                t.header.playlist_only,
            );
        }
    }

    println!("  iama[128..136] non-zero counts across snapshots:");
    for rel in [
        "library-data/Previous Libraries/Apple Music Library [2025-09-28 20.36.49].musiclibrary/Library.musicdb",
        "library-data/Previous Libraries/Apple Music Library [2026-10-28 22.33.41].musiclibrary/Library.musicdb",
        "snapshots/2026-11-07-initial-bitflags/00_baseline/Library.musicdb",
        "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
    ] {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let Ok(raw) = load_raw(&p) else { continue };
        let nz: Vec<_> = albums_of(&raw)
            .iter()
            .filter_map(|a| {
                a.header.unk_128_136.as_ref().and_then(|u| {
                    if **u == [0u8; 8] {
                        None
                    } else {
                        let [b0, b1, b2, b3, ..] = **u;
                        let w0 = u32::from_le_bytes([b0, b1, b2, b3]);
                        Some((
                            boma_str(&a.bomas, 0x012D).unwrap_or("").to_string(),
                            boma_str(&a.bomas, 0x012C).unwrap_or("").to_string(),
                            fmt_apple_ts(w0),
                        ))
                    }
                })
            })
            .collect();
        println!("    {rel}: {} non-zero (first: {:?})", nz.len(), nz.first());
    }

    Ok(())
}
