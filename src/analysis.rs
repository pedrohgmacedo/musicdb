use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use plist::Value as PlistValue;
use serde::{Deserialize, Serialize};

use crate::model::format_persistent_id;
use crate::sections::{BomaPayload, HsmaChild, ItmaSection, RawLibraryFile};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownFieldSummary {
    pub section: String,
    pub field_name: String,
    pub offset_info: String,
    pub total_instances: usize,
    pub nonzero_instances: usize,
    pub distinct_nonzero_count: usize,
    pub sample_values: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownsReport {
    pub unknown_hsma_sections: Vec<(u32, String, usize)>,
    pub unknown_boma_subtypes: BTreeMap<String, usize>,
    pub boma_subtype_counts: BTreeMap<String, usize>,
    pub ipfa_short_id_verification: IpfaShortIdCheck,
    pub global_counter_analysis: GlobalCounterAnalysis,
    pub nonzero_unknown_fields: Vec<UnknownFieldSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfaShortIdCheck {
    pub total_ipfa_items: usize,
    pub matched_track_persistent_id: usize,
    pub matched_itma_global_counter: usize,
    pub mismatched_examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalCounterAnalysis {
    pub album_count: usize,
    pub artist_count: usize,
    pub first_itma_global_counter: Option<u32>,
    pub last_itma_global_counter: Option<u32>,
    pub first_lpma_global_counter: Option<u32>,
    pub last_ipfa_global_counter: Option<u32>,
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
            *self
                .counts
                .entry(format!("{} (0x{:02X})", v, v))
                .or_insert(0) += 1;
        }
    }

    fn record_u16(&mut self, v: u16) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self
                .counts
                .entry(format!("{} (0x{:04X})", v, v))
                .or_insert(0) += 1;
        }
    }

    #[allow(dead_code)]
    fn record_i16(&mut self, v: i16) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self
                .counts
                .entry(format!("{} (0x{:04X})", v, v as u16))
                .or_insert(0) += 1;
        }
    }

    fn record_u32(&mut self, v: u32) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self
                .counts
                .entry(format!("{} (0x{:08X})", v, v))
                .or_insert(0) += 1;
        }
    }

    fn record_u64(&mut self, v: u64) {
        self.total += 1;
        if v != 0 {
            self.nonzero += 1;
            *self.counts.entry(format!("0x{:016X}", v)).or_insert(0) += 1;
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
        pairs.truncate(8);
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

/// Scans a `RawLibraryFile` for all unknown byte regions, unhandled `boma` subtypes,
/// and verifies structural invariants (such as `ipfa` offset 40 == `itma` `global_counter`).
pub fn analyze_unknowns(raw: &RawLibraryFile) -> UnknownsReport {
    let mut unknown_hsma_sections = Vec::new();
    let mut unknown_boma_subtypes: BTreeMap<String, usize> = BTreeMap::new();
    let mut boma_subtype_counts: BTreeMap<String, usize> = BTreeMap::new();

    let mut trackers: Vec<FieldTracker> = Vec::new();
    let mut tracker_map: HashMap<(&'static str, &'static str), usize> = HashMap::new();

    macro_rules! track {
        ($sec:expr, $name:expr, $off:expr, $method:ident, $val:expr) => {{
            let idx = *tracker_map.entry(($sec, $name)).or_insert_with(|| {
                let i = trackers.len();
                trackers.push(FieldTracker::new($sec, $name, $off));
                i
            });
            trackers[idx].$method($val);
        }};
    }

    // Outer hfma
    let oh = &raw.outer_header;
    track!("hfma(outer)", "unk_60", "60..64", record_u32, oh.unk_60);
    track!(
        "hfma(outer)",
        "unk_64_flags",
        "64..68",
        record_bytes,
        &oh.unk_64_flags
    );
    track!("hfma(outer)", "unk_104", "104..108", record_u32, oh.unk_104);
    track!(
        "hfma(outer)",
        "unk_116",
        "116..120",
        record_bytes,
        &oh.unk_116
    );
    track!(
        "hfma(outer)",
        "unk_132_160",
        "132..160",
        record_bytes,
        &oh.unk_132_160
    );

    let mut track_short_id_by_persistent_id: HashMap<u64, u32> = HashMap::new();
    let mut album_count = 0usize;
    let mut artist_count = 0usize;
    let mut first_itma_gc = None;
    let mut last_itma_gc = None;
    let mut first_lpma_gc = None;
    let mut last_ipfa_gc = None;

    // First pass to collect track `global_counter` by `track_persistent_id`
    for hsma in &raw.sections {
        if let HsmaChild::TrackList(ltma) = &hsma.child {
            for itma in &ltma.tracks {
                track_short_id_by_persistent_id
                    .insert(itma.header.track_persistent_id, itma.header.global_counter);
                if first_itma_gc.is_none() {
                    first_itma_gc = Some(itma.header.global_counter);
                }
                last_itma_gc = Some(itma.header.global_counter);
            }
        }
    }

    let mut total_ipfa_items = 0usize;
    let mut matched_track_persistent_id = 0usize;
    let mut matched_itma_global_counter = 0usize;
    let mut mismatched_examples = Vec::new();

    for hsma in &raw.sections {
        track!(
            "hsma",
            "unk_24_end",
            "24..56",
            record_bytes,
            &hsma.header.unk_24_end
        );

        match &hsma.child {
            HsmaChild::InnerHfma(ih) => {
                track!("hfma(inner)", "unk_60", "60..64", record_u32, ih.unk_60);
                track!(
                    "hfma(inner)",
                    "unk_64_flags",
                    "64..68",
                    record_bytes,
                    &ih.unk_64_flags
                );
                track!("hfma(inner)", "unk_104", "104..108", record_u32, ih.unk_104);
                track!(
                    "hfma(inner)",
                    "unk_116",
                    "116..120",
                    record_bytes,
                    &ih.unk_116
                );
                track!(
                    "hfma(inner)",
                    "unk_132_160",
                    "132..160",
                    record_bytes,
                    &ih.unk_132_160
                );
            }
            HsmaChild::LibraryMaster(plma) => {
                let ph = &plma.header;
                track!("plma", "unk_12_24", "12..24", record_bytes, &ph.unk_12_24);
                track!("plma", "unk_25_52", "25..52", record_bytes, &ph.unk_25_52);
                track!("plma", "unk_52", "52..56", record_u32, ph.unk_52);
                track!("plma", "unk_56_58", "56..58", record_bytes, &ph.unk_56_58);
                track!("plma", "unk_66_68", "66..68", record_bytes, &ph.unk_66_68);
                track!("plma", "unk_68", "68..72", record_u32, ph.unk_68);
                track!("plma", "unk_72_92", "72..92", record_bytes, &ph.unk_72_92);
                track!("plma", "unk_100", "100..104", record_u32, ph.unk_100);
                track!(
                    "plma",
                    "unk_104_shared_val",
                    "104..108",
                    record_u32,
                    ph.unk_104_shared_val
                );
                track!(
                    "plma",
                    "unk_108_128",
                    "108..128",
                    record_bytes,
                    &ph.unk_108_128
                );
                track!("plma", "unk_128", "128..132", record_u32, ph.unk_128);
                track!("plma", "unk_132", "132..136", record_u32, ph.unk_132);
                track!("plma", "unk_136", "136..140", record_u32, ph.unk_136);
                track!("plma", "unk_140", "140..144", record_u32, ph.unk_140);
                track!("plma", "unk_144", "144..148", record_u32, ph.unk_144);
                track!(
                    "plma",
                    "unk_149_160",
                    "149..160",
                    record_bytes,
                    &ph.unk_149_160
                );
                track!(
                    "plma",
                    "unk_160_end",
                    "160..end",
                    record_bytes,
                    &ph.unk_160_end
                );

                for boma in &plma.bomas {
                    record_boma_stats(
                        "plma",
                        boma,
                        &mut boma_subtype_counts,
                        &mut unknown_boma_subtypes,
                    );
                }
            }
            HsmaChild::AlbumList(lama) => {
                album_count = lama.albums.len();
                track!(
                    "lama",
                    "unk_12_end",
                    "12..48",
                    record_bytes,
                    &lama.header.unk_12_end
                );
                for iama in &lama.albums {
                    let h = &iama.header;
                    track!("iama", "unk_24", "24..25", record_u8, h.unk_24);
                    track!("iama", "unk_26_28", "26..28", record_bytes, &h.unk_26_28);
                    track!("iama", "unk_28", "28..32", record_u32, h.unk_28);
                    track!("iama", "unk_44_64", "44..64", record_bytes, &h.unk_44_64);
                    track!("iama", "unk_72_96", "72..96", record_bytes, &h.unk_72_96);
                    track!("iama", "unk_tail", "128..end", record_bytes, &h.unk_tail);
                    for boma in &iama.bomas {
                        record_boma_stats(
                            "iama",
                            boma,
                            &mut boma_subtype_counts,
                            &mut unknown_boma_subtypes,
                        );
                    }
                }
            }
            HsmaChild::ArtistList(l_ama) => {
                artist_count = l_ama.artists.len();
                track!(
                    "lAma",
                    "unk_12_end",
                    "12..100",
                    record_bytes,
                    &l_ama.header.unk_12_end
                );
                for i_ama in &l_ama.artists {
                    let h = &i_ama.header;
                    track!("iAma", "unk_24", "24..28", record_u32, h.unk_24);
                    track!("iAma", "unk_28", "28..32", record_u32, h.unk_28);
                    track!("iAma", "unk_32_52", "32..52", record_bytes, &h.unk_32_52);
                    track!("iAma", "unk_88_96", "88..96", record_bytes, &h.unk_88_96);
                    track!("iAma", "unk_100", "100..101", record_u8, h.unk_100);
                    track!("iAma", "unk_103", "103..104", record_u8, h.unk_103);
                    track!("iAma", "unk_tail", "124..132", record_bytes, &h.unk_tail);
                    for boma in &i_ama.bomas {
                        record_boma_stats(
                            "iAma",
                            boma,
                            &mut boma_subtype_counts,
                            &mut unknown_boma_subtypes,
                        );
                    }
                }
            }
            HsmaChild::TrackList(ltma) => {
                track!(
                    "ltma",
                    "unk_12_end",
                    "12..92",
                    record_bytes,
                    &ltma.header.unk_12_end
                );
                for itma in &ltma.tracks {
                    let h = &itma.header;
                    track!("itma", "unk_28_30", "28..30", record_bytes, &h.unk_28_30);
                    track!(
                        "itma",
                        "unk_31_played_flag",
                        "31..32",
                        record_u8,
                        h.unk_31_played_flag
                    );
                    track!("itma", "unk_32_38", "32..38", record_bytes, &h.unk_32_38);
                    track!("itma", "unk_40_42", "40..42", record_bytes, &h.unk_40_42);
                    track!("itma", "unk_43", "43..44", record_u8, h.unk_43);
                    track!("itma", "unk_45", "45..46", record_u8, h.unk_45);
                    track!("itma", "unk_47_49", "47..49", record_bytes, &h.unk_47_49);
                    track!("itma", "unk_54", "54..55", record_u8, h.unk_54);
                    track!("itma", "unk_56", "56..57", record_u8, h.unk_56);
                    track!("itma", "unk_60_62", "60..62", record_bytes, &h.unk_60_62);
                    track!(
                        "itma",
                        "unk_64_store_flag",
                        "64..65",
                        record_u8,
                        h.unk_64_store_flag
                    );
                    track!("itma", "unk_72_82", "72..82", record_bytes, &h.unk_72_82);
                    track!(
                        "itma",
                        "unk_96_purchased_album_group",
                        "96..100",
                        record_u32,
                        h.unk_96_purchased_album_group
                    );
                    track!(
                        "itma",
                        "unk_100_108",
                        "100..108",
                        record_bytes,
                        &h.unk_100_108
                    );
                    track!("itma", "unk_112", "112..116", record_u32, h.unk_112);
                    track!(
                        "itma",
                        "unk_118_148",
                        "118..148",
                        record_bytes,
                        &h.unk_118_148
                    );
                    track!("itma", "unk_156", "156..160", record_u32, h.unk_156);
                    track!(
                        "itma",
                        "unk_162_168",
                        "162..168",
                        record_bytes,
                        &h.unk_162_168
                    );
                    track!(
                        "itma",
                        "unk_192_220",
                        "192..220",
                        record_bytes,
                        &h.unk_192_220
                    );
                    track!(
                        "itma",
                        "unk_244_shared_val",
                        "244..248",
                        record_u32,
                        h.unk_244_shared_val
                    );
                    track!("itma", "unk_248", "248..252", record_u32, h.unk_248);
                    track!("itma", "unk_252_254", "252..254", record_u16, h.unk_252_254);
                    track!("itma", "unk_312_320", "312..320", record_u64, h.unk_312_320);
                    track!("itma", "unk_328", "328..332", record_u32, h.unk_328);
                    track!("itma", "unk_332", "332..336", record_u32, h.unk_332);
                    track!("itma", "unk_340", "340..344", record_u32, h.unk_340);
                    track!(
                        "itma",
                        "unk_356_376",
                        "356..376",
                        record_bytes,
                        &h.unk_356_376
                    );

                    for boma in &itma.bomas {
                        record_boma_stats(
                            "itma",
                            boma,
                            &mut boma_subtype_counts,
                            &mut unknown_boma_subtypes,
                        );
                        match &boma.payload {
                            BomaPayload::TrackNumerics(n) => {
                                track!("TrackNumerics", "unk_16", "16..20", record_u32, n.unk_16);
                                track!(
                                    "TrackNumerics",
                                    "unk_20_flags",
                                    "20..24",
                                    record_bytes,
                                    &n.unk_20_flags
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_24_27",
                                    "24..27",
                                    record_bytes,
                                    &n.unk_24_27
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_35_60",
                                    "35..60",
                                    record_bytes,
                                    &n.unk_35_60
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_68_file_type",
                                    "68..72",
                                    record_u32,
                                    n.unk_68_file_type
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_96_108",
                                    "96..108",
                                    record_bytes,
                                    &n.unk_96_108
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_152",
                                    "152..156",
                                    record_u32,
                                    n.unk_152
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_176_200",
                                    "176..200",
                                    record_bytes,
                                    &n.unk_176_200
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_200_shared_val",
                                    "200..204",
                                    record_u32,
                                    n.unk_200_shared_val
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_204",
                                    "204..208",
                                    record_u32,
                                    n.unk_204
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_216_256",
                                    "216..256",
                                    record_bytes,
                                    &n.unk_216_256
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_268_272",
                                    "268..272",
                                    record_u32,
                                    n.unk_268_272
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_280_shared_val",
                                    "280..284",
                                    record_u32,
                                    n.unk_280_shared_val
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_284_288",
                                    "284..288",
                                    record_u32,
                                    n.unk_284_288
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_300",
                                    "300..304",
                                    record_u32,
                                    n.unk_300
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_316_320",
                                    "316..320",
                                    record_u32,
                                    n.unk_316_320
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_336_340",
                                    "336..340",
                                    record_u32,
                                    n.unk_336_340
                                );
                                track!(
                                    "TrackNumerics",
                                    "unk_360",
                                    "360..364",
                                    record_u32,
                                    n.unk_360
                                );
                            }
                            BomaPayload::TrackPlaysSkips(ps) => {
                                track!(
                                    "TrackPlaysSkips",
                                    "unk_24",
                                    "24..28",
                                    record_u32,
                                    ps.unk_24
                                );
                                track!(
                                    "TrackPlaysSkips",
                                    "unk_40",
                                    "40..44",
                                    record_u32,
                                    ps.unk_40
                                );
                                track!(
                                    "TrackPlaysSkips",
                                    "unk_44_52",
                                    "44..52",
                                    record_bytes,
                                    &ps.unk_44_52
                                );
                            }
                            BomaPayload::VideoInfo(v) => {
                                track!("VideoInfo", "unk_8", "8..12", record_u32, v.unk_8);
                                track!("VideoInfo", "unk_16", "16..20", record_u32, v.unk_16);
                                track!("VideoInfo", "unk_20", "20..24", record_u32, v.unk_20);
                                track!("VideoInfo", "unk_24", "24..28", record_u32, v.unk_24);
                                track!("VideoInfo", "unk_28", "28..32", record_u32, v.unk_28);
                                track!("VideoInfo", "unk_32", "32..36", record_u32, v.unk_32);
                                track!("VideoInfo", "unk_36", "36..40", record_u32, v.unk_36);
                                track!("VideoInfo", "unk_44", "44..48", record_u32, v.unk_44);
                                track!("VideoInfo", "unk_48", "48..52", record_u32, v.unk_48);
                                track!(
                                    "VideoInfo",
                                    "unk_56_84",
                                    "56..84",
                                    record_bytes,
                                    &v.unk_56_84
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
            HsmaChild::PlaylistList(l_pma) => {
                track!(
                    "lPma",
                    "unk_12_end",
                    "12..92",
                    record_bytes,
                    &l_pma.header.unk_12_end
                );
                for lpma in &l_pma.playlists {
                    let h = &lpma.header;
                    if first_lpma_gc.is_none() {
                        first_lpma_gc = Some(h.global_counter);
                    }
                    track!("lpma", "unk_20_22", "20..22", record_bytes, &h.unk_20_22);
                    track!("lpma", "unk_38_44", "38..44", record_bytes, &h.unk_38_44);
                    track!("lpma", "unk_45", "45..46", record_u8, h.unk_45);
                    track!(
                        "lpma",
                        "unk_46_smart_flag",
                        "46..48",
                        record_u16,
                        h.unk_46_smart_flag
                    );
                    track!(
                        "lpma",
                        "unk_48_folder_flag",
                        "48..50",
                        record_bytes,
                        &h.unk_48_folder_flag
                    );
                    track!("lpma", "unk_58_70", "58..70", record_bytes, &h.unk_58_70);
                    track!("lpma", "unk_82_138", "82..138", record_bytes, &h.unk_82_138);
                    track!(
                        "lpma",
                        "unk_142_174",
                        "142..174",
                        record_bytes,
                        &h.unk_142_174
                    );
                    track!(
                        "lpma",
                        "unk_186_flags",
                        "186..190",
                        record_bytes,
                        &h.unk_186_flags
                    );
                    track!("lpma", "unk_192", "192..196", record_u32, h.unk_192);
                    track!(
                        "lpma",
                        "unk_204_214",
                        "204..214",
                        record_bytes,
                        &h.unk_204_214
                    );
                    track!("lpma", "unk_222", "222..223", record_u8, h.unk_222);
                    track!(
                        "lpma",
                        "unk_225_256",
                        "225..256",
                        record_bytes,
                        &h.unk_225_256
                    );
                    track!(
                        "lpma",
                        "unk_260_262",
                        "260..262",
                        record_bytes,
                        &h.unk_260_262
                    );
                    track!("lpma", "unk_279", "279..280", record_u8, h.unk_279);
                    track!(
                        "lpma",
                        "unk_288_296",
                        "288..296",
                        record_bytes,
                        &h.unk_288_296
                    );
                    track!("lpma", "unk_312", "312..316", record_u32, h.unk_312);
                    if let Some(b) = &h.unk_328_340 {
                        track!("lpma", "unk_328_340", "328..340", record_bytes, b);
                    }
                    if let Some(v) = h.unk_348_356 {
                        track!("lpma", "unk_348_356", "348..356", record_u64, v);
                    }
                    if let Some(v) = h.unk_360_368 {
                        track!("lpma", "unk_360_368", "360..368", record_u64, v);
                    }
                    track!("lpma", "unk_tail", "368..end", record_bytes, &h.unk_tail);

                    for boma in &lpma.bomas {
                        record_boma_stats(
                            "lpma",
                            boma,
                            &mut boma_subtype_counts,
                            &mut unknown_boma_subtypes,
                        );
                        if let BomaPayload::PlaylistItem(ipfa) = &boma.payload {
                            last_ipfa_gc = Some(ipfa.global_counter);
                            total_ipfa_items += 1;
                            if let Some(&itma_gc) =
                                track_short_id_by_persistent_id.get(&ipfa.track_persistent_id)
                            {
                                matched_track_persistent_id += 1;
                                if ipfa.unk_40_counter == itma_gc {
                                    matched_itma_global_counter += 1;
                                } else if mismatched_examples.len() < 10 {
                                    mismatched_examples.push(format!(
                                        "track {:016X}: ipfa.unk_40={} != itma.global_counter={}",
                                        ipfa.track_persistent_id, ipfa.unk_40_counter, itma_gc
                                    ));
                                }
                            }
                            track!("ipfa", "unk_28_40", "28..40", record_bytes, &ipfa.unk_28_40);
                            track!(
                                "ipfa",
                                "unk_52_end",
                                "52..68",
                                record_bytes,
                                &ipfa.unk_52_end
                            );
                        } else if let BomaPayload::PlaylistItemCompact(ipfa) = &boma.payload {
                            total_ipfa_items += 1;
                            if track_short_id_by_persistent_id
                                .contains_key(&ipfa.track_persistent_id)
                            {
                                matched_track_persistent_id += 1;
                            }
                            track!(
                                "IPfa",
                                "unk_24_end",
                                "24..32",
                                record_bytes,
                                &ipfa.unk_24_end
                            );
                        }
                    }
                }
            }
            HsmaChild::SocialProfileList(lsma) => {
                track!(
                    "Lsma",
                    "unk_12_end",
                    "12..92",
                    record_bytes,
                    &lsma.header.unk_12_end
                );
                for isma in &lsma.profiles {
                    let h = &isma.header;
                    track!("isma", "unk_24_32", "24..32", record_u64, h.unk_24_32);
                    track!("isma", "unk_52", "52..56", record_u32, h.unk_52);
                    track!("isma", "unk_64_end", "64..136", record_bytes, &h.unk_64_end);
                    for boma in &isma.bomas {
                        record_boma_stats(
                            "isma",
                            boma,
                            &mut boma_subtype_counts,
                            &mut unknown_boma_subtypes,
                        );
                    }
                }
            }
            HsmaChild::Padding(lpma_pad) => {
                track!(
                    "LPma",
                    "raw_bytes",
                    "8..end",
                    record_bytes,
                    &lpma_pad.raw_bytes
                );
            }
            HsmaChild::Unknown {
                subtype,
                signature,
                raw_bytes,
            } => {
                unknown_hsma_sections.push((
                    *subtype,
                    String::from_utf8_lossy(signature).into_owned(),
                    raw_bytes.len(),
                ));
            }
        }
    }

    let nonzero_unknown_fields = trackers
        .into_iter()
        .filter_map(FieldTracker::into_summary)
        .collect();

    UnknownsReport {
        unknown_hsma_sections,
        unknown_boma_subtypes,
        boma_subtype_counts,
        ipfa_short_id_verification: IpfaShortIdCheck {
            total_ipfa_items,
            matched_track_persistent_id,
            matched_itma_global_counter,
            mismatched_examples,
        },
        global_counter_analysis: GlobalCounterAnalysis {
            album_count,
            artist_count,
            first_itma_global_counter: first_itma_gc,
            last_itma_global_counter: last_itma_gc,
            first_lpma_global_counter: first_lpma_gc,
            last_ipfa_global_counter: last_ipfa_gc,
        },
        nonzero_unknown_fields,
    }
}

fn record_boma_stats(
    parent: &str,
    boma: &crate::sections::BomaSection,
    subtype_counts: &mut BTreeMap<String, usize>,
    unknown_subtypes: &mut BTreeMap<String, usize>,
) {
    let key = format!(
        "{}::0x{:04X} ({})",
        parent,
        boma.subtype,
        boma.subtype_name()
    );
    *subtype_counts.entry(key.clone()).or_insert(0) += 1;
    if matches!(boma.payload, BomaPayload::Unknown { .. }) {
        *unknown_subtypes.entry(key).or_insert(0) += 1;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDiffReport {
    pub hfma_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub hsma_10_hex_a: Option<String>,
    pub hsma_10_hex_b: Option<String>,
    pub plma_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub plma_boma_changes: Vec<String>,
    pub tracks_in_a: usize,
    pub tracks_in_b: usize,
    pub common_tracks: usize,
    pub added_tracks: Vec<String>,
    pub removed_tracks: Vec<String>,
    pub itma_offset_change_frequencies: BTreeMap<usize, usize>,
    pub track_numerics_offset_change_frequencies: BTreeMap<usize, usize>,
    pub sample_track_diffs: Vec<TrackDiffSample>,
    pub album_diffs: Vec<String>,
    pub artist_diffs: Vec<String>,
    pub playlist_diffs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackDiffSample {
    pub persistent_id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub high_level_changes: Vec<String>,
    pub itma_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub numerics_changed_4byte_offsets: Vec<(usize, u32, u32)>,
    pub plays_skips_changed_4byte_offsets: Vec<(usize, u32, u32)>,
}

fn diff_4byte_words<T: binrw::BinWrite>(
    a: &T,
    b: &T,
    ignore_offsets: &[usize],
) -> Vec<(usize, u32, u32)>
where
    for<'a> T::Args<'a>: Default,
{
    let mut cur_a = std::io::Cursor::new(Vec::new());
    let mut cur_b = std::io::Cursor::new(Vec::new());
    let _ = binrw::BinWrite::write_le(a, &mut cur_a);
    let _ = binrw::BinWrite::write_le(b, &mut cur_b);
    let bytes_a = cur_a.into_inner();
    let bytes_b = cur_b.into_inner();
    let min_len = bytes_a.len().min(bytes_b.len());
    let mut out = Vec::new();
    for off in (0..min_len).step_by(4) {
        if ignore_offsets.contains(&off) {
            continue;
        }
        if off + 4 <= min_len {
            let va = u32::from_le_bytes(bytes_a[off..off + 4].try_into().unwrap());
            let vb = u32::from_le_bytes(bytes_b[off..off + 4].try_into().unwrap());
            if va != vb {
                out.push((off, va, vb));
            }
        }
    }
    out
}

/// Compares two `RawLibraryFile` snapshots (e.g. 2024 vs 2025 or 2025 vs 2026) at both the high-level
/// and 4-byte word offset levels for common tracks, albums, artists, playlists, and headers.
pub fn diff_libraries(raw_a: &RawLibraryFile, raw_b: &RawLibraryFile) -> LibraryDiffReport {
    let hfma_changed_4byte_offsets =
        diff_4byte_words(&raw_a.outer_header, &raw_b.outer_header, &[]);

    let mut hsma_10_hex_a = None;
    let mut hsma_10_hex_b = None;
    let mut plma_a = None;
    let mut plma_b = None;
    let mut albums_a = HashMap::new();
    let mut albums_b = HashMap::new();
    let mut artists_a = HashMap::new();
    let mut artists_b = HashMap::new();
    let mut playlists_a = HashMap::new();
    let mut playlists_b = HashMap::new();
    let mut map_a: HashMap<u64, &ItmaSection> = HashMap::new();
    let mut map_b: HashMap<u64, &ItmaSection> = HashMap::new();

    for hsma in &raw_a.sections {
        match &hsma.child {
            HsmaChild::TrackList(ltma) => {
                for t in &ltma.tracks {
                    map_a.insert(t.header.track_persistent_id, t);
                }
            }
            HsmaChild::LibraryMaster(p) => plma_a = Some(p),
            HsmaChild::AlbumList(l) => {
                for a in &l.albums {
                    albums_a.insert(a.header.album_persistent_id, a);
                }
            }
            HsmaChild::ArtistList(l) => {
                for a in &l.artists {
                    artists_a.insert(a.header.artist_persistent_id, a);
                }
            }
            HsmaChild::PlaylistList(l) => {
                for p in &l.playlists {
                    playlists_a.insert(p.header.playlist_persistent_id, p);
                }
            }
            HsmaChild::Unknown {
                subtype: 10,
                signature,
                raw_bytes,
            } => {
                let mut v = signature.to_vec();
                v.extend_from_slice(raw_bytes);
                hsma_10_hex_a = Some(hex::encode(v));
            }
            _ => {}
        }
    }
    for hsma in &raw_b.sections {
        match &hsma.child {
            HsmaChild::TrackList(ltma) => {
                for t in &ltma.tracks {
                    map_b.insert(t.header.track_persistent_id, t);
                }
            }
            HsmaChild::LibraryMaster(p) => plma_b = Some(p),
            HsmaChild::AlbumList(l) => {
                for a in &l.albums {
                    albums_b.insert(a.header.album_persistent_id, a);
                }
            }
            HsmaChild::ArtistList(l) => {
                for a in &l.artists {
                    artists_b.insert(a.header.artist_persistent_id, a);
                }
            }
            HsmaChild::PlaylistList(l) => {
                for p in &l.playlists {
                    playlists_b.insert(p.header.playlist_persistent_id, p);
                }
            }
            HsmaChild::Unknown {
                subtype: 10,
                signature,
                raw_bytes,
            } => {
                let mut v = signature.to_vec();
                v.extend_from_slice(raw_bytes);
                hsma_10_hex_b = Some(hex::encode(v));
            }
            _ => {}
        }
    }

    let mut plma_changed_4byte_offsets = Vec::new();
    let mut plma_boma_changes = Vec::new();
    if let (Some(pa), Some(pb)) = (plma_a, plma_b) {
        plma_changed_4byte_offsets = diff_4byte_words(&pa.header, &pb.header, &[]);
        for boma_b in &pb.bomas {
            let matching_a = pa.bomas.iter().find(|b| b.subtype == boma_b.subtype);
            if matching_a.map(|a| &a.payload) != Some(&boma_b.payload) {
                plma_boma_changes.push(format!(
                    "plma boma 0x{:04X} ({}) changed",
                    boma_b.subtype,
                    boma_b.subtype_name()
                ));
            }
        }
    }

    let mut album_diffs = Vec::new();
    for (&apid, &ab) in &albums_b {
        if !albums_a.contains_key(&apid) {
            let ma = crate::model::Album::from_raw(ab);
            album_diffs.push(format!(
                "+ ADDED ALBUM {:016X}: title={:?} artist={:?} album_artist={:?} is_comp={}",
                apid, ma.title, ma.artist, ma.album_artist, ma.is_compilation
            ));
        }
    }
    for (&apid, &aa) in &albums_a {
        let Some(&ab) = albums_b.get(&apid) else {
            let ma = crate::model::Album::from_raw(aa);
            album_diffs.push(format!(
                "- REMOVED ALBUM {:016X}: title={:?} artist={:?} album_artist={:?}",
                apid, ma.title, ma.artist, ma.album_artist
            ));
            continue;
        };
        let off_diffs = diff_4byte_words(&aa.header, &ab.header, &[8]);
        let ma = crate::model::Album::from_raw(aa);
        let mb = crate::model::Album::from_raw(ab);
        if !off_diffs.is_empty()
            || ma.title != mb.title
            || ma.artist != mb.artist
            || ma.album_artist != mb.album_artist
        {
            album_diffs.push(format!(
                "~ CHANGED ALBUM {:016X} ({:?}): offsets={:?} artist={:?}->{:?} album_artist={:?}->{:?} comp={}->{} rating={}->{} fav={:?}->{:?}",
                apid, mb.title, off_diffs, ma.artist, mb.artist, ma.album_artist, mb.album_artist, ma.is_compilation, mb.is_compilation, ma.star_rating, mb.star_rating, ma.suggestion_flag, mb.suggestion_flag
            ));
        }
    }

    let mut artist_diffs = Vec::new();
    for (&rpid, &ra) in &artists_a {
        if let Some(&rb) = artists_b.get(&rpid) {
            let off_diffs = diff_4byte_words(&ra.header, &rb.header, &[8]);
            let ma = crate::model::Artist::from_raw(ra);
            let mb = crate::model::Artist::from_raw(rb);
            if !off_diffs.is_empty() || ma.name != mb.name || ma.sort_name != mb.sort_name {
                artist_diffs.push(format!(
                    "~ CHANGED ARTIST {:016X} ({:?}): offsets={:?} fav={:?}->{:?}",
                    rpid, mb.name, off_diffs, ma.suggestion_flag, mb.suggestion_flag
                ));
            }
        }
    }

    let mut playlist_diffs = Vec::new();
    let mut pb_keys: Vec<u64> = playlists_b.keys().copied().collect();
    pb_keys.sort();
    for ppid in pb_keys {
        let pb = playlists_b[&ppid];
        let mb = crate::model::Playlist::from_raw(pb);
        if let Some(&pa) = playlists_a.get(&ppid) {
            let mut ignore = vec![8, 16];
            if pa.header.date_created == pb.header.date_created {
                ignore.push(24);
            }
            let off_diffs = diff_4byte_words(&pa.header, &pb.header, &ignore);
            let ma = crate::model::Playlist::from_raw(pa);
            let mut boma_diffs = Vec::new();
            if pa.bomas.len() != pb.bomas.len() {
                boma_diffs.push(format!(
                    "boma_count: {}->{}",
                    pa.bomas.len(),
                    pb.bomas.len()
                ));
            }
            for bb in &pb.bomas {
                if bb.subtype == 0x00DA || bb.subtype == 0x00CE {
                    continue;
                }
                let ma_b = pa.bomas.iter().find(|ba| ba.subtype == bb.subtype);
                if ma_b.map(|x| &x.payload) != Some(&bb.payload) {
                    boma_diffs.push(format!(
                        "boma 0x{:04X} ({}) changed",
                        bb.subtype,
                        bb.subtype_name()
                    ));
                }
            }
            if ma.track_persistent_ids != mb.track_persistent_ids {
                boma_diffs.push(format!(
                    "track_pids changed ({} -> {})",
                    ma.track_persistent_ids.len(),
                    mb.track_persistent_ids.len()
                ));
            }
            if !off_diffs.is_empty()
                || !boma_diffs.is_empty()
                || ma.name != mb.name
                || ma.description != mb.description
                || ma.declared_track_count != mb.declared_track_count
            {
                playlist_diffs.push(format!(
                    "~ CHANGED PLAYLIST {:016X} ({:?}): parent={:?} tracks={}->{} share={}->{} fav={:?}->{:?} desc={:?}->{:?} offsets={:?} boma_diffs={:?}",
                    ppid, mb.name, mb.parent_persistent_id, ma.declared_track_count, mb.declared_track_count, ma.cloud_share_mode, mb.cloud_share_mode, ma.suggestion_flag, mb.suggestion_flag, ma.description, mb.description, off_diffs, boma_diffs
                ));
            }
        } else {
            let mut cur = std::io::Cursor::new(Vec::new());
            let _ = binrw::BinWrite::write_le(&pb.header, &mut cur);
            let hdr_bytes = cur.into_inner();
            let mut extra_info = String::new();
            let mut nonzero_hdr = Vec::new();
            for (idx, &b) in hdr_bytes.iter().enumerate() {
                if b != 0 && !(0..38).contains(&idx) {
                    nonzero_hdr.push(format!("{}:0x{:02X}", idx, b));
                }
            }
            extra_info.push_str(&format!(
                "\n      nonzero_hdr(>37)=[{}]",
                nonzero_hdr.join(", ")
            ));
            for boma in &pb.bomas {
                match &boma.payload {
                    BomaPayload::SmartPlaylistRules(rules) => {
                        extra_info.push_str(&format!(
                            "\n      [SmartPlaylistRules 0x00C9]: unk_4=0x{:08X} rule_count={} conjunction={}",
                            rules.unk_4, rules.rule_count, rules.conjunction
                        ));
                        for (ri, r) in rules.rules.iter().enumerate() {
                            extra_info.push_str(&format!(
                                "\n        Rule #{:02}: field=0x{:02X} ({}: {}) cmp=0x{:08X} ({}) unk_8=0x{:08X} unk_44=0x{:X} unk_48=0x{:X} child={:?}",
                                ri + 1,
                                r.field_id,
                                r.field_id,
                                r.field_name(),
                                r.comparison_method,
                                r.comparison_description(),
                                r.unk_8,
                                r.unk_44,
                                r.unk_48,
                                r.child
                            ));
                        }
                    }
                    BomaPayload::SmartPlaylistOptions(opts) => {
                        let mut ocur = std::io::Cursor::new(Vec::new());
                        let _ = binrw::BinWrite::write_le(opts.as_ref(), &mut ocur);
                        extra_info.push_str(&format!(
                            "\n      [SmartPlaylistOptions 0x00CA]: live={} match_rules={} limit={} unit={} ({}) select_method=0x{:02X} ({}: {}) limit_count={} only_checked={} negate_select={} raw_hex={}",
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
                            hex::encode(ocur.into_inner())
                        ));
                    }
                    BomaPayload::StringSection(s) => {
                        extra_info.push_str(&format!(
                            "\n      [boma 0x{:04X} ({})]: {:?}",
                            boma.subtype,
                            boma.subtype_name(),
                            s.text
                        ));
                    }
                    BomaPayload::RawStringUtf8 { text, .. }
                    | BomaPayload::RawStringUtf16Le { text, .. } => {
                        extra_info.push_str(&format!(
                            "\n      [boma 0x{:04X} ({})]: {:?}",
                            boma.subtype,
                            boma.subtype_name(),
                            text
                        ));
                    }
                    BomaPayload::PlaylistItemCompact(ipfa) => {
                        extra_info.push_str(&format!(
                            "\n      [IPfa 0x00DA]: item_pid={:016X} track_pid={:016X}",
                            ipfa.item_persistent_id, ipfa.track_persistent_id
                        ));
                    }
                    BomaPayload::Unknown { raw_bytes, .. } => {
                        extra_info.push_str(&format!(
                            "\n      [boma 0x{:04X} ({})]: len={} hex={}",
                            boma.subtype,
                            boma.subtype_name(),
                            raw_bytes.len(),
                            hex::encode(raw_bytes)
                        ));
                    }
                    _ => {}
                }
            }
            playlist_diffs.push(format!(
                "+ ADDED PLAYLIST {:016X} ({:?}): parent={:?} is_folder={} is_smart={} tracks={} share={} fav={:?} desc={:?}{}",
                ppid, mb.name, mb.parent_persistent_id, mb.is_folder, mb.is_smart, mb.declared_track_count, mb.cloud_share_mode, mb.suggestion_flag, mb.description, extra_info
            ));
        }
    }

    let mut added_tracks = Vec::new();
    let mut removed_tracks = Vec::new();
    let mut common_tracks = 0usize;
    let mut itma_offset_change_frequencies: BTreeMap<usize, usize> = BTreeMap::new();
    let mut track_numerics_offset_change_frequencies: BTreeMap<usize, usize> = BTreeMap::new();
    let mut sample_track_diffs = Vec::new();

    for (&pid, &itma_b) in &map_b {
        if !map_a.contains_key(&pid) {
            let tb = crate::model::Track::from_raw(itma_b);
            added_tracks.push(format!(
                "{} ({:?} - {:?})",
                tb.persistent_id, tb.artist, tb.title
            ));
        }
    }

    let mut pids: Vec<u64> = map_a.keys().copied().collect();
    pids.sort();
    for pid in pids {
        let itma_a = map_a[&pid];
        let Some(&itma_b) = map_b.get(&pid) else {
            let ta = crate::model::Track::from_raw(itma_a);
            removed_tracks.push(format!(
                "{} ({:?} - {:?})",
                ta.persistent_id, ta.artist, ta.title
            ));
            continue;
        };
        common_tracks += 1;

        let ta = crate::model::Track::from_raw(itma_a);
        let tb = crate::model::Track::from_raw(itma_b);

        // Compare serialized 376-byte itma headers word-by-word
        let mut cur_a = std::io::Cursor::new(Vec::new());
        let mut cur_b = std::io::Cursor::new(Vec::new());
        let _ = binrw::BinWrite::write_le(&itma_a.header, &mut cur_a);
        let _ = binrw::BinWrite::write_le(&itma_b.header, &mut cur_b);
        let bytes_a = cur_a.into_inner();
        let bytes_b = cur_b.into_inner();

        let mut itma_changed_4byte_offsets = Vec::new();
        let mut non_sort_itma_changed = false;
        let min_len = bytes_a.len().min(bytes_b.len());
        for off in (0..min_len).step_by(4) {
            if off + 4 <= min_len {
                let va = u32::from_le_bytes(bytes_a[off..off + 4].try_into().unwrap());
                let vb = u32::from_le_bytes(bytes_b[off..off + 4].try_into().unwrap());
                if va != vb {
                    *itma_offset_change_frequencies.entry(off).or_insert(0) += 1;
                    if off != 24 {
                        itma_changed_4byte_offsets.push((off, va, vb));
                        if !(280..308).contains(&off) {
                            non_sort_itma_changed = true;
                        }
                    }
                }
            }
        }

        // Compare TrackNumerics (boma 0x0001)
        let mut numerics_changed_4byte_offsets = Vec::new();
        let num_a = itma_a.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::TrackNumerics(n) => Some(n),
            _ => None,
        });
        let num_b = itma_b.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::TrackNumerics(n) => Some(n),
            _ => None,
        });
        if let (Some(na), Some(nb)) = (num_a, num_b) {
            let mut ncur_a = std::io::Cursor::new(Vec::new());
            let mut ncur_b = std::io::Cursor::new(Vec::new());
            let _ = binrw::BinWrite::write_le(na.as_ref(), &mut ncur_a);
            let _ = binrw::BinWrite::write_le(nb.as_ref(), &mut ncur_b);
            let nbytes_a = ncur_a.into_inner();
            let nbytes_b = ncur_b.into_inner();
            let nmin = nbytes_a.len().min(nbytes_b.len());
            for off in (0..nmin).step_by(4) {
                if off + 4 <= nmin {
                    let va = u32::from_le_bytes(nbytes_a[off..off + 4].try_into().unwrap());
                    let vb = u32::from_le_bytes(nbytes_b[off..off + 4].try_into().unwrap());
                    if va != vb {
                        *track_numerics_offset_change_frequencies
                            .entry(off)
                            .or_insert(0) += 1;
                        if off != 0 {
                            numerics_changed_4byte_offsets.push((off, va, vb));
                        }
                    }
                }
            }
        }

        let mut plays_skips_changed_4byte_offsets = Vec::new();
        let ps_a = itma_a.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::TrackPlaysSkips(ps) => Some(ps),
            _ => None,
        });
        let ps_b = itma_b.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::TrackPlaysSkips(ps) => Some(ps),
            _ => None,
        });
        if let (Some(pa), Some(pb)) = (ps_a, ps_b) {
            plays_skips_changed_4byte_offsets = diff_4byte_words(pa, pb, &[]);
        }

        let mut hl_changes = Vec::new();
        macro_rules! cmp_field {
            ($name:expr, $fa:expr, $fb:expr) => {
                if $fa != $fb {
                    hl_changes.push(format!("{}: {:?} -> {:?}", $name, $fa, $fb));
                }
            };
        }
        cmp_field!("title", ta.title, tb.title);
        cmp_field!("album", ta.album, tb.album);
        cmp_field!("artist", ta.artist, tb.artist);
        cmp_field!("album_artist", ta.album_artist, tb.album_artist);
        cmp_field!("composer", ta.composer, tb.composer);
        cmp_field!("grouping", ta.grouping, tb.grouping);
        cmp_field!("comments", ta.comments, tb.comments);
        cmp_field!("work_name", ta.work_name, tb.work_name);
        cmp_field!("movement_name", ta.movement_name, tb.movement_name);
        cmp_field!("movement_number", ta.movement_number, tb.movement_number);
        cmp_field!("movement_total", ta.movement_total, tb.movement_total);
        cmp_field!("sort_title", ta.sort_title, tb.sort_title);
        cmp_field!("sort_album", ta.sort_album, tb.sort_album);
        cmp_field!("sort_artist", ta.sort_artist, tb.sort_artist);
        cmp_field!(
            "sort_album_artist",
            ta.sort_album_artist,
            tb.sort_album_artist
        );
        cmp_field!("sort_composer", ta.sort_composer, tb.sort_composer);
        cmp_field!("disc_number", ta.disc_number, tb.disc_number);
        cmp_field!("disc_total", ta.disc_total, tb.disc_total);
        cmp_field!("bpm", ta.bpm, tb.bpm);
        cmp_field!(
            "volume_adjustment",
            ta.volume_adjustment,
            tb.volume_adjustment
        );
        cmp_field!("equalizer", ta.equalizer, tb.equalizer);
        cmp_field!(
            "playback_start_ms",
            ta.playback_start_ms,
            tb.playback_start_ms
        );
        cmp_field!("playback_stop_ms", ta.playback_stop_ms, tb.playback_stop_ms);
        cmp_field!("disabled", ta.disabled, tb.disabled);
        cmp_field!(
            "skip_when_shuffling",
            ta.skip_when_shuffling,
            tb.skip_when_shuffling
        );
        cmp_field!(
            "remember_playback_position",
            ta.remember_playback_position,
            tb.remember_playback_position
        );
        cmp_field!(
            "show_composer_in_all_views",
            ta.show_composer_in_all_views,
            tb.show_composer_in_all_views
        );
        cmp_field!(
            "use_work_and_movement",
            ta.use_work_and_movement,
            tb.use_work_and_movement
        );
        cmp_field!(
            "album_is_compilation",
            ta.album_is_compilation,
            tb.album_is_compilation
        );
        cmp_field!(
            "has_custom_lyrics",
            ta.has_custom_lyrics,
            tb.has_custom_lyrics
        );
        cmp_field!(
            "has_local_custom_lyrics",
            ta.has_local_custom_lyrics,
            tb.has_local_custom_lyrics
        );
        cmp_field!(
            "has_unsynced_local_changes",
            ta.has_unsynced_local_changes,
            tb.has_unsynced_local_changes
        );
        cmp_field!("auto_sort_flags", ta.auto_sort_flags, tb.auto_sort_flags);
        cmp_field!(
            "custom_lyrics_hash",
            ta.custom_lyrics_hash,
            tb.custom_lyrics_hash
        );
        cmp_field!(
            "cloud_lyrics_token",
            ta.cloud_lyrics_token,
            tb.cloud_lyrics_token
        );
        cmp_field!("play_count", ta.play_count, tb.play_count);
        cmp_field!("true_play_count", ta.true_play_count, tb.true_play_count);
        cmp_field!("skip_count", ta.skip_count, tb.skip_count);
        cmp_field!("true_skip_count", ta.true_skip_count, tb.true_skip_count);
        cmp_field!("star_rating", ta.star_rating, tb.star_rating);
        cmp_field!("suggestion_flag", ta.suggestion_flag, tb.suggestion_flag);
        cmp_field!("date_modified", ta.date_modified, tb.date_modified);
        cmp_field!("file_url", ta.file_url, tb.file_url);
        cmp_field!("legacy_file_path", ta.legacy_file_path, tb.legacy_file_path);

        let subs_a: Vec<u32> = itma_a.bomas.iter().map(|b| b.subtype).collect();
        let subs_b: Vec<u32> = itma_b.bomas.iter().map(|b| b.subtype).collect();
        if subs_a != subs_b {
            hl_changes.push(format!("boma_subtypes: {:04X?} -> {:04X?}", subs_a, subs_b));
        }

        if (!hl_changes.is_empty()
            || non_sort_itma_changed
            || !numerics_changed_4byte_offsets.is_empty()
            || !plays_skips_changed_4byte_offsets.is_empty())
            && sample_track_diffs.len() < 100
        {
            sample_track_diffs.push(TrackDiffSample {
                persistent_id: ta.persistent_id,
                title: tb.title,
                artist: tb.artist,
                high_level_changes: hl_changes,
                itma_changed_4byte_offsets,
                numerics_changed_4byte_offsets,
                plays_skips_changed_4byte_offsets,
            });
        }
    }

    added_tracks.truncate(50);
    removed_tracks.truncate(50);

    LibraryDiffReport {
        hfma_changed_4byte_offsets,
        hsma_10_hex_a,
        hsma_10_hex_b,
        plma_changed_4byte_offsets,
        plma_boma_changes,
        tracks_in_a: map_a.len(),
        tracks_in_b: map_b.len(),
        common_tracks,
        added_tracks,
        removed_tracks,
        itma_offset_change_frequencies,
        track_numerics_offset_change_frequencies,
        sample_track_diffs,
        album_diffs,
        artist_diffs,
        playlist_diffs,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XmlComparisonReport {
    pub xml_track_count: usize,
    pub musicdb_track_count: usize,
    pub matched_by_persistent_id: usize,
    pub field_match_counts: BTreeMap<String, (usize, usize)>,
    pub unmapped_xml_keys_found_in_binary: BTreeMap<String, Vec<String>>,
}

/// Cross-references a `RawLibraryFile` against an `iTunes Music Library.xml` file.
pub fn compare_with_itunes_xml(
    raw: &RawLibraryFile,
    xml_path: impl AsRef<Path>,
) -> anyhow::Result<XmlComparisonReport> {
    let root = PlistValue::from_file(xml_path)?;
    let dict = root
        .as_dictionary()
        .ok_or_else(|| anyhow::anyhow!("Root of XML plist is not a dictionary"))?;

    let tracks_dict = dict
        .get("Tracks")
        .and_then(|v| v.as_dictionary())
        .ok_or_else(|| anyhow::anyhow!("Missing 'Tracks' dictionary in XML plist"))?;

    let mut mdb_tracks: HashMap<String, &ItmaSection> = HashMap::new();
    for hsma in &raw.sections {
        if let HsmaChild::TrackList(ltma) = &hsma.child {
            for t in &ltma.tracks {
                mdb_tracks.insert(format_persistent_id(t.header.track_persistent_id), t);
            }
        }
    }

    let mut matched_by_persistent_id = 0usize;
    let mut field_match_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut unmapped_xml_keys_found_in_binary: BTreeMap<String, Vec<String>> = BTreeMap::new();

    macro_rules! check_eq {
        ($name:expr, $xml_val:expr, $mdb_val:expr) => {{
            let entry = field_match_counts
                .entry($name.to_string())
                .or_insert((0, 0));
            entry.1 += 1;
            if $xml_val == $mdb_val {
                entry.0 += 1;
            }
        }};
    }

    for (_track_id_key, track_val) in tracks_dict {
        let Some(tdict) = track_val.as_dictionary() else {
            continue;
        };
        let Some(pid) = tdict.get("Persistent ID").and_then(|v| v.as_string()) else {
            continue;
        };
        let Some(&itma) = mdb_tracks.get(&pid.to_uppercase()) else {
            continue;
        };
        matched_by_persistent_id += 1;
        let mdb_track = crate::model::Track::from_raw(itma);

        if let Some(v) = tdict.get("Name").and_then(|v| v.as_string()) {
            check_eq!("Name (title)", Some(v), mdb_track.title.as_deref());
        }
        if let Some(v) = tdict.get("Artist").and_then(|v| v.as_string()) {
            check_eq!("Artist", Some(v), mdb_track.artist.as_deref());
        }
        if let Some(v) = tdict.get("Album").and_then(|v| v.as_string()) {
            check_eq!("Album", Some(v), mdb_track.album.as_deref());
        }
        if let Some(v) = tdict.get("Album Artist").and_then(|v| v.as_string()) {
            check_eq!("Album Artist", Some(v), mdb_track.album_artist.as_deref());
        }
        if let Some(v) = tdict.get("Composer").and_then(|v| v.as_string()) {
            check_eq!("Composer", Some(v), mdb_track.composer.as_deref());
        }
        if let Some(v) = tdict.get("Genre").and_then(|v| v.as_string()) {
            check_eq!("Genre", Some(v), mdb_track.genre.as_deref());
        }
        if let Some(v) = tdict
            .get("Total Time")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!(
                "Total Time (duration_ms)",
                Some(v as u32),
                mdb_track.duration_ms
            );
        }
        if let Some(v) = tdict.get("Size").and_then(|v| v.as_unsigned_integer()) {
            check_eq!("Size (file_size)", Some(v as u32), mdb_track.file_size);
        }
        if let Some(v) = tdict.get("Bit Rate").and_then(|v| v.as_unsigned_integer()) {
            check_eq!("Bit Rate", Some(v as u32), mdb_track.bit_rate);
        }
        if let Some(v) = tdict
            .get("Sample Rate")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!(
                "Sample Rate",
                Some(v as u32),
                mdb_track.sample_rate.map(|f| f as u32)
            );
        }
        if let Some(v) = tdict
            .get("Track Number")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Track Number", v as u16, mdb_track.track_number);
        }
        if let Some(v) = tdict
            .get("Track Count")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Track Count", v as u16, mdb_track.track_total);
        }
        if let Some(v) = tdict
            .get("Disc Number")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Disc Number", v as u16, mdb_track.disc_number);
        }
        if let Some(v) = tdict
            .get("Disc Count")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Disc Count", v as u16, mdb_track.disc_total);
        }
        if let Some(v) = tdict.get("Year").and_then(|v| v.as_unsigned_integer()) {
            check_eq!("Year", v as u32, mdb_track.year);
        }
        if let Some(v) = tdict.get("BPM").and_then(|v| v.as_unsigned_integer()) {
            check_eq!("BPM", v as u16, mdb_track.bpm);
        }
        if let Some(v) = tdict
            .get("Normalization")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Normalization", Some(v as u32), mdb_track.normalization);
        }
        if let Some(v) = tdict
            .get("Artwork Count")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Artwork Count", Some(v as u16), mdb_track.artwork_count);
        }
        if let Some(v) = tdict
            .get("File Folder Count")
            .and_then(|v| v.as_signed_integer())
        {
            check_eq!(
                "File Folder Count",
                Some(v as u16),
                mdb_track.file_folder_count
            );
        }
        if let Some(v) = tdict
            .get("Library Folder Count")
            .and_then(|v| v.as_signed_integer())
        {
            check_eq!(
                "Library Folder Count",
                Some(v as u16),
                mdb_track.library_folder_count
            );
        }
        if let Some(v) = tdict.get("File Type").and_then(|v| v.as_unsigned_integer()) {
            let num_ft = itma.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) => Some(n.file_type_fourcc),
                _ => None,
            });
            check_eq!("File Type (FourCC u32)", Some(v as u32), num_ft);
        }
        if let Some(v) = tdict.get("Play Date").and_then(|v| v.as_unsigned_integer()) {
            // Note: In iTunes XML, "Play Date" is seconds since 1904-01-01 in local time!
            let ps_date = itma.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackPlaysSkips(ps) => Some(ps.date_last_played),
                _ => None,
            });
            check_eq!("Play Date (Apple local epoch)", Some(v as u32), ps_date);
        }
        if let Some(v) = tdict
            .get("Play Count")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Play Count", Some(v as u32), mdb_track.play_count);
        }
        if let Some(v) = tdict
            .get("Skip Count")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Skip Count", Some(v as u32), mdb_track.skip_count);
        }
        if let Some(v) = tdict
            .get("Part Of Gapless Album")
            .and_then(|v| v.as_boolean())
        {
            check_eq!("Part Of Gapless Album", v, mdb_track.part_of_gapless_album);
        }
        if let Some(v) = tdict.get("Playlist Only").and_then(|v| v.as_boolean()) {
            check_eq!("Playlist Only", v, mdb_track.playlist_only);
        }
        if let Some(v) = tdict.get("Compilation").and_then(|v| v.as_boolean()) {
            check_eq!("Compilation", v, mdb_track.album_is_compilation);
        }
        if let Some(v) = tdict.get("Matched").and_then(|v| v.as_boolean()) {
            check_eq!(
                "Matched (iTunes Match)",
                Some(v),
                mdb_track.is_itunes_matched
            );
        }
        if let Some(v) = tdict.get("Apple Music").and_then(|v| v.as_boolean()) {
            check_eq!(
                "Apple Music (catalog)",
                Some(v),
                mdb_track.is_apple_music_catalog
            );
        }
        if let Some(v) = tdict.get("Work").and_then(|v| v.as_string()) {
            check_eq!("Work", Some(v), mdb_track.work_name.as_deref());
        }
        if let Some(v) = tdict.get("Movement Name").and_then(|v| v.as_string()) {
            check_eq!("Movement Name", Some(v), mdb_track.movement_name.as_deref());
        }
        if let Some(v) = tdict
            .get("Movement Number")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Movement Number", v as u16, mdb_track.movement_number);
        }
        if let Some(v) = tdict
            .get("Movement Count")
            .and_then(|v| v.as_unsigned_integer())
        {
            check_eq!("Movement Count", v as u16, mdb_track.movement_total);
        }

        // Record all XML keys present across tracks
        for (k, _v) in tdict {
            unmapped_xml_keys_found_in_binary
                .entry(k.clone())
                .or_default();
        }
    }

    Ok(XmlComparisonReport {
        xml_track_count: tracks_dict.len(),
        musicdb_track_count: mdb_tracks.len(),
        matched_by_persistent_id,
        field_match_counts,
        unmapped_xml_keys_found_in_binary,
    })
}
