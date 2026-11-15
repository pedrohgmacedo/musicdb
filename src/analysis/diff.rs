use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use musicdb::PersistentId;
use musicdb::model::{Album, Artist, Playlist, Track};
use musicdb::sections::{
    AlbumItem, ArtistItem, BomaPayload, BomaSection, HsmaChild, LibraryMasterSection, PlaylistItem,
    RawLibraryFile, SectionKind, TrackItem, TrackNumerics,
};

use super::{diff_4byte_words, serialize_le};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LibraryDiffReport {
    pub(crate) hfma_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub(crate) hsma_10_hex_a: Option<String>,
    pub(crate) hsma_10_hex_b: Option<String>,
    pub(crate) plma_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub(crate) plma_boma_changes: Vec<String>,
    pub(crate) tracks_in_a: usize,
    pub(crate) tracks_in_b: usize,
    pub(crate) common_tracks: usize,
    pub(crate) added_tracks: Vec<String>,
    pub(crate) removed_tracks: Vec<String>,
    pub(crate) itma_offset_change_frequencies: BTreeMap<usize, usize>,
    pub(crate) track_numerics_offset_change_frequencies: BTreeMap<usize, usize>,
    pub(crate) sample_track_diffs: Vec<TrackDiffSample>,
    pub(crate) album_diffs: Vec<String>,
    pub(crate) artist_diffs: Vec<String>,
    pub(crate) playlist_diffs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TrackDiffSample {
    pub(crate) persistent_id: String,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) high_level_changes: Vec<String>,
    pub(crate) itma_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub(crate) numerics_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub(crate) plays_skips_changed_4byte_offsets: Vec<(usize, u32, u32)>,
}

#[derive(Default)]
struct DiffInputs<'a> {
    hsma_10_hex: Option<String>,
    plma: Option<&'a LibraryMasterSection>,
    albums: HashMap<u64, &'a AlbumItem>,
    artists: HashMap<u64, &'a ArtistItem>,
    playlists: HashMap<u64, &'a PlaylistItem>,
    tracks: HashMap<u64, &'a TrackItem>,
}

impl<'a> DiffInputs<'a> {
    fn from_raw(raw: &'a RawLibraryFile) -> Self {
        let mut inputs = Self::default();
        for hsma in &raw.sections {
            match &hsma.child {
                HsmaChild::TrackList(ltma) => {
                    for track in &ltma.tracks {
                        inputs
                            .tracks
                            .insert(track.header.track_persistent_id, track);
                    }
                }
                HsmaChild::LibraryMaster(plma) => inputs.plma = Some(plma),
                HsmaChild::AlbumList(list) => {
                    for album in &list.albums {
                        inputs
                            .albums
                            .insert(album.header.album_persistent_id, album);
                    }
                }
                HsmaChild::ArtistList(list) => {
                    for artist in &list.artists {
                        inputs
                            .artists
                            .insert(artist.header.artist_persistent_id, artist);
                    }
                }
                HsmaChild::PlaylistList(list) => {
                    for playlist in &list.playlists {
                        inputs
                            .playlists
                            .insert(playlist.header.playlist_persistent_id, playlist);
                    }
                }
                HsmaChild::Unknown {
                    kind: SectionKind::Trailer,
                    signature,
                    raw_bytes,
                } => {
                    let mut bytes = signature.to_vec();
                    bytes.extend_from_slice(raw_bytes);
                    inputs.hsma_10_hex = Some(hex::encode(bytes));
                }
                _ => {}
            }
        }
        inputs
    }
}

