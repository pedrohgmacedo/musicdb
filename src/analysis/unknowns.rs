use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use musicdb::sections::{
    BomaPayload, BomaSection, HsmaChild, PlaylistItem, RawLibraryFile, TrackItemHeader,
    TrackNumerics, Unknown, UnknownSpan, UnknownValue, UnknownVisitor, VisitUnknowns,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UnknownFieldSummary {
    pub(crate) section: String,
    pub(crate) field_name: String,
    pub(crate) offset_info: String,
    pub(crate) total_instances: usize,
    pub(crate) nonzero_instances: usize,
    pub(crate) distinct_nonzero_count: usize,
    pub(crate) sample_values: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UnknownsReport {
    pub(crate) unknown_hsma_sections: Vec<(u32, String, usize)>,
    pub(crate) unknown_boma_subtypes: BTreeMap<String, usize>,
    pub(crate) boma_subtype_counts: BTreeMap<String, usize>,
    pub(crate) ipfa_short_id_verification: IpfaShortIdCheck,
    pub(crate) global_counter_analysis: GlobalCounterAnalysis,
    pub(crate) nonzero_unknown_fields: Vec<UnknownFieldSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IpfaShortIdCheck {
    pub(crate) total_ipfa_items: usize,
    pub(crate) matched_track_persistent_id: usize,
    pub(crate) matched_itma_global_counter: usize,
    pub(crate) mismatched_examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GlobalCounterAnalysis {
    pub(crate) album_count: usize,
    pub(crate) artist_count: usize,
    pub(crate) first_itma_global_counter: Option<u32>,
    pub(crate) last_itma_global_counter: Option<u32>,
    pub(crate) first_lpma_global_counter: Option<u32>,
    pub(crate) last_ipfa_global_counter: Option<u32>,
}

struct FieldTracker {
    section: String,
    field_name: String,
    offset_info: String,
    total: usize,
    nonzero: usize,
    counts: HashMap<String, usize>,
}

impl FieldTracker {
    fn new(section: String, field_name: String, offset_info: String) -> Self {
        Self {
            section,
            field_name,
            offset_info,
            total: 0,
            nonzero: 0,
            counts: HashMap::new(),
        }
    }

    fn record_value<T: UnknownValue>(&mut self, val: &Unknown<T>) {
        self.total += 1;
        if !val.is_zero() {
            self.nonzero += 1;
            *self.counts.entry(val.format_unknown()).or_insert(0) += 1;
        }
    }

    fn record_bytes(&mut self, bytes: &[u8]) {
        self.total += 1;
        if bytes.iter().any(|&b| b != 0) {
            self.nonzero += 1;
            *self.counts.entry(hex::encode(bytes)).or_insert(0) += 1;
        }
    }

    fn into_summary(self) -> Option<UnknownFieldSummary> {
        if self.nonzero == 0 {
            return None;
        }
        let distinct_nonzero_count = self.counts.len();
        let mut pairs: Vec<(String, usize)> = self.counts.into_iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        pairs.truncate(15);
        Some(UnknownFieldSummary {
            section: self.section,
            field_name: self.field_name,
            offset_info: self.offset_info,
            total_instances: self.total,
            nonzero_instances: self.nonzero,
            distinct_nonzero_count,
            sample_values: pairs,
        })
    }
}

#[derive(Default)]
struct TrackerSet {
    trackers: Vec<FieldTracker>,
    map: HashMap<String, HashMap<String, usize>>,
}

impl TrackerSet {
    fn with_tracker(
        &mut self,
        sec: &str,
        name: &str,
        make_off: impl FnOnce() -> String,
        apply: impl FnOnce(&mut FieldTracker),
    ) {
        let idx = if let Some(&existing) = self.map.get(sec).and_then(|fields| fields.get(name)) {
            existing
        } else {
            let i = self.trackers.len();
            self.trackers.push(FieldTracker::new(
                sec.to_string(),
                name.to_string(),
                make_off(),
            ));
            self.map
                .entry(sec.to_string())
                .or_default()
                .insert(name.to_string(), i);
            i
        };
        if let Some(tracker) = self.trackers.get_mut(idx) {
            apply(tracker);
        }
    }

    fn bytes(&mut self, sec: &str, name: &str, off: &str, val: &[u8]) {
        self.with_tracker(sec, name, || off.to_string(), |t| t.record_bytes(val));
    }

    fn into_summaries(self) -> Vec<UnknownFieldSummary> {
        self.trackers
            .into_iter()
            .filter_map(FieldTracker::into_summary)
            .collect()
    }
}

impl UnknownVisitor for TrackerSet {
    fn visit_unknown<T: UnknownValue>(
        &mut self,
        section: &str,
        field_name: &str,
        span: UnknownSpan,
        value: &Unknown<T>,
    ) {
        self.with_tracker(
            section,
            field_name,
            || span.to_string(),
            |t| t.record_value(value),
        );
    }
}

#[derive(Default)]
struct UnknownsCollector {
    trackers: TrackerSet,
    unknown_hsma_sections: Vec<(u32, String, usize)>,
    unknown_boma_subtypes: BTreeMap<String, usize>,
    boma_subtype_counts: BTreeMap<String, usize>,
    track_short_id_by_persistent_id: HashMap<u64, u32>,
    album_count: usize,
    artist_count: usize,
    first_itma_gc: Option<u32>,
    last_itma_gc: Option<u32>,
    first_lpma_gc: Option<u32>,
    last_ipfa_gc: Option<u32>,
    total_ipfa_items: usize,
    matched_track_persistent_id: usize,
    matched_itma_global_counter: usize,
    mismatched_examples: Vec<String>,
}

impl UnknownsCollector {
    fn index_tracks(&mut self, raw: &RawLibraryFile) {
        for hsma in &raw.sections {
            if let HsmaChild::TrackList(ltma) = &hsma.child {
                for itma in &ltma.tracks {
                    self.track_short_id_by_persistent_id
                        .insert(itma.header.track_persistent_id, itma.header.global_counter);
                    if self.first_itma_gc.is_none() {
                        self.first_itma_gc = Some(itma.header.global_counter);
                    }
                    self.last_itma_gc = Some(itma.header.global_counter);
                }
            }
        }
    }

    fn record_boma(&mut self, parent: &str, boma: &BomaSection) {
        record_boma_stats(
            parent,
            boma,
            &mut self.boma_subtype_counts,
            &mut self.unknown_boma_subtypes,
        );
    }

    fn record_match_cat_tuple(&mut self, hdr: &TrackItemHeader, num: &TrackNumerics) {
        self.trackers.bytes(
            "TrackNumerics",
            "match_cat_tuple(sub,cloud,32,33,34,35)",
            "32..36",
            &[
                u8::try_from(num.subrecord_kind).unwrap_or(u8::MAX),
                hdr.cloud_status,
                num.is_itunes_matched[0],
                num.is_itunes_matched[1],
                num.is_apple_music_catalog,
                num.unk_35_60.first().copied().unwrap_or_default(),
            ],
        );
    }

    fn check_playlist_bomas(&mut self, lpma: &PlaylistItem) {
        for boma in &lpma.bomas {
            self.record_boma("lpma", boma);
            if let BomaPayload::PlaylistItem(ipfa) = &boma.payload {
                self.last_ipfa_gc = Some(ipfa.global_counter);
                self.total_ipfa_items += 1;
                if let Some(&itma_gc) = self
                    .track_short_id_by_persistent_id
                    .get(&ipfa.track_persistent_id)
                {
                    self.matched_track_persistent_id += 1;
                    if *ipfa.unk_40_counter == itma_gc {
                        self.matched_itma_global_counter += 1;
                    } else if self.mismatched_examples.len() < 10 {
                        self.mismatched_examples.push(format!(
                            "track {:016X}: ipfa.unk_40={} != itma.global_counter={itma_gc}",
                            ipfa.track_persistent_id, ipfa.unk_40_counter
                        ));
                    }
                }
            } else if let BomaPayload::PlaylistItemCompact(ipfa) = &boma.payload {
                self.total_ipfa_items += 1;
                if self
                    .track_short_id_by_persistent_id
                    .contains_key(&ipfa.track_persistent_id)
                {
                    self.matched_track_persistent_id += 1;
                }
            }
        }
    }

    fn finish(self) -> UnknownsReport {
        UnknownsReport {
            unknown_hsma_sections: self.unknown_hsma_sections,
            unknown_boma_subtypes: self.unknown_boma_subtypes,
            boma_subtype_counts: self.boma_subtype_counts,
            ipfa_short_id_verification: IpfaShortIdCheck {
                total_ipfa_items: self.total_ipfa_items,
                matched_track_persistent_id: self.matched_track_persistent_id,
                matched_itma_global_counter: self.matched_itma_global_counter,
                mismatched_examples: self.mismatched_examples,
            },
            global_counter_analysis: GlobalCounterAnalysis {
                album_count: self.album_count,
                artist_count: self.artist_count,
                first_itma_global_counter: self.first_itma_gc,
                last_itma_global_counter: self.last_itma_gc,
                first_lpma_global_counter: self.first_lpma_gc,
                last_ipfa_global_counter: self.last_ipfa_gc,
            },
            nonzero_unknown_fields: self.trackers.into_summaries(),
        }
    }
}

/// Scans a `RawLibraryFile` for all unknown byte regions, unhandled `boma` subtypes,
/// and verifies structural invariants (such as `ipfa` offset 40 == `itma` `global_counter`).
pub(crate) fn analyze_unknowns(raw: &RawLibraryFile) -> UnknownsReport {
    let mut collector = UnknownsCollector::default();
    raw.visit_unknowns("", &mut collector.trackers);
    collector.index_tracks(raw);

    for hsma in &raw.sections {
        match &hsma.child {
            HsmaChild::LibraryMaster(plma) => {
                for boma in &plma.bomas {
                    collector.record_boma("plma", boma);
                }
            }
            HsmaChild::AlbumList(lama) => {
                collector.album_count = lama.albums.len();
                for album in &lama.albums {
                    for boma in &album.bomas {
                        collector.record_boma("iama", boma);
                    }
                }
            }
            HsmaChild::ArtistList(artist_list) => {
                collector.artist_count = artist_list.artists.len();
                for artist in &artist_list.artists {
                    for boma in &artist.bomas {
                        collector.record_boma("iAma", boma);
                    }
                }
            }
            HsmaChild::TrackList(ltma) => {
                for itma in &ltma.tracks {
                    for boma in &itma.bomas {
                        collector.record_boma("itma", boma);
                        if let BomaPayload::TrackNumerics(num) = &boma.payload {
                            collector.record_match_cat_tuple(&itma.header, num);
                        }
                    }
                }
            }
            HsmaChild::PlaylistList(playlist_list) => {
                for lpma in &playlist_list.playlists {
                    if collector.first_lpma_gc.is_none() {
                        collector.first_lpma_gc = Some(lpma.header.global_counter);
                    }
                    collector.check_playlist_bomas(lpma);
                }
            }
            HsmaChild::SocialProfileList(lsma) => {
                for isma in &lsma.profiles {
                    for boma in &isma.bomas {
                        collector.record_boma("isma", boma);
                    }
                }
            }
            HsmaChild::Unknown {
                kind,
                signature,
                raw_bytes,
            } => {
                collector.unknown_hsma_sections.push((
                    u32::from(*kind),
                    String::from_utf8_lossy(signature).into_owned(),
                    raw_bytes.len(),
                ));
            }
            HsmaChild::InnerHfma(_) | HsmaChild::Padding(_) | HsmaChild::Skipped { .. } | _ => {}
        }
    }

    collector.finish()
}

fn record_boma_stats(
    parent: &str,
    boma: &BomaSection,
    subtype_counts: &mut BTreeMap<String, usize>,
    unknown_subtypes: &mut BTreeMap<String, usize>,
) {
    let key = format!("{parent}::0x{:04X} ({})", boma.subtype, boma.subtype_name());
    *subtype_counts.entry(key.clone()).or_insert(0) += 1;
    if matches!(boma.payload, BomaPayload::Unknown { .. }) {
        *unknown_subtypes.entry(key).or_insert(0) += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use musicdb::sections::RawLibraryFile;

    fn make_boma(subtype: u32, unk_16: &[u8], payload: &[u8]) -> Vec<u8> {
        let header_size = u32::try_from(16 + unk_16.len()).expect("header_size");
        let total_size = u32::try_from(16 + unk_16.len() + payload.len()).expect("total_size");
        let mut out = Vec::with_capacity(total_size as usize);
        out.extend_from_slice(b"boma");
        out.extend_from_slice(&header_size.to_le_bytes());
        out.extend_from_slice(&total_size.to_le_bytes());
        out.extend_from_slice(&subtype.to_le_bytes());
        out.extend_from_slice(unk_16);
        out.extend_from_slice(payload);
        out
    }

    fn wrap_hsma(subtype: u32, child: &[u8]) -> Vec<u8> {
        let total = u32::try_from(56 + child.len()).expect("hsma total");
        let mut out = vec![0u8; 56];
        out[0..4].copy_from_slice(b"hsma");
        out[4..8].copy_from_slice(&56u32.to_le_bytes());
        out[8..12].copy_from_slice(&total.to_le_bytes());
        out[12..16].copy_from_slice(&subtype.to_le_bytes());
        out.extend_from_slice(child);
        out
    }

    fn make_slst_be(unk_4: u32, rules_bytes: &[u8], rule_count: u32) -> Vec<u8> {
        let mut out = vec![0u8; 136];
        out[0..4].copy_from_slice(b"SLst");
        out[4..8].copy_from_slice(&unk_4.to_be_bytes());
        out[8..12].copy_from_slice(&rule_count.to_be_bytes());
        out.extend_from_slice(rules_bytes);
        out
    }

    fn make_smart_rule_be(field_id: u32, unk_8: u32, child: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 56];
        out[0..4].copy_from_slice(&field_id.to_be_bytes());
        out[8..12].copy_from_slice(&unk_8.to_be_bytes());
        let child_size = u32::try_from(child.len()).expect("child_size");
        out[52..56].copy_from_slice(&child_size.to_be_bytes());
        out.extend_from_slice(child);
        out
    }

    fn build_missed_plma_hsma() -> Vec<u8> {
        let mut boma_str_payload = vec![0u8; 18];
        boma_str_payload[0..4].copy_from_slice(&2u32.to_le_bytes());
        boma_str_payload[4..8].copy_from_slice(&2u32.to_le_bytes());
        boma_str_payload[12..16].copy_from_slice(&1u32.to_le_bytes());
        boma_str_payload[16..18].copy_from_slice(b"hi");
        let boma_str = make_boma(0x0002, &[0xBB, 0, 0, 0], &boma_str_payload);

        let mut first_sub_buf = vec![0u8; 20];
        first_sub_buf[0..4].copy_from_slice(&1u32.to_le_bytes());
        let first_sub_boma = make_boma(0x01F6, &[0; 4], &first_sub_buf);

        let mut second_sub_buf = vec![0u8; 44];
        second_sub_buf[0..4].copy_from_slice(&1u32.to_le_bytes());
        second_sub_buf[24] = 2;
        let second_sub_boma = make_boma(0x01FF, &[0; 4], &second_sub_buf);

        let mut plma = vec![0u8; 160];
        plma[0..4].copy_from_slice(b"plma");
        plma[4..8].copy_from_slice(&160u32.to_le_bytes());
        plma[8..12].copy_from_slice(&3u32.to_le_bytes());
        plma.extend_from_slice(&boma_str);
        plma.extend_from_slice(&first_sub_boma);
        plma.extend_from_slice(&second_sub_boma);
        wrap_hsma(6, &plma)
    }

    fn build_missed_playlist_hsma() -> Vec<u8> {
        let mut smart_opts = vec![0u8; 112];
        smart_opts[15] = 1;
        let boma_opts = make_boma(0x00CA, &[0; 4], &smart_opts);

        let mut args = vec![0u8; 68];
        args[64..68].copy_from_slice(&1u32.to_be_bytes());
        let inner_rule = make_smart_rule_be(2, 2, &args);
        let inner_slst = make_slst_be(2, &inner_rule, 1);
        let outer_rule = make_smart_rule_be(0, 1, &inner_slst);
        let outer_slst = make_slst_be(1, &outer_rule, 1);
        let boma_slst = make_boma(0x00C9, &[0; 4], &outer_slst);

        let mut playlist_item_buf = vec![0u8; 324];
        playlist_item_buf[0..4].copy_from_slice(b"lpma");
        playlist_item_buf[4..8].copy_from_slice(&324u32.to_le_bytes());
        let lpma_total =
            u32::try_from(324 + boma_opts.len() + boma_slst.len()).expect("lpma total");
        playlist_item_buf[8..12].copy_from_slice(&lpma_total.to_le_bytes());
        playlist_item_buf[12..16].copy_from_slice(&2u32.to_le_bytes());
        playlist_item_buf[48] = 0x77;
        playlist_item_buf.extend_from_slice(&boma_opts);
        playlist_item_buf.extend_from_slice(&boma_slst);

        let mut playlist_list_buf = vec![0u8; 92];
        playlist_list_buf[0..4].copy_from_slice(b"lPma");
        playlist_list_buf[4..8].copy_from_slice(&92u32.to_le_bytes());
        playlist_list_buf[8..12].copy_from_slice(&1u32.to_le_bytes());
        playlist_list_buf.extend_from_slice(&playlist_item_buf);
        wrap_hsma(2, &playlist_list_buf)
    }

    fn build_missed_album_and_track_hsmas() -> (Vec<u8>, Vec<u8>) {
        let mut album_item_buf = vec![0u8; 104];
        album_item_buf[0..4].copy_from_slice(b"iama");
        album_item_buf[4..8].copy_from_slice(&104u32.to_le_bytes());
        album_item_buf[8..12].copy_from_slice(&104u32.to_le_bytes());
        album_item_buf[27] = 0x55;
        let mut album_list_buf = vec![0u8; 92];
        album_list_buf[0..4].copy_from_slice(b"lama");
        album_list_buf[4..8].copy_from_slice(&92u32.to_le_bytes());
        album_list_buf[8..12].copy_from_slice(&1u32.to_le_bytes());
        album_list_buf.extend_from_slice(&album_item_buf);

        let mut video_info = vec![0u8; 84];
        video_info[40..44].copy_from_slice(&9u32.to_le_bytes());
        let boma_video = make_boma(0x0024, &[0; 4], &video_info);
        let mut track_item_buf = vec![0u8; 380];
        track_item_buf[0..4].copy_from_slice(b"itma");
        track_item_buf[4..8].copy_from_slice(&380u32.to_le_bytes());
        let itma_total = u32::try_from(380 + boma_video.len()).expect("itma total");
        track_item_buf[8..12].copy_from_slice(&itma_total.to_le_bytes());
        track_item_buf[12..16].copy_from_slice(&1u32.to_le_bytes());
        track_item_buf[376..380].copy_from_slice(&[0xEE; 4]);
        track_item_buf.extend_from_slice(&boma_video);
        let mut track_list_buf = vec![0u8; 92];
        track_list_buf[0..4].copy_from_slice(b"ltma");
        track_list_buf[4..8].copy_from_slice(&92u32.to_le_bytes());
        track_list_buf[8..12].copy_from_slice(&1u32.to_le_bytes());
        track_list_buf.extend_from_slice(&track_item_buf);

        (wrap_hsma(4, &album_list_buf), wrap_hsma(1, &track_list_buf))
    }

    #[test]
    fn analyze_unknowns_reports_previously_missed_sections() {
        let master_section = build_missed_plma_hsma();
        let playlist_section = build_missed_playlist_hsma();
        let (album_section, track_section) = build_missed_album_and_track_hsmas();

        let mut file_bytes = vec![0u8; 164];
        file_bytes[0..4].copy_from_slice(b"hfma");
        file_bytes[4..8].copy_from_slice(&164u32.to_le_bytes());
        let file_total = u32::try_from(
            164 + master_section.len()
                + playlist_section.len()
                + album_section.len()
                + track_section.len(),
        )
        .expect("file total");
        file_bytes[8..12].copy_from_slice(&file_total.to_le_bytes());
        file_bytes[16..20].copy_from_slice(&4u32.to_le_bytes());
        file_bytes[160] = 0xAA;
        file_bytes.extend_from_slice(&master_section);
        file_bytes.extend_from_slice(&playlist_section);
        file_bytes.extend_from_slice(&album_section);
        file_bytes.extend_from_slice(&track_section);

        let raw = RawLibraryFile::from_raw_bytes(&file_bytes).expect("parse synthetic file");
        let report = analyze_unknowns(&raw);
        let keys: Vec<(&str, &str)> = report
            .nonzero_unknown_fields
            .iter()
            .map(|f| (f.section.as_str(), f.field_name.as_str()))
            .collect();

        for expected in [
            ("hfma(outer)", "extra_bytes"),
            ("plma::boma", "unk_16_end"),
            ("boma::str", "unk_12"),
            ("plma::0x01F6", "unk_0"),
            ("plma::0x01FF", "unk_0"),
            ("plma::0x01FF", "unk_24_44"),
            ("iama", "unk_27"),
            ("itma", "extra_bytes"),
            ("VideoInfo", "unk_40"),
            ("lpma", "unk_48"),
            ("SmartPlaylistOptions", "unk_15"),
            ("SLst", "unk_4"),
            ("SmartPlaylistRule", "unk_8"),
            ("SmartPlaylistRuleArguments", "unk_64"),
        ] {
            assert!(
                keys.contains(&expected),
                "expected {expected:?} in nonzero_unknown_fields, got {keys:?}"
            );
        }

        let slst_unk4 = report
            .nonzero_unknown_fields
            .iter()
            .find(|f| f.section == "SLst" && f.field_name == "unk_4")
            .expect("SLst.unk_4 summary");
        assert_eq!(slst_unk4.nonzero_instances, 2);

        let rule_unk8 = report
            .nonzero_unknown_fields
            .iter()
            .find(|f| f.section == "SmartPlaylistRule" && f.field_name == "unk_8")
            .expect("SmartPlaylistRule.unk_8 summary");
        assert_eq!(rule_unk8.nonzero_instances, 2);
    }
}
