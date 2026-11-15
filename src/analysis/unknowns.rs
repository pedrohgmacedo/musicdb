use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use musicdb::sections::{
    AlbumListSection, ArtistListSection, BomaPayload, BomaSection, HfmaHeader, HsmaChild,
    LibraryMasterSection, PlaylistItem, PlaylistListSection, RawLibraryFile,
    SocialProfileListSection, TrackItemHeader, TrackListSection, TrackNumerics, TrackPlaysSkips,
    VideoInfo,
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
    section: &'static str,
    field_name: &'static str,
    offset_info: &'static str,
    total: usize,
    nonzero: usize,
    counts: HashMap<String, usize>,
}

impl FieldTracker {
    fn new(section: &'static str, field_name: &'static str, offset_info: &'static str) -> Self {
        Self {
            section,
            field_name,
            offset_info,
            total: 0,
            nonzero: 0,
            counts: HashMap::new(),
        }
    }

    fn record_u8(&mut self, v: u8) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self.counts.entry(format!("{v} (0x{v:02X})")).or_insert(0) += 1;
        }
    }

    fn record_u16(&mut self, v: u16) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self.counts.entry(format!("{v} (0x{v:04X})")).or_insert(0) += 1;
        }
    }

    fn record_u32(&mut self, v: u32) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self.counts.entry(format!("{v} (0x{v:08X})")).or_insert(0) += 1;
        }
    }

    fn record_u64(&mut self, v: u64) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self.counts.entry(format!("0x{v:016X}")).or_insert(0) += 1;
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
            section: self.section.to_string(),
            field_name: self.field_name.to_string(),
            offset_info: self.offset_info.to_string(),
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
    map: HashMap<(&'static str, &'static str), usize>,
}

impl TrackerSet {
    fn with_tracker(
        &mut self,
        sec: &'static str,
        name: &'static str,
        off: &'static str,
        apply: impl FnOnce(&mut FieldTracker),
    ) {
        let idx = *self.map.entry((sec, name)).or_insert_with(|| {
            let i = self.trackers.len();
            self.trackers.push(FieldTracker::new(sec, name, off));
            i
        });
        if let Some(tracker) = self.trackers.get_mut(idx) {
            apply(tracker);
        }
    }

    fn u8(&mut self, sec: &'static str, name: &'static str, off: &'static str, val: u8) {
        self.with_tracker(sec, name, off, |t| t.record_u8(val));
    }

    fn u16(&mut self, sec: &'static str, name: &'static str, off: &'static str, val: u16) {
        self.with_tracker(sec, name, off, |t| t.record_u16(val));
    }

    fn u32(&mut self, sec: &'static str, name: &'static str, off: &'static str, val: u32) {
        self.with_tracker(sec, name, off, |t| t.record_u32(val));
    }

    fn u64(&mut self, sec: &'static str, name: &'static str, off: &'static str, val: u64) {
        self.with_tracker(sec, name, off, |t| t.record_u64(val));
    }

    fn bytes(&mut self, sec: &'static str, name: &'static str, off: &'static str, val: &[u8]) {
        self.with_tracker(sec, name, off, |t| t.record_bytes(val));
    }