/// Compares two `RawLibraryFile` snapshots (e.g. 2024 vs 2025 or 2025 vs 2026) at both the high-level
/// and 4-byte word offset levels for common tracks, albums, artists, playlists, and headers.
pub(crate) fn diff_libraries(raw_a: &RawLibraryFile, raw_b: &RawLibraryFile) -> LibraryDiffReport {
    let hfma_changed_4byte_offsets =
        diff_4byte_words(&raw_a.outer_header, &raw_b.outer_header, &[]);
    let left = DiffInputs::from_raw(raw_a);
    let right = DiffInputs::from_raw(raw_b);

    let (plma_changed_4byte_offsets, plma_boma_changes) = diff_plma(left.plma, right.plma);
    let album_diffs = diff_albums(&left.albums, &right.albums);
    let artist_diffs = diff_artists(&left.artists, &right.artists);
    let playlist_diffs = diff_playlists(&left.playlists, &right.playlists);
    let tracks_diff = diff_tracks(&left.tracks, &right.tracks);

    LibraryDiffReport {
        hfma_changed_4byte_offsets,
        hsma_10_hex_a: left.hsma_10_hex,
        hsma_10_hex_b: right.hsma_10_hex,
        plma_changed_4byte_offsets,
        plma_boma_changes,
        tracks_in_a: left.tracks.len(),
        tracks_in_b: right.tracks.len(),
        common_tracks: tracks_diff.common_tracks,
        added_tracks: tracks_diff.added_tracks,
        removed_tracks: tracks_diff.removed_tracks,
        itma_offset_change_frequencies: tracks_diff.itma_freq,
        track_numerics_offset_change_frequencies: tracks_diff.numerics_freq,
        sample_track_diffs: tracks_diff.samples,
        album_diffs,
        artist_diffs,
        playlist_diffs,
    }
}

fn diff_plma(
    plma_a: Option<&LibraryMasterSection>,
    plma_b: Option<&LibraryMasterSection>,
) -> (Vec<(usize, u32, u32)>, Vec<String>) {
    let (Some(left), Some(right)) = (plma_a, plma_b) else {
        return (Vec::new(), Vec::new());
    };
    let changed_offsets = diff_4byte_words(&left.header, &right.header, &[]);
    let mut boma_changes = Vec::new();
    for boma_b in &right.bomas {
        let matching_a = left.bomas.iter().find(|b| b.subtype == boma_b.subtype);
        if matching_a.map(|a| &a.payload) != Some(&boma_b.payload) {
            let hex_a = matching_a.map_or_else(|| "<none>".to_string(), encode_boma_body_hex);
            let hex_b = encode_boma_body_hex(boma_b);
            boma_changes.push(format!(
                "plma boma 0x{:04X} ({}) changed:\n      A: {hex_a}\n      B: {hex_b}",
                boma_b.subtype,
                boma_b.subtype_name()
            ));
        }
    }
    (changed_offsets, boma_changes)
}

fn encode_boma_body_hex(boma: &BomaSection) -> String {
    let bytes = serialize_le(boma);
    let Some(body) = bytes.get(20..) else {
        return String::new();
    };
    hex::encode(body)
}

fn diff_albums(
    left_albums: &HashMap<u64, &AlbumItem>,
    right_albums: &HashMap<u64, &AlbumItem>,
) -> Vec<String> {
    let mut diffs = Vec::new();
    for (&apid, &ab) in right_albums {
        if !left_albums.contains_key(&apid) {
            let model_b = Album::from(ab);
            diffs.push(format!(
                "+ ADDED ALBUM {apid:016X}: title={:?} artist={:?} album_artist={:?} is_comp={}",
                model_b.title, model_b.artist, model_b.album_artist, model_b.is_compilation
            ));
        }
    }
    for (&apid, &aa) in left_albums {
        let Some(&ab) = right_albums.get(&apid) else {
            let model_a = Album::from(aa);
            diffs.push(format!(
                "- REMOVED ALBUM {apid:016X}: title={:?} artist={:?} album_artist={:?}",
                model_a.title, model_a.artist, model_a.album_artist
            ));
            continue;
        };
        let off_diffs = diff_4byte_words(&aa.header, &ab.header, &[8]);
        let model_a = Album::from(aa);
        let model_b = Album::from(ab);
        if !off_diffs.is_empty()
            || model_a.title != model_b.title
            || model_a.artist != model_b.artist
            || model_a.album_artist != model_b.album_artist
        {
            diffs.push(format!(
                "~ CHANGED ALBUM {apid:016X} ({:?}): offsets={off_diffs:?} artist={:?}->{:?} album_artist={:?}->{:?} comp={}->{} rating={}->{} fav={:?}->{:?}",
                model_b.title,
                model_a.artist,
                model_b.artist,
                model_a.album_artist,
                model_b.album_artist,
                model_a.is_compilation,
                model_b.is_compilation,
                model_a.star_rating,
                model_b.star_rating,
                model_a.suggestion_flag,
                model_b.suggestion_flag
            ));
        }
    }
    diffs
}

