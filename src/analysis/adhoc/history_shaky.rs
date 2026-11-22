//! Ad-hoc probe: verification of `history.md` Section 5 shaky fields & bitfields.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::{AppleTimestamp, BomaPayload, HsmaChild, TrackItem};

use super::helpers::{
    albums_of, artists_of, boma_str, load_1119, load_raw, playlists_of, socials_of, tracks_of,
};

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc verification suite for history.md"
)]
pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    type WorkMovKey = (u8, u8, u8, bool, bool, bool, bool);
    type NonDlCodecKey = (u8, u8, u8, String, u16, u32, String, String);

    let raw_1119 = load_1119(corpus_dir)?;
    println!("=== HISTORY.MD SECTION 5 SHAKY FIELDS & BITFIELD VERIFICATION ===");

    // 7.0 Bitfield analysis of artwork_cache_state, artwork_source_kind, and artwork.sqlite columns
    println!(
        "\n--- 7.0 BITFIELD ANALYSIS: artwork_cache_state, artwork_source_kind vs artwork.sqlite ---"
    );
    #[cfg(feature = "sqlite")]
    {
        type FullArtRow = (i64, i64, i64, i64, i64, String, i64);
        let art_db =
            corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/artwork.sqlite");
        if art_db.exists() {
            let conn = rusqlite::Connection::open(&art_db)?;
            let mut stmt = conn.prepare(
                "SELECT i.item_id, i.source_kind, i.remote_id, s.aspect, s.location_type, c.size_kind, c.extension, c.status \
                 FROM item_to_artwork i \
                 LEFT JOIN artwork_source s ON s.artwork_id = i.artwork_id \
                 LEFT JOIN cache_items c ON c.artwork_id = i.artwork_id",
            )?;
            let mut sql_map: BTreeMap<u64, FullArtRow> = BTreeMap::new();
            let rows = stmt.query_map([], |r| {
                let item_id: i64 = r.get(0)?;
                Ok((
                    item_id.cast_unsigned(),
                    (
                        r.get::<_, i64>(1).unwrap_or(-1),
                        r.get::<_, i64>(2).unwrap_or(-1),
                        r.get::<_, i64>(3).unwrap_or(-1),
                        r.get::<_, i64>(4).unwrap_or(-1),
                        r.get::<_, i64>(5).unwrap_or(-1),
                        r.get::<_, String>(6).unwrap_or_default(),
                        r.get::<_, i64>(7).unwrap_or(-1),
                    ),
                ))
            })?;
            for r in rows.flatten() {
                sql_map.insert(r.0, r.1);
            }

            let mut bit_dist: BTreeMap<(String, u32, String), usize> = BTreeMap::new();
            for t in tracks_of(&raw_1119) {
                let st = u32::from(t.header.artwork_cache_state);
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&t.header.track_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "itma(src={}, sz={}, fmt={})",
                            t.header.artwork_source_kind,
                            t.header.artwork_size_kind,
                            t.header.artwork_image_format
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for a in artists_of(&raw_1119) {
                let st = u32::from(a.header.artwork_cache_state);
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&a.header.artist_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "iAma(src={}, sz={}, fmt={})",
                            a.header.artwork_source_kind,
                            a.header.artwork_size_kind,
                            a.header.artwork_image_format
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for p in playlists_of(&raw_1119) {
                let st = p.header.artwork_cache_state;
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&p.header.playlist_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "lpma(src={}, sz={}, fmt={}, unk192={})",
                            p.header.artwork_source_kind,
                            p.header.artwork_size_kind,
                            p.header.artwork_image_format,
                            *p.header.unk_192
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for s in socials_of(&raw_1119) {
                let st = u32::from(s.header.artwork_cache_state);
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&s.header.profile_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "isma(src={}, sz={}, fmt={})",
                            s.header.artwork_source_kind,
                            s.header.artwork_size_kind,
                            s.header.artwork_image_format
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for ((sec, st, sql_desc), cnt) in &bit_dist {
                println!("  {sec} | state={st} (0b{st:04b}) | {sql_desc} => {cnt}");
            }
        }
    }

    // 7.1 TrackNumerics[116..120] on catalog tracks + iTunes XML GaplessHeuristicInfo
    println!("\n--- 7.1 TrackNumerics[116..120] ON CATALOG TRACKS & DIFF ACROSS SNAPSHOTS ---");
    let mut cat_116_dist: BTreeMap<(u8, u8, bool, bool), usize> = BTreeMap::new();
    let mut cat_116_samples = Vec::new();
    for t in tracks_of(&raw_1119) {
        if t.header.cloud_status == 8 {
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let v = *n.unk_116_cloud_timestamp;
                    let has_116 = v != 0;
                    let eq_352 = v != 0 && v == t.header.date_cloud_synced.raw();
                    *cat_116_dist
                        .entry((
                            t.header.playlist_only,
                            t.header.downloaded_status,
                            has_116,
                            eq_352,
                        ))
                        .or_insert(0) += 1;
                    if has_116 && cat_116_samples.len() < 8 {
                        cat_116_samples.push(format!(
                            "[{:016X}] {} - {:?} (pl_only={}, dl={}, sub={}, 116={:?}, added={:?}, mod={:?}, sync={:?}, itma352={:?})",
                            t.header.track_persistent_id,
                            boma_str(&t.bomas, 0x0004).unwrap_or(""),
                            boma_str(&t.bomas, 0x0002).unwrap_or(""),
                            t.header.playlist_only,
                            t.header.downloaded_status,
                            n.subrecord_kind,
                            AppleTimestamp::from(v).to_utc(),
                            n.date_added.to_utc(),
                            n.date_modified.to_utc(),
                            n.date_cloud_synced.to_utc(),
                            t.header.date_cloud_synced.to_utc(),
                        ));
                    }
                }
            }
        }
    }
    for ((pl_only, dl, has_116, eq_352), cnt) in &cat_116_dist {
        println!(
            "  cloud=8: playlist_only={pl_only}, downloaded={dl}, has_116={has_116}, eq_itma352={eq_352} => {cnt} subrecords"
        );
    }
    for s in &cat_116_samples {
        println!("    sample: {s}");
    }

    let xml_path = corpus_dir.join("library-data/2026-11-01/iTunes Music Library.xml");
    if xml_path.exists() {
        let xml_text = fs::read_to_string(&xml_path)?;
        let mut gh_vals: BTreeMap<String, usize> = BTreeMap::new();
        for line in xml_text.lines() {
            if let Some(pos) = line.find("<key>GaplessHeuristicInfo</key><integer>") {
                let rest = &line[pos + "<key>GaplessHeuristicInfo</key><integer>".len()..];
                if let Some(end) = rest.find("</integer>") {
                    *gh_vals.entry(rest[..end].to_string()).or_insert(0) += 1;
                }
            }
        }
        println!("  iTunes Music Library.xml <key>GaplessHeuristicInfo</key> values: {gh_vals:?}");
    }

    // 7.2 TrackItemHeader::date_cloud_synced (itma[352..356])
    println!("\n--- 7.2 itma[352..356] (date_cloud_synced) DISTRIBUTION & CORRELATION ---");
    let mut dist_352: BTreeMap<(u8, u8, bool), usize> = BTreeMap::new();
    let mut match_352_vs_other: BTreeMap<&str, usize> = BTreeMap::new();
    for t in tracks_of(&raw_1119) {
        let v352 = t.header.date_cloud_synced.raw();
        *dist_352
            .entry((t.header.cloud_status, t.header.playlist_only, v352 != 0))
            .or_insert(0) += 1;
        if v352 != 0 {
            let mut matched = "other";
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    if v352 == n.date_added.raw() {
                        matched = "eq_date_added";
                        break;
                    } else if v352 == n.date_cloud_synced.raw() {
                        matched = "eq_date_cloud_synced";
                        break;
                    } else if v352 == *n.unk_116_cloud_timestamp {
                        matched = "eq_num_116";
                        break;
                    } else if v352 == n.date_modified.raw() {
                        matched = "eq_date_modified";
                        break;
                    } else if v352.abs_diff(n.date_added.raw()) <= 60 {
                        matched = "within_60s_of_date_added";
                    } else if v352.abs_diff(n.date_cloud_synced.raw()) <= 60 {
                        matched = "within_60s_of_date_cloud_synced";
                    } else if v352.abs_diff(*n.unk_116_cloud_timestamp) <= 60 {
                        matched = "within_60s_of_num_116";
                    }
                }
            }
            *match_352_vs_other.entry(matched).or_insert(0) += 1;
        }
    }
    for ((cs, pl, nz), cnt) in &dist_352 {
        println!("  itma[352..356]: cloud_status={cs}, playlist_only={pl}, non_zero={nz} => {cnt}");
    }
    println!("  itma[352..356] non-zero correlations: {match_352_vs_other:?}");

    // 7.3 itma[51], itma[52], itma[53]
    println!("\n--- 7.3 itma[51], itma[52], itma[53] ACROSS 2026-11-19 ---");
    let mut wm_dist: BTreeMap<WorkMovKey, usize> = BTreeMap::new();
    for t in tracks_of(&raw_1119) {
        let has_work = boma_str(&t.bomas, 0x003F).is_some();
        let has_mov_name = boma_str(&t.bomas, 0x0040).is_some();
        let has_mov_num = t.header.movement_number != 0 || t.header.movement_total != 0;
        let has_composer = boma_str(&t.bomas, 0x000C).is_some();
        let k = (
            t.header.checkbox_show_composer_in_all_views,
            t.header.checkbox_use_work_and_movement,
            t.header.unk_53_55.first().copied().unwrap_or(0),
            has_work,
            has_mov_name,
            has_mov_num,
            has_composer,
        );
        *wm_dist.entry(k).or_insert(0) += 1;
    }
    for (k, cnt) in &wm_dist {
        println!(
            "  (off51={}, off52={}, off53={}, has_work={}, has_mov_name={}, has_mov_num={}, has_composer_0x0C={}) => {cnt}",
            k.0, k.1, k.2, k.3, k.4, k.5, k.6
        );
    }

    // 7.4 itma[66..72] (auto_sort_flags) per-byte breakdown
    println!("\n--- 7.4 itma[66..72] (auto_sort_flags) PER-BYTE BREAKDOWN ---");
    let slot_bomas: [(usize, &str, u32, u32); 6] = [
        (0, "off66(Title?)", 0x0002, 0x001E),
        (1, "off67(Album?)", 0x0003, 0x001F),
        (2, "off68(Artist?)", 0x0004, 0x0020),
        (3, "off69(AlbumArtist?)", 0x001B, 0x0021),
        (4, "off70(Composer?)", 0x000C, 0x0022),
        (5, "off71(Show?)", 0x0018, 0x0053),
    ];
    for (idx, label, pri_sub, sort_sub) in slot_bomas {
        let mut slot_dist: BTreeMap<(u8, u8, bool, bool), usize> = BTreeMap::new();
        for t in tracks_of(&raw_1119) {
            let val = t.header.auto_sort_flags.get(idx).copied().unwrap_or(0);
            let pri = boma_str(&t.bomas, pri_sub).unwrap_or("");
            let has_sort = boma_str(&t.bomas, sort_sub).is_some();
            let starts_article = pri.starts_with("The ")
                || pri.starts_with("the ")
                || pri.starts_with("A ")
                || pri.starts_with("An ");
            *slot_dist
                .entry((t.header.cloud_status, val, has_sort, starts_article))
                .or_insert(0) += 1;
        }
        println!("  Slot {idx} {label}:");
        for ((cs, val, has_sort, art), cnt) in &slot_dist {
            println!(
                "    cloud={cs}, val=0x{val:02X} (0b{val:08b}), has_sort_boma={has_sort}, starts_article={art} => {cnt}"
            );
        }
    }

    // 7.5 Non-downloaded Matched and Uploaded subrecord_kind=3 codec
    println!(
        "\n--- 7.5 NON-DOWNLOADED MATCHED (cloud=2, dl=3) & UPLOADED (cloud=3, dl=3) CODECS ---"
    );
    let mut nondl_dist: BTreeMap<NonDlCodecKey, usize> = BTreeMap::new();
    for t in tracks_of(&raw_1119) {
        if (t.header.cloud_status == 2 || t.header.cloud_status == 3)
            && t.header.downloaded_status != 1
        {
            let kind_str = boma_str(&t.bomas, 0x0006).unwrap_or("").to_string();
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    *nondl_dist
                        .entry((
                            t.header.cloud_status,
                            t.header.downloaded_status,
                            n.asset_container_kind,
                            n.file_type_fourcc_str().unwrap_or_default(),
                            n.container_format_kind,
                            n.cloud_codec_kind,
                            n.codec_fourcc_str().unwrap_or_default(),
                            kind_str.clone(),
                        ))
                        .or_insert(0) += 1;
                }
            }
        }
    }
    for ((cs, dl, o54, f4, fmt, cck, c4, kstr), cnt) in &nondl_dist {
        println!(
            "  cloud={cs} dl={dl}: off54={o54}, file_4cc={f4:?}, fmt={fmt}, cloud_codec={cck}, codec_4cc={c4:?}, kind={kstr:?} => {cnt}"
        );
    }

    // 7.6 Playlist play counts & ipfa[44..52]
    println!(
        "\n--- 7.6 PLAYLIST PLAY COUNTS (lpma[174..186]) & ipfa[44..52] (item_persistent_id_2) ---"
    );
    let mut track_plays_by_id: BTreeMap<u64, u32> = BTreeMap::new();
    for t in tracks_of(&raw_1119) {
        for b in &t.bomas {
            if let BomaPayload::TrackPlaysSkips(ps) = &b.payload {
                track_plays_by_id.insert(t.header.track_persistent_id, ps.play_count);
            }
        }
    }
    let mut nz_lpma_plays = 0usize;
    for p in playlists_of(&raw_1119) {
        if p.header.play_count != 0
            || p.header.true_play_count != 0
            || p.header.date_last_played.raw() != 0
        {
            nz_lpma_plays += 1;
            let name = boma_str(&p.bomas, 0x00C8).unwrap_or("<unnamed>");
            let mut sum_track_plays = 0u64;
            for b in &p.bomas {
                if let BomaPayload::PlaylistItem(ipfa) = &b.payload {
                    sum_track_plays += u64::from(
                        *track_plays_by_id
                            .get(&ipfa.track_persistent_id)
                            .unwrap_or(&0),
                    );
                }
            }
            if nz_lpma_plays <= 25 {
                println!(
                    "  [{:016X}] {:?} (dist_kind={}, tracks={}) | play_cnt={} true_play_cnt={} last_played={:?} | sum_track_plays={sum_track_plays}",
                    p.header.playlist_persistent_id,
                    name,
                    p.header.distinguished_kind,
                    p.header.track_count,
                    p.header.play_count,
                    p.header.true_play_count,
                    p.header.date_last_played.to_utc(),
                );
            }
        }
    }
    println!(
        "  Total playlists with non-zero play_count/date_last_played: {nz_lpma_plays} / {}",
        playlists_of(&raw_1119).len()
    );

    let mdb_1101 =
        corpus_dir.join("library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb");
    if mdb_1101.exists() {
        let raw_1101 = load_raw(&mdb_1101)?;
        let mut ipfa_total = 0usize;
        let mut ipfa_id2_eq_id1 = 0usize;
        let mut ipfa_id2_eq_prev = 0usize;
        let mut ipfa_id2_other_nz = 0usize;
        let mut ipfa_id2_zero = 0usize;
        let mut ipfa_pos_dist: BTreeMap<&str, usize> = BTreeMap::new();
        for p in playlists_of(&raw_1101) {
            let ipfas: Vec<_> = p
                .bomas
                .iter()
                .filter_map(|b| match &b.payload {
                    BomaPayload::PlaylistItem(i) => Some(i),
                    _ => None,
                })
                .collect();
            let len = ipfas.len();
            for (idx, i) in ipfas.iter().enumerate() {
                ipfa_total += 1;
                if i.item_persistent_id_2 == 0 {
                    ipfa_id2_zero += 1;
                } else if i.item_persistent_id_2 == i.item_persistent_id {
                    ipfa_id2_eq_id1 += 1;
                    let pos_kind = if idx == 0 && len == 1 {
                        "only_item"
                    } else if idx == 0 {
                        "first_item"
                    } else if idx + 1 == len {
                        "last_item"
                    } else {
                        "middle_item"
                    };
                    *ipfa_pos_dist.entry(pos_kind).or_insert(0) += 1;
                } else if idx > 0
                    && ipfas
                        .get(idx - 1)
                        .is_some_and(|prev| i.item_persistent_id_2 == prev.item_persistent_id)
                {
                    ipfa_id2_eq_prev += 1;
                } else {
                    ipfa_id2_other_nz += 1;
                }
            }
        }
        println!(
            "  2026-11-01 ipfa[44..52] across {ipfa_total} items: zero={ipfa_id2_zero}, eq_own_id={ipfa_id2_eq_id1} (positions: {ipfa_pos_dist:?}), eq_prev_id={ipfa_id2_eq_prev}, other_nz={ipfa_id2_other_nz}"
        );
    }

    // 7.7 iama.star_rating / star_rating_inheritance & itma.checkbox_part_of_gapless_album
    println!(
        "\n--- 7.7 iama[40..42] (star_rating & star_rating_inheritance) & itma[49] (gapless_album) ---"
    );
    let mut album_rating_dist: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    for a in albums_of(&raw_1119) {
        *album_rating_dist
            .entry((a.header.star_rating, a.header.star_rating_inheritance))
            .or_insert(0) += 1;
    }
    println!("  iama (star_rating, star_rating_inheritance) dist: {album_rating_dist:?}");

    let mut off49_dist: BTreeMap<(u8, u8, u8), usize> = BTreeMap::new();
    for t in tracks_of(&raw_1119) {
        *off49_dist
            .entry((
                t.header.checkbox_part_of_gapless_album,
                t.header.cloud_status,
                t.header.playlist_only,
            ))
            .or_insert(0) += 1;
    }
    println!("  itma[49] (val, cloud_status, playlist_only) dist in 2026-11-19: {off49_dist:?}");
    if mdb_1101.exists() {
        let raw_1101 = load_raw(&mdb_1101)?;
        let mut off49_1101: BTreeMap<(u8, u8, u8), usize> = BTreeMap::new();
        for t in tracks_of(&raw_1101) {
            *off49_1101
                .entry((
                    t.header.checkbox_part_of_gapless_album,
                    t.header.cloud_status,
                    t.header.playlist_only,
                ))
                .or_insert(0) += 1;
        }
        println!(
            "  itma[49] (val, cloud_status, playlist_only) dist in 2026-11-01: {off49_1101:?}"
        );
    }

    // 7.8 VideoInfo, custom_lyrics_hash, and PlmaSub1F6/1FF
    println!("\n--- 7.8 VideoInfo, custom_lyrics_hash, and PlmaSub1F6/PlmaSub1FF ---");
    for t in tracks_of(&raw_1119) {
        if t.header.media_kind != 1 {
            let subs: Vec<_> = t
                .bomas
                .iter()
                .map(|b| format!("0x{:04X}(len={})", b.subtype, b.total_size))
                .collect();
            println!(
                "  Non-music track [{:016X}] media_kind=0x{:X} {} - {:?}: bomas={subs:?}",
                t.header.track_persistent_id,
                t.header.media_kind,
                boma_str(&t.bomas, 0x0004).unwrap_or(""),
                boma_str(&t.bomas, 0x0002).unwrap_or(""),
            );
        }
        for b in &t.bomas {
            if b.subtype == 0x0024 {
                println!(
                    "  boma 0x0024 on [{:016X}] {} - {:?}: payload={:?}",
                    t.header.track_persistent_id,
                    boma_str(&t.bomas, 0x0004).unwrap_or(""),
                    boma_str(&t.bomas, 0x0002).unwrap_or(""),
                    b.payload,
                );
            }
        }
    }
    for rel in [
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb",
        "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
    ] {
        let p = corpus_dir.join(rel);
        if p.exists()
            && let Ok(r) = load_raw(&p)
        {
            for t in tracks_of(&r) {
                for b in &t.bomas {
                    if b.subtype == 0x0024 {
                        println!(
                            "  {rel} boma 0x0024 on [{:016X}] {:?}: payload={:?}",
                            t.header.track_persistent_id,
                            boma_str(&t.bomas, 0x0002).unwrap_or(""),
                            b.payload,
                        );
                    }
                }
            }
        }
    }

    let mut lyrics_hash_dist: BTreeMap<(u8, u8, bool, bool), usize> = BTreeMap::new();
    let mut instrumental_hashes: Vec<(String, u32, u32)> = Vec::new();
    for t in tracks_of(&raw_1119) {
        let mut cloud_lyrics_val = None;
        let mut hash_sub1 = 0u32;
        let mut hash_sub3 = 0u32;
        for b in &t.bomas {
            match &b.payload {
                BomaPayload::TrackNumerics(n) => {
                    if n.subrecord_kind == 1 {
                        hash_sub1 = n.custom_lyrics_hash;
                    } else {
                        hash_sub3 = n.custom_lyrics_hash;
                    }
                }
                BomaPayload::RawStringUtf8 { raw_bytes, .. }
                | BomaPayload::BinaryPlist { raw_bytes, .. }
                    if b.subtype == 0x0036 =>
                {
                    if let Ok(plist::Value::Dictionary(d)) =
                        plist::Value::from_reader(Cursor::new(raw_bytes))
                        && let Some(plist::Value::String(s)) = d.get("cloud-lyrics")
                    {
                        cloud_lyrics_val = Some(s.clone());
                    }
                }
                _ => {}
            }
        }
        let has_hash = hash_sub1 != 0 || hash_sub3 != 0;
        *lyrics_hash_dist
            .entry((
                t.header.has_local_custom_lyrics,
                t.header.has_custom_lyrics,
                cloud_lyrics_val.is_some(),
                has_hash,
            ))
            .or_insert(0) += 1;
        if let Some(s) = &cloud_lyrics_val
            && (s == "{{Instrumental}}" || s == "(Instrumental)")
            && instrumental_hashes.len() < 12
        {
            instrumental_hashes.push((
                format!(
                    "{} - {:?} ({s:?})",
                    boma_str(&t.bomas, 0x0004).unwrap_or(""),
                    boma_str(&t.bomas, 0x0002).unwrap_or("")
                ),
                hash_sub1,
                hash_sub3,
            ));
        }
    }
    println!(
        "  custom_lyrics_hash dist (itma44_local, itma255_token, has_0x36_lyrics, has_num124_hash): {lyrics_hash_dist:?}"
    );
    for (desc, h1, h3) in &instrumental_hashes {
        println!("    instrumental hash: {desc} => sub1=0x{h1:08X} ({h1}), sub3=0x{h3:08X} ({h3})");
    }

    let check_paths = [
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/2025/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/Previous Libraries/Apple Music Library [2025-09-28 20.36.49].musiclibrary/Library.musicdb",
        "library-data/Previous Libraries/Apple Music Library [2026-10-28 22.33.41].musiclibrary/Library.musicdb",
        "library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/2026-11-07/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-07-initial-bitflags/00_baseline/Library.musicdb",
        "snapshots/2026-11-07-initial-bitflags/06_noop_reopen/Library.musicdb",
        "snapshots/2026-11-11/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
        "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
    ];
    for rel in check_paths {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let r = load_raw(&p)?;
        for s in &r.sections {
            if let HsmaChild::LibraryMaster(plma) = &s.child {
                let mut f6 = String::from("none");
                let mut ff = String::from("none");
                for b in &plma.bomas {
                    if let BomaPayload::Unknown { subtype, raw_bytes } = &b.payload {
                        if *subtype == 0x01F6 {
                            f6 = format!(
                                "len={} bytes={:02X?}",
                                raw_bytes.len(),
                                raw_bytes.as_slice()
                            );
                        } else if *subtype == 0x01FF {
                            ff = format!(
                                "len={} head={:02X?}",
                                raw_bytes.len(),
                                raw_bytes.get(..24).unwrap_or(raw_bytes)
                            );
                        }
                    }
                }
                println!("  {rel}:\n    0x01F6: {f6}\n    0x01FF: {ff}");
            }
        }
    }

    // 7.9 Diff between 2026-11-14-offline-delete and 2026-11-19
    println!("\n--- 7.9 DIFF BETWEEN 2026-11-14-OFFLINE-DELETE AND 2026-11-19 ---");
    let base_p = corpus_dir.join(
        "snapshots/2026-11-14-offline-delete/offline-delete/Apple Music Library.musiclibrary/Library.musicdb",
    );
    if base_p.exists() {
        let raw_base = load_raw(&base_p)?;
        let old_by_id: BTreeMap<u64, &TrackItem> = tracks_of(&raw_base)
            .iter()
            .map(|t| (t.header.track_persistent_id, t))
            .collect();
        let mut diff_cnt = 0usize;
        for n in tracks_of(&raw_1119) {
            let tid = n.header.track_persistent_id;
            if let Some(o) = old_by_id.get(&tid)
                && (o.header.checkbox_part_of_gapless_album
                    != n.header.checkbox_part_of_gapless_album
                    || o.header.checkbox_show_composer_in_all_views
                        != n.header.checkbox_show_composer_in_all_views
                    || o.header.checkbox_use_work_and_movement
                        != n.header.checkbox_use_work_and_movement
                    || o.header.unk_53_55 != n.header.unk_53_55
                    || o.header.star_rating != n.header.star_rating
                    || o.header.auto_sort_flags != n.header.auto_sort_flags
                    || o.header.volume_adjustment != n.header.volume_adjustment
                    || o.header.has_local_custom_lyrics != n.header.has_local_custom_lyrics
                    || o.header.has_unsynced_local_changes != n.header.has_unsynced_local_changes
                    || o.header.unk_60_62 != n.header.unk_60_62)
            {
                diff_cnt += 1;
                if diff_cnt <= 20 {
                    let [o53, o54] = *o.header.unk_53_55;
                    let [n53, n54] = *n.header.unk_53_55;
                    println!(
                        "  Mutated Track [{tid:016X}] {} - {:?} (cloud={}):",
                        boma_str(&n.bomas, 0x0004).unwrap_or(""),
                        boma_str(&n.bomas, 0x0002).unwrap_or(""),
                        n.header.cloud_status,
                    );
                    println!(
                        "    off44(local_lyrics): {} -> {}, off46(unsynced): {} -> {}, off49..56: [{},{},{},{},{},{},{},{}] -> [{},{},{},{},{},{},{},{}]",
                        o.header.has_local_custom_lyrics,
                        n.header.has_local_custom_lyrics,
                        o.header.has_unsynced_local_changes,
                        n.header.has_unsynced_local_changes,
                        o.header.checkbox_part_of_gapless_album,
                        o.header.checkbox_remember_playback_position,
                        o.header.checkbox_show_composer_in_all_views,
                        o.header.checkbox_use_work_and_movement,
                        o53,
                        o54,
                        o.header.has_playback_or_bounds_activity,
                        *o.header.unk_56,
                        n.header.checkbox_part_of_gapless_album,
                        n.header.checkbox_remember_playback_position,
                        n.header.checkbox_show_composer_in_all_views,
                        n.header.checkbox_use_work_and_movement,
                        n53,
                        n54,
                        n.header.has_playback_or_bounds_activity,
                        *n.header.unk_56,
                    );
                    println!(
                        "    off60..62: {:02X?} -> {:02X?}, star_rating(65): {} -> {}, auto_sort_flags(66..72): {:02X?} -> {:02X?}",
                        *o.header.unk_60_62,
                        *n.header.unk_60_62,
                        o.header.star_rating,
                        n.header.star_rating,
                        o.header.auto_sort_flags,
                        n.header.auto_sort_flags,
                    );
                }
            }
        }
        println!("  Total tracks with metadata/flag changes between 11-14 and 11-19: {diff_cnt}");
    }

    Ok(())
}