    fn into_summaries(self) -> Vec<UnknownFieldSummary> {
        self.trackers
            .into_iter()
            .filter_map(FieldTracker::into_summary)
            .collect()
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

    fn collect_hfma(&mut self, label: &'static str, hdr: &HfmaHeader) {
        self.trackers.u32(label, "unk_60", "60..64", hdr.unk_60);
        self.trackers
            .bytes(label, "unk_64_flags", "64..68", &hdr.unk_64_flags);
        self.trackers.u32(label, "unk_104", "104..108", hdr.unk_104);
        self.trackers
            .bytes(label, "unk_116", "116..120", &hdr.unk_116);
        self.trackers
            .bytes(label, "unk_132_160", "132..160", &hdr.unk_132_160);
    }

    fn record_boma(&mut self, parent: &str, boma: &BomaSection) {
        record_boma_stats(
            parent,
            boma,
            &mut self.boma_subtype_counts,
            &mut self.unknown_boma_subtypes,
        );
    }

    fn collect_plma(&mut self, plma: &LibraryMasterSection) {
        let ph = &plma.header;
        self.trackers
            .bytes("plma", "unk_12_24", "12..24", &ph.unk_12_24);
        self.trackers
            .bytes("plma", "unk_25_52", "25..52", &ph.unk_25_52);
        self.trackers.u32("plma", "unk_52", "52..56", ph.unk_52);
        self.trackers
            .bytes("plma", "unk_56_58", "56..58", &ph.unk_56_58);
        self.trackers
            .bytes("plma", "unk_66_68", "66..68", &ph.unk_66_68);
        self.trackers.u32("plma", "unk_68", "68..72", ph.unk_68);
        self.trackers
            .bytes("plma", "unk_72_92", "72..92", &ph.unk_72_92);
        self.trackers.u32("plma", "unk_100", "100..104", ph.unk_100);
        self.trackers.u32(
            "plma",
            "unk_104_shared_val",
            "104..108",
            ph.unk_104_shared_val,
        );
        self.trackers
            .bytes("plma", "unk_108_128", "108..128", &ph.unk_108_128);
        self.trackers.u32("plma", "unk_128", "128..132", ph.unk_128);
        self.trackers.u32("plma", "unk_132", "132..136", ph.unk_132);
        self.trackers.u32("plma", "unk_136", "136..140", ph.unk_136);
        self.trackers.u32("plma", "unk_140", "140..144", ph.unk_140);
        self.trackers.u32("plma", "unk_144", "144..148", ph.unk_144);
        self.trackers
            .bytes("plma", "unk_149_160", "149..160", &ph.unk_149_160);
        self.trackers
            .bytes("plma", "unk_160_end", "160..end", &ph.unk_160_end);

        for boma in &plma.bomas {
            self.record_boma("plma", boma);
        }
    }

    fn collect_albums(&mut self, album_list: &AlbumListSection) {
        self.album_count = album_list.albums.len();
        self.trackers.bytes(
            "lama",
            "unk_12_end",
            "12..48",
            &album_list.header.unk_12_end,
        );
        for item in &album_list.albums {
            let hdr = &item.header;
            self.trackers.u8("iama", "unk_24", "24..25", hdr.unk_24);
            self.trackers
                .bytes("iama", "unk_26_28", "26..28", &hdr.unk_26_28);
            self.trackers.u32("iama", "unk_28", "28..32", hdr.unk_28);
            self.trackers
                .bytes("iama", "unk_44_64", "44..64", &hdr.unk_44_64);
            self.trackers
                .bytes("iama", "unk_72_96", "72..96", &hdr.unk_72_96);
            self.trackers
                .bytes("iama", "unk_tail", "128..end", &hdr.unk_tail);
            for boma in &item.bomas {
                self.record_boma("iama", boma);
            }
        }
    }

    fn collect_artists(&mut self, artist_list: &ArtistListSection) {
        self.artist_count = artist_list.artists.len();
        self.trackers.bytes(
            "lAma",
            "unk_12_end",
            "12..100",
            &artist_list.header.unk_12_end,
        );
        for item in &artist_list.artists {
            let hdr = &item.header;
            self.trackers.u32("iAma", "unk_24", "24..28", hdr.unk_24);
            self.trackers.u32("iAma", "unk_28", "28..32", hdr.unk_28);
            self.trackers
                .bytes("iAma", "unk_32_52", "32..52", &hdr.unk_32_52);
            self.trackers
                .bytes("iAma", "unk_88_96", "88..96", &hdr.unk_88_96);
            self.trackers.u8("iAma", "unk_100", "100..101", hdr.unk_100);
            self.trackers.u8("iAma", "unk_103", "103..104", hdr.unk_103);
            self.trackers
                .bytes("iAma", "unk_tail", "124..132", &hdr.unk_tail);
            for boma in &item.bomas {
                self.record_boma("iAma", boma);
            }
        }
    }

    fn collect_tracks(&mut self, ltma: &TrackListSection) {
        self.trackers
            .bytes("ltma", "unk_12_end", "12..92", &ltma.header.unk_12_end);
        for itma in &ltma.tracks {
            self.collect_itma_header(&itma.header);
            for boma in &itma.bomas {
                self.record_boma("itma", boma);
                match &boma.payload {
                    BomaPayload::TrackNumerics(num) => {
                        self.collect_track_numerics(&itma.header, num);
                    }
                    BomaPayload::TrackPlaysSkips(ps) => self.collect_track_plays_skips(ps),
                    BomaPayload::VideoInfo(vid) => self.collect_video_info(vid),
                    _ => {}
                }
            }
        }
    }

    fn collect_itma_header(&mut self, hdr: &TrackItemHeader) {
        self.trackers
            .bytes("itma", "unk_28_30", "28..30", &hdr.unk_28_30);
        self.trackers.u8(
            "itma",
            "unk_31_played_flag",
            "31..32",
            hdr.unk_31_played_flag,
        );
        self.trackers
            .bytes("itma", "unk_32_38", "32..38", &hdr.unk_32_38);
        self.trackers
            .bytes("itma", "unk_40_42", "40..42", &hdr.unk_40_42);
        self.trackers.u8("itma", "unk_43", "43..44", hdr.unk_43);
        self.trackers.u8("itma", "unk_45", "45..46", hdr.unk_45);
        self.trackers
            .bytes("itma", "unk_47_49", "47..49", &hdr.unk_47_49);
        self.trackers.u8("itma", "unk_54", "54..55", hdr.unk_54);
        self.trackers.u8("itma", "unk_56", "56..57", hdr.unk_56);
        self.trackers
            .bytes("itma", "unk_60_62", "60..62", &hdr.unk_60_62);
        self.trackers
            .u8("itma", "unk_64_store_flag", "64..65", hdr.unk_64_store_flag);
        self.trackers
            .bytes("itma", "unk_72_82", "72..82", &hdr.unk_72_82);
        self.trackers.u32(
            "itma",
            "unk_96_purchased_album_group",
            "96..100",
            hdr.unk_96_purchased_album_group,
        );
        self.trackers
            .bytes("itma", "unk_100_108", "100..108", &hdr.unk_100_108);
        self.trackers
            .u32("itma", "unk_112", "112..116", hdr.unk_112);
        self.trackers
            .bytes("itma", "unk_118_148", "118..148", &hdr.unk_118_148);
        self.trackers
            .u32("itma", "unk_156", "156..160", hdr.unk_156);
        self.trackers
            .bytes("itma", "unk_162_168", "162..168", &hdr.unk_162_168);
        self.trackers
            .bytes("itma", "unk_192_220", "192..220", &hdr.unk_192_220);
        self.trackers.u32(
            "itma",
            "unk_244_shared_val",
            "244..248",
            hdr.unk_244_shared_val,
        );
        self.trackers
            .u32("itma", "unk_248", "248..252", hdr.unk_248);
        self.trackers
            .u16("itma", "unk_252_254", "252..254", hdr.unk_252_254);
        self.trackers
            .u64("itma", "unk_312_320", "312..320", hdr.unk_312_320);
        self.trackers
            .u32("itma", "unk_328", "328..332", hdr.unk_328);
        self.trackers
            .u32("itma", "unk_332", "332..336", hdr.unk_332);
        self.trackers
            .u32("itma", "unk_340", "340..344", hdr.unk_340);
        self.trackers
            .bytes("itma", "unk_356_376", "356..376", &hdr.unk_356_376);
    }

    fn collect_track_numerics(&mut self, hdr: &TrackItemHeader, num: &TrackNumerics) {
        self.trackers
            .u32("TrackNumerics", "unk_16", "16..20", num.unk_16);
        self.trackers
            .bytes("TrackNumerics", "unk_20_flags", "20..24", &num.unk_20_flags);
        self.trackers
            .bytes("TrackNumerics", "unk_24_27", "24..27", &num.unk_24_27);
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
                num.unk_35_60[0],
            ],
        );
        self.trackers
            .bytes("TrackNumerics", "unk_35_60", "35..60", &num.unk_35_60);
        self.trackers.u32(
            "TrackNumerics",
            "unk_68_file_type",
            "68..72",
            num.unk_68_file_type,
        );
        self.trackers
            .bytes("TrackNumerics", "unk_96_108", "96..108", &num.unk_96_108);
        self.trackers
            .u32("TrackNumerics", "unk_152", "152..156", num.unk_152);
        self.trackers
            .bytes("TrackNumerics", "unk_176_200", "176..200", &num.unk_176_200);
        self.trackers.u32(
            "TrackNumerics",
            "unk_200_shared_val",
            "200..204",
            num.unk_200_shared_val,
        );
        self.trackers
            .u32("TrackNumerics", "unk_204", "204..208", num.unk_204);
        self.trackers
            .bytes("TrackNumerics", "unk_216_256", "216..256", &num.unk_216_256);
        self.trackers
            .u32("TrackNumerics", "unk_268_272", "268..272", num.unk_268_272);
        self.trackers.u32(
            "TrackNumerics",
            "unk_280_shared_val",
            "280..284",
            num.unk_280_shared_val,
        );
        self.trackers
            .u32("TrackNumerics", "unk_284_288", "284..288", num.unk_284_288);
        self.trackers
            .u32("TrackNumerics", "unk_300", "300..304", num.unk_300);
        self.trackers
            .u32("TrackNumerics", "unk_316_320", "316..320", num.unk_316_320);
        self.trackers
            .u32("TrackNumerics", "unk_336_340", "336..340", num.unk_336_340);
        self.trackers
            .u32("TrackNumerics", "unk_360", "360..364", num.unk_360);
    }

    fn collect_track_plays_skips(&mut self, ps: &TrackPlaysSkips) {
        self.trackers
            .u32("TrackPlaysSkips", "unk_24", "24..28", ps.unk_24);
        self.trackers
            .u32("TrackPlaysSkips", "unk_40", "40..44", ps.unk_40);
        self.trackers
            .bytes("TrackPlaysSkips", "unk_44_52", "44..52", &ps.unk_44_52);
    }

    fn collect_video_info(&mut self, vid: &VideoInfo) {
        self.trackers.u32("VideoInfo", "unk_8", "8..12", vid.unk_8);
        self.trackers
            .u32("VideoInfo", "unk_16", "16..20", vid.unk_16);
        self.trackers
            .u32("VideoInfo", "unk_20", "20..24", vid.unk_20);
        self.trackers
            .u32("VideoInfo", "unk_24", "24..28", vid.unk_24);
        self.trackers
            .u32("VideoInfo", "unk_28", "28..32", vid.unk_28);
        self.trackers
            .u32("VideoInfo", "unk_32", "32..36", vid.unk_32);
        self.trackers
            .u32("VideoInfo", "unk_36", "36..40", vid.unk_36);
        self.trackers
            .u32("VideoInfo", "unk_44", "44..48", vid.unk_44);
        self.trackers
            .u32("VideoInfo", "unk_48", "48..52", vid.unk_48);
        self.trackers
            .bytes("VideoInfo", "unk_56_84", "56..84", &vid.unk_56_84);
    }

    fn collect_playlists(&mut self, playlist_list: &PlaylistListSection) {
        self.trackers.bytes(
            "lPma",
            "unk_12_end",
            "12..92",
            &playlist_list.header.unk_12_end,
        );
        for lpma in &playlist_list.playlists {
            self.collect_lpma_header(lpma);
            self.collect_playlist_bomas(lpma);
        }
    }

    fn collect_lpma_header(&mut self, lpma: &PlaylistItem) {
        let hdr = &lpma.header;
        if self.first_lpma_gc.is_none() {
            self.first_lpma_gc = Some(hdr.global_counter);
        }
        self.trackers
            .bytes("lpma", "unk_20_22", "20..22", &hdr.unk_20_22);
        self.trackers
            .bytes("lpma", "unk_38_44", "38..44", &hdr.unk_38_44);
        self.trackers.u8("lpma", "unk_45", "45..46", hdr.unk_45);
        self.trackers
            .u16("lpma", "unk_46_smart_flag", "46..48", hdr.unk_46_smart_flag);
        self.trackers.bytes(
            "lpma",
            "unk_48_folder_flag",
            "48..50",
            &hdr.unk_48_folder_flag,
        );
        self.trackers
            .bytes("lpma", "unk_58_70", "58..70", &hdr.unk_58_70);
        self.trackers
            .bytes("lpma", "unk_82_138", "82..138", &hdr.unk_82_138);
        self.trackers
            .bytes("lpma", "unk_142_174", "142..174", &hdr.unk_142_174);
        self.trackers
            .bytes("lpma", "unk_186_flags", "186..190", &hdr.unk_186_flags);
        self.trackers
            .u32("lpma", "unk_192", "192..196", hdr.unk_192);
        self.trackers
            .bytes("lpma", "unk_204_214", "204..214", &hdr.unk_204_214);
        self.trackers.u8("lpma", "unk_222", "222..223", hdr.unk_222);
        self.trackers
            .bytes("lpma", "unk_225_256", "225..256", &hdr.unk_225_256);
        self.trackers
            .bytes("lpma", "unk_260_262", "260..262", &hdr.unk_260_262);
        self.trackers.u8("lpma", "unk_279", "279..280", hdr.unk_279);
        self.trackers
            .bytes("lpma", "unk_288_296", "288..296", &hdr.unk_288_296);
        self.trackers
            .u32("lpma", "unk_312", "312..316", hdr.unk_312);
        if let Some(bytes) = &hdr.unk_328_340 {
            self.trackers
                .bytes("lpma", "unk_328_340", "328..340", bytes);
        }
        if let Some(val) = hdr.unk_348_356 {
            self.trackers.u64("lpma", "unk_348_356", "348..356", val);
        }
        if let Some(val) = hdr.unk_360_368 {
            self.trackers.u64("lpma", "unk_360_368", "360..368", val);
        }
        self.trackers
            .bytes("lpma", "unk_tail", "368..end", &hdr.unk_tail);
    }

    fn collect_playlist_bomas(&mut self, lpma: &PlaylistItem) {
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
                    if ipfa.unk_40_counter == itma_gc {
                        self.matched_itma_global_counter += 1;
                    } else if self.mismatched_examples.len() < 10 {
                        self.mismatched_examples.push(format!(
                            "track {:016X}: ipfa.unk_40={} != itma.global_counter={itma_gc}",
                            ipfa.track_persistent_id, ipfa.unk_40_counter
                        ));
                    }
                }
                self.trackers
                    .bytes("ipfa", "unk_28_40", "28..40", &ipfa.unk_28_40);
                self.trackers
                    .bytes("ipfa", "unk_52_end", "52..68", &ipfa.unk_52_end);
            } else if let BomaPayload::PlaylistItemCompact(ipfa) = &boma.payload {
                self.total_ipfa_items += 1;
                if self
                    .track_short_id_by_persistent_id
                    .contains_key(&ipfa.track_persistent_id)
                {
                    self.matched_track_persistent_id += 1;
                }
                self.trackers
                    .bytes("IPfa", "unk_24_end", "24..32", &ipfa.unk_24_end);
            }
        }
    }

    fn collect_social_profiles(&mut self, lsma: &SocialProfileListSection) {
        self.trackers
            .bytes("Lsma", "unk_12_end", "12..92", &lsma.header.unk_12_end);
        for isma in &lsma.profiles {
            let hdr = &isma.header;
            self.trackers
                .u64("isma", "unk_24_32", "24..32", hdr.unk_24_32);
            self.trackers.u32("isma", "unk_52", "52..56", hdr.unk_52);
            self.trackers
                .bytes("isma", "unk_64_end", "64..136", &hdr.unk_64_end);
            for boma in &isma.bomas {
                self.record_boma("isma", boma);
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
    collector.collect_hfma("hfma(outer)", &raw.outer_header);
    collector.index_tracks(raw);

    for hsma in &raw.sections {
        collector
            .trackers
            .bytes("hsma", "unk_24_end", "24..56", &hsma.header.unk_24_end);

        match &hsma.child {
            HsmaChild::InnerHfma(inner) => collector.collect_hfma("hfma(inner)", inner),
            HsmaChild::LibraryMaster(plma) => collector.collect_plma(plma),
            HsmaChild::AlbumList(lama) => collector.collect_albums(lama),
            HsmaChild::ArtistList(artist_list) => collector.collect_artists(artist_list),
            HsmaChild::TrackList(ltma) => collector.collect_tracks(ltma),
            HsmaChild::PlaylistList(playlist_list) => collector.collect_playlists(playlist_list),
            HsmaChild::SocialProfileList(lsma) => collector.collect_social_profiles(lsma),
            HsmaChild::Padding(lpma_pad) => {
                collector
                    .trackers
                    .bytes("LPma", "raw_bytes", "8..end", &lpma_pad.raw_bytes);
            }
            HsmaChild::Unknown {
                subtype,
                signature,
                raw_bytes,
            } => {
                collector.unknown_hsma_sections.push((
                    *subtype,
                    String::from_utf8_lossy(signature).into_owned(),
                    raw_bytes.len(),
                ));
            }
            HsmaChild::Skipped { .. } | _ => {}
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