fn diff_artists(
    left_artists: &HashMap<u64, &ArtistItem>,
    right_artists: &HashMap<u64, &ArtistItem>,
) -> Vec<String> {
    let mut diffs = Vec::new();
    for (&rpid, &ra) in left_artists {
        if let Some(&rb) = right_artists.get(&rpid) {
            let off_diffs = diff_4byte_words(&ra.header, &rb.header, &[8]);
            let model_a = Artist::from(ra);
            let model_b = Artist::from(rb);
            if !off_diffs.is_empty()
                || model_a.name != model_b.name
                || model_a.sort_name != model_b.sort_name
            {
                diffs.push(format!(
                    "~ CHANGED ARTIST {rpid:016X} ({:?}): offsets={off_diffs:?} fav={:?}->{:?}",
                    model_b.name, model_a.suggestion_flag, model_b.suggestion_flag
                ));
            }
        }
    }
    diffs
}

fn diff_playlists(
    playlists_a: &HashMap<u64, &PlaylistItem>,
    playlists_b: &HashMap<u64, &PlaylistItem>,
) -> Vec<String> {
    let mut diffs = Vec::new();
    let mut right_keys: Vec<u64> = playlists_b.keys().copied().collect();
    right_keys.sort_unstable();
    for ppid in right_keys {
        let Some(&pl_b) = playlists_b.get(&ppid) else {
            continue;
        };
        if let Some(&pl_a) = playlists_a.get(&ppid) {
            if let Some(msg) = diff_changed_playlist(ppid, pl_a, pl_b) {
                diffs.push(msg);
            }
        } else {
            diffs.push(format_added_playlist(ppid, pl_b));
        }
    }

    let mut left_keys: Vec<u64> = playlists_a.keys().copied().collect();
    left_keys.sort_unstable();
    for ppid in left_keys {
        if !playlists_b.contains_key(&ppid)
            && let Some(&pl_a) = playlists_a.get(&ppid)
        {
            let mod_a = Playlist::from(pl_a);
            diffs.push(format!(
                "- REMOVED PLAYLIST {ppid:016X} ({:?}): parent={:?} kind={:?} tracks={}",
                mod_a.name, mod_a.parent_persistent_id, mod_a.kind, mod_a.declared_track_count
            ));
        }
    }
    diffs
}

fn diff_changed_playlist(ppid: u64, pl_a: &PlaylistItem, pl_b: &PlaylistItem) -> Option<String> {
    let mut ignore = vec![8, 16];
    if pl_a.header.date_created == pl_b.header.date_created {
        ignore.push(24);
    }
    let off_diffs = diff_4byte_words(&pl_a.header, &pl_b.header, &ignore);
    let mod_a = Playlist::from(pl_a);
    let mod_b = Playlist::from(pl_b);
    let mut boma_diffs = Vec::new();
    if pl_a.bomas.len() != pl_b.bomas.len() {
        boma_diffs.push(format!(
            "boma_count: {}->{}",
            pl_a.bomas.len(),
            pl_b.bomas.len()
        ));
    }
    for boma_b in &pl_b.bomas {
        if boma_b.subtype == 0x00DA || boma_b.subtype == 0x00CE {
            continue;
        }
        let matching_a = pl_a.bomas.iter().find(|ba| ba.subtype == boma_b.subtype);
        if matching_a.map(|x| &x.payload) != Some(&boma_b.payload) {
            boma_diffs.push(format!(
                "boma 0x{:04X} ({}) changed",
                boma_b.subtype,
                boma_b.subtype_name()
            ));
        }
    }
    if mod_a.track_persistent_ids != mod_b.track_persistent_ids {
        boma_diffs.push(format_playlist_track_diff(
            &mod_a.track_persistent_ids,
            &mod_b.track_persistent_ids,
        ));
    }
    if !off_diffs.is_empty()
        || !boma_diffs.is_empty()
        || mod_a.name != mod_b.name
        || mod_a.description != mod_b.description
        || mod_a.declared_track_count != mod_b.declared_track_count
    {
        return Some(format!(
            "~ CHANGED PLAYLIST {ppid:016X} ({:?}): parent={:?} tracks={}->{} share={}->{} fav={:?}->{:?} desc={:?}->{:?} offsets={off_diffs:?} boma_diffs={boma_diffs:?}",
            mod_b.name,
            mod_b.parent_persistent_id,
            mod_a.declared_track_count,
            mod_b.declared_track_count,
            mod_a.share_mode.raw(),
            mod_b.share_mode.raw(),
            mod_a.suggestion_flag,
            mod_b.suggestion_flag,
            mod_a.description,
            mod_b.description
        ));
    }
    None
}

fn format_playlist_track_diff(left: &[PersistentId], right: &[PersistentId]) -> String {
    let set_a: HashSet<PersistentId> = left.iter().copied().collect();
    let set_b: HashSet<PersistentId> = right.iter().copied().collect();
    let added: Vec<String> = set_b
        .difference(&set_a)
        .take(20)
        .map(ToString::to_string)
        .collect();
    let removed: Vec<String> = set_a
        .difference(&set_b)
        .take(20)
        .map(ToString::to_string)
        .collect();
    format!(
        "track_pids changed ({} -> {}, +{}={added:?}, -{}={removed:?})",
        left.len(),
        right.len(),
        set_b.difference(&set_a).count(),
        set_a.difference(&set_b).count(),
    )
}

fn format_added_playlist(ppid: u64, pl_b: &PlaylistItem) -> String {
    let mod_b = Playlist::from(pl_b);
    let hdr_bytes = serialize_le(&pl_b.header);
    let mut extra_info = String::new();
    let nonzero_hdr: Vec<String> = hdr_bytes
        .iter()
        .enumerate()
        .filter(|&(idx, &b)| b != 0 && !(0..38).contains(&idx))
        .map(|(idx, &b)| format!("{idx}:0x{b:02X}"))
        .collect();
    let _ = write!(
        extra_info,
        "\n      nonzero_hdr(>37)=[{}]",
        nonzero_hdr.join(", ")
    );
    for boma in &pl_b.bomas {
        append_added_boma_info(&mut extra_info, boma);
    }
    format!(
        "+ ADDED PLAYLIST {ppid:016X} ({:?}): parent={:?} is_folder={} is_smart={} tracks={} share={} fav={:?} desc={:?}{extra_info}",
        mod_b.name,
        mod_b.parent_persistent_id,
        mod_b.is_folder,
        mod_b.is_smart,
        mod_b.declared_track_count,
        mod_b.share_mode.raw(),
        mod_b.suggestion_flag,
        mod_b.description
    )
}

fn append_added_boma_info(out: &mut String, boma: &BomaSection) {
    match &boma.payload {
        BomaPayload::SmartPlaylistRules(rules) => {
            let _ = write!(
                out,
                "\n      [SmartPlaylistRules 0x00C9]: unk_4=0x{:08X} rule_count={} conjunction={}",
                rules.unk_4, rules.rule_count, rules.conjunction
            );
            for (ri, rule) in rules.rules.iter().enumerate() {
                let _ = write!(
                    out,
                    "\n        Rule #{:02}: field=0x{:02X} ({}: {}) cmp=0x{:08X} ({}) unk_8=0x{:08X} unk_44=0x{:X} unk_48=0x{:X} child={:?}",
                    ri + 1,
                    rule.field_id,
                    rule.field_id,
                    rule.field_name(),
                    rule.comparison_method,
                    rule.comparison_description(),
                    rule.unk_8,
                    rule.unk_44,
                    rule.unk_48,
                    rule.child
                );
            }
        }
        BomaPayload::SmartPlaylistOptions(opts) => {
            let raw_hex = hex::encode(serialize_le(opts.as_ref()));
            let _ = write!(
                out,
                "\n      [SmartPlaylistOptions 0x00CA]: live={} match_rules={} limit={} unit={} ({}) select_method=0x{:02X} ({}: {}) limit_count={} only_checked={} negate_select={} raw_hex={raw_hex}",
                opts.checkbox_live_updating,
                opts.checkbox_enable_matching_rules,
                opts.checkbox_enable_limit,
                opts.limit_unit,
                opts.limit_unit_name(),
                opts.limit_selection_method,
                opts.limit_selection_method,
                opts.selection_method_name(),
                opts.limit_count,
                opts.checkbox_match_only_checked_items,
                opts.negate_limit_selection_method,
            );
        }
        BomaPayload::StringSection(s) => {
            let _ = write!(
                out,
                "\n      [boma 0x{:04X} ({})]: {:?}",
                boma.subtype,
                boma.subtype_name(),
                s.text
            );
        }
        BomaPayload::RawStringUtf8 { text, .. } | BomaPayload::RawStringUtf16Le { text, .. } => {
            let _ = write!(
                out,
                "\n      [boma 0x{:04X} ({})]: {text:?}",
                boma.subtype,
                boma.subtype_name(),
            );
        }
        BomaPayload::PlaylistItemCompact(ipfa) => {
            let _ = write!(
                out,
                "\n      [IPfa 0x00DA]: item_pid={:016X} track_pid={:016X}",
                ipfa.item_persistent_id, ipfa.track_persistent_id
            );
        }
        BomaPayload::Unknown { raw_bytes, .. } => {
            let _ = write!(
                out,
                "\n      [boma 0x{:04X} ({})]: len={} hex={}",
                boma.subtype,
                boma.subtype_name(),
                raw_bytes.len(),
                hex::encode(raw_bytes)
            );
        }
        _ => {}
    }
}

struct TrackDiffAccumulator {
    common_tracks: usize,
    added_tracks: Vec<String>,
    removed_tracks: Vec<String>,
    itma_freq: BTreeMap<usize, usize>,
    numerics_freq: BTreeMap<usize, usize>,
    samples: Vec<TrackDiffSample>,
}

fn diff_tracks(
    tracks_a: &HashMap<u64, &TrackItem>,
    tracks_b: &HashMap<u64, &TrackItem>,
) -> TrackDiffAccumulator {
    let mut acc = TrackDiffAccumulator {
        common_tracks: 0,
        added_tracks: Vec::new(),
        removed_tracks: Vec::new(),
        itma_freq: BTreeMap::new(),
        numerics_freq: BTreeMap::new(),
        samples: Vec::new(),
    };

    for (&pid, &itma_b) in tracks_b {
        if !tracks_a.contains_key(&pid) {
            let tb = Track::from(itma_b);
            acc.added_tracks.push(format!(
                "{} ({:?} - {:?})",
                tb.persistent_id, tb.artist, tb.title
            ));
        }
    }

    let mut pids: Vec<u64> = tracks_a.keys().copied().collect();
    pids.sort_unstable();
    for pid in pids {
        let Some(&itma_a) = tracks_a.get(&pid) else {
            continue;
        };
        let Some(&itma_b) = tracks_b.get(&pid) else {
            let ta = Track::from(itma_a);
            acc.removed_tracks.push(format!(
                "{} ({:?} - {:?})",
                ta.persistent_id, ta.artist, ta.title
            ));
            continue;
        };
        acc.common_tracks += 1;
        diff_common_track(itma_a, itma_b, &mut acc);
    }

    acc.added_tracks.truncate(50);
    acc.removed_tracks.truncate(50);
    acc
}

fn diff_common_track(itma_a: &TrackItem, itma_b: &TrackItem, acc: &mut TrackDiffAccumulator) {
    let mut itma_changed = Vec::new();
    let mut non_sort_itma_changed = false;
    for (off, va, vb) in diff_4byte_words(&itma_a.header, &itma_b.header, &[]) {
        *acc.itma_freq.entry(off).or_insert(0) += 1;
        if off != 24 {
            itma_changed.push((off, va, vb));
            if !(280..308).contains(&off) {
                non_sort_itma_changed = true;
            }
        }
    }

    let numerics_changed = diff_track_numerics(itma_a, itma_b, &mut acc.numerics_freq);
    let ps_a = itma_a.bomas.iter().find_map(|b| match &b.payload {
        BomaPayload::TrackPlaysSkips(ps) => Some(ps),
        _ => None,
    });
    let ps_b = itma_b.bomas.iter().find_map(|b| match &b.payload {
        BomaPayload::TrackPlaysSkips(ps) => Some(ps),
        _ => None,
    });
    let plays_skips_changed = match (ps_a, ps_b) {
        (Some(pa), Some(pb)) => diff_4byte_words(pa, pb, &[]),
        _ => Vec::new(),
    };

    let ta = Track::from(itma_a);
    let tb = Track::from(itma_b);
    let mut hl_changes = collect_track_hl_changes(&ta, &tb);
    let subs_a: Vec<u32> = itma_a.bomas.iter().map(|b| b.subtype).collect();
    let subs_b: Vec<u32> = itma_b.bomas.iter().map(|b| b.subtype).collect();
    if subs_a != subs_b {
        hl_changes.push(format!("boma_subtypes: {subs_a:04X?} -> {subs_b:04X?}"));
    }

    if (!hl_changes.is_empty()
        || non_sort_itma_changed
        || !numerics_changed.is_empty()
        || !plays_skips_changed.is_empty())
        && acc.samples.len() < 100
    {
        acc.samples.push(TrackDiffSample {
            persistent_id: ta.persistent_id.to_string(),
            title: tb.title,
            artist: tb.artist,
            high_level_changes: hl_changes,
            itma_changed_4byte_offsets: itma_changed,
            numerics_changed_4byte_offsets: numerics_changed,
            plays_skips_changed_4byte_offsets: plays_skips_changed,
        });
    }
}

fn collect_track_numerics(item: &TrackItem) -> Vec<&TrackNumerics> {
    item.bomas
        .iter()
        .filter_map(|b| match &b.payload {
            BomaPayload::TrackNumerics(n) => Some(n.as_ref()),
            _ => None,
        })
        .collect()
}

fn diff_track_numerics(
    itma_a: &TrackItem,
    itma_b: &TrackItem,
    freq: &mut BTreeMap<usize, usize>,
) -> Vec<(usize, u32, u32)> {
    let nums_a = collect_track_numerics(itma_a);
    let nums_b = collect_track_numerics(itma_b);
    let mut out = Vec::new();
    for (idx, (na, nb)) in nums_a.iter().zip(nums_b.iter()).enumerate() {
        for (off, va, vb) in diff_4byte_words(*na, *nb, &[]) {
            *freq.entry(off).or_insert(0) += 1;
            if off != 0 {
                out.push((idx * 1000 + off, va, vb));
            }
        }
    }
    out
}

fn cmp_hl_field<T: PartialEq + std::fmt::Debug>(
    changes: &mut Vec<String>,
    name: &str,
    left: &T,
    right: &T,
) {
    if left != right {
        changes.push(format!("{name}: {left:?} -> {right:?}"));
    }
}

fn collect_track_hl_changes(ta: &Track, tb: &Track) -> Vec<String> {
    let mut changes = Vec::new();
    collect_track_text_changes(ta, tb, &mut changes);
    collect_track_flag_changes(ta, tb, &mut changes);
    collect_track_stat_changes(ta, tb, &mut changes);
    changes
}

fn collect_track_text_changes(ta: &Track, tb: &Track, changes: &mut Vec<String>) {
    cmp_hl_field(changes, "title", &ta.title, &tb.title);
    cmp_hl_field(changes, "album", &ta.album, &tb.album);
    cmp_hl_field(changes, "artist", &ta.artist, &tb.artist);
    cmp_hl_field(changes, "album_artist", &ta.album_artist, &tb.album_artist);
    cmp_hl_field(changes, "composer", &ta.composer, &tb.composer);
    cmp_hl_field(changes, "grouping", &ta.grouping, &tb.grouping);
    cmp_hl_field(changes, "comments", &ta.comments, &tb.comments);
    cmp_hl_field(changes, "work_name", &ta.work_name, &tb.work_name);
    cmp_hl_field(
        changes,
        "movement_name",
        &ta.movement_name,
        &tb.movement_name,
    );
    cmp_hl_field(changes, "sort_title", &ta.sort_title, &tb.sort_title);
    cmp_hl_field(changes, "sort_album", &ta.sort_album, &tb.sort_album);
    cmp_hl_field(changes, "sort_artist", &ta.sort_artist, &tb.sort_artist);
    cmp_hl_field(
        changes,
        "sort_album_artist",
        &ta.sort_album_artist,
        &tb.sort_album_artist,
    );
    cmp_hl_field(
        changes,
        "sort_composer",
        &ta.sort_composer,
        &tb.sort_composer,
    );
    cmp_hl_field(changes, "equalizer", &ta.equalizer, &tb.equalizer);
    cmp_hl_field(changes, "file_url", &ta.file_url, &tb.file_url);
    cmp_hl_field(
        changes,
        "legacy_file_path",
        &ta.legacy_file_path,
        &tb.legacy_file_path,
    );
}

fn collect_track_flag_changes(ta: &Track, tb: &Track, changes: &mut Vec<String>) {
    cmp_hl_field(changes, "disabled", &ta.disabled, &tb.disabled);
    cmp_hl_field(
        changes,
        "skip_when_shuffling",
        &ta.skip_when_shuffling,
        &tb.skip_when_shuffling,
    );
    cmp_hl_field(
        changes,
        "remember_playback_position",
        &ta.remember_playback_position,
        &tb.remember_playback_position,
    );
    cmp_hl_field(
        changes,
        "show_composer_in_all_views",
        &ta.show_composer_in_all_views,
        &tb.show_composer_in_all_views,
    );
    cmp_hl_field(
        changes,
        "use_work_and_movement",
        &ta.use_work_and_movement,
        &tb.use_work_and_movement,
    );
    cmp_hl_field(
        changes,
        "album_is_compilation",
        &ta.album_is_compilation,
        &tb.album_is_compilation,
    );
    cmp_hl_field(
        changes,
        "has_custom_lyrics",
        &ta.has_custom_lyrics,
        &tb.has_custom_lyrics,
    );
    cmp_hl_field(
        changes,
        "has_local_custom_lyrics",
        &ta.has_local_custom_lyrics,
        &tb.has_local_custom_lyrics,
    );
    cmp_hl_field(
        changes,
        "has_unsynced_local_changes",
        &ta.has_unsynced_local_changes,
        &tb.has_unsynced_local_changes,
    );
    cmp_hl_field(changes, "sort_modes", &ta.sort_modes, &tb.sort_modes);
}

fn collect_track_stat_changes(ta: &Track, tb: &Track, changes: &mut Vec<String>) {
    cmp_hl_field(
        changes,
        "movement_number",
        &ta.movement_number,
        &tb.movement_number,
    );
    cmp_hl_field(
        changes,
        "movement_total",
        &ta.movement_total,
        &tb.movement_total,
    );
    cmp_hl_field(changes, "disc_number", &ta.disc_number, &tb.disc_number);
    cmp_hl_field(changes, "disc_total", &ta.disc_total, &tb.disc_total);
    cmp_hl_field(changes, "bpm", &ta.bpm, &tb.bpm);
    cmp_hl_field(
        changes,
        "volume_adjustment",
        &ta.volume_adjustment,
        &tb.volume_adjustment,
    );
    cmp_hl_field(
        changes,
        "playback_start_ms",
        &ta.playback_start_ms,
        &tb.playback_start_ms,
    );
    cmp_hl_field(
        changes,
        "playback_stop_ms",
        &ta.playback_stop_ms,
        &tb.playback_stop_ms,
    );
    cmp_hl_field(
        changes,
        "custom_lyrics_hash",
        &ta.custom_lyrics_hash,
        &tb.custom_lyrics_hash,
    );
    cmp_hl_field(
        changes,
        "cloud_lyrics_token",
        &ta.cloud_lyrics_token,
        &tb.cloud_lyrics_token,
    );
    cmp_hl_field(changes, "play_count", &ta.play_count, &tb.play_count);
    cmp_hl_field(
        changes,
        "true_play_count",
        &ta.true_play_count,
        &tb.true_play_count,
    );
    cmp_hl_field(changes, "skip_count", &ta.skip_count, &tb.skip_count);
    cmp_hl_field(
        changes,
        "true_skip_count",
        &ta.true_skip_count,
        &tb.true_skip_count,
    );
    cmp_hl_field(changes, "star_rating", &ta.star_rating, &tb.star_rating);
    cmp_hl_field(
        changes,
        "suggestion_flag",
        &ta.suggestion_flag,
        &tb.suggestion_flag,
    );
    cmp_hl_field(
        changes,
        "date_modified",
        &ta.date_modified,
        &tb.date_modified,
    );
}
