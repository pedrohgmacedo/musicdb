use std::io::{Cursor, Read, Seek, Write};

use binrw::{BinRead, BinResult, BinWrite, Endian, binrw};

use super::boma::BomaSection;
use super::timestamp::AppleTimestamp;
use super::unknown::{Unknown, UnknownVisitor, VisitUnknowns, read_unknown_tail, visit_unknowns};
use super::{read_bounded_vec, remaining_stream_bytes};
use crate::error::MusicDbError;
use crate::model::SmartOperatorFlags;

/// Playlist list master header (`lPma`, typically 92 bytes).
#[binrw]
#[brw(little, magic = b"lPma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaylistListHeader {
    /// Offset 4: Header size in bytes (typically 92, minimum 12).
    pub header_size: u32,
    /// Offset 8: Number of `lpma` playlist subsections that follow.
    pub playlist_count: u32,
    /// Offset 12..92: Unknown / zero padding.
    #[br(parse_with = read_unknown_tail, args(header_size, 12, "lPma"))]
    pub unk_12_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for PlaylistListHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "lPma", [(tail: self.unk_12_end, 12)]);
    }
}

/// Complete playlist list section (`lPma`) with all playlist items (`lpma`).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaylistListSection {
    /// The `lPma` list header.
    pub header: PlaylistListHeader,
    /// All `lpma` playlist records in the library.
    #[br(parse_with = read_bounded_vec, args(header.playlist_count, 324, "lPma.playlists"))]
    pub playlists: Vec<PlaylistItem>,
}

impl VisitUnknowns for PlaylistListSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("lPma", visitor);
        for playlist in &self.playlists {
            playlist.visit_unknowns("lpma", visitor);
        }
    }
}

/// Playlist item header (`lpma`, typically 368 bytes in modern Apple Music, 324 in Catalina).
#[binrw]
#[brw(little, magic = b"lpma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaylistItemHeader {
    /// Offset 4: Header size in bytes (typically 368).
    pub header_size: u32,
    /// Offset 8: Total size of this `lpma` + its `boma` children.
    pub total_size: u32,
    /// Offset 12: Number of `boma` subsections.
    pub boma_count: u32,
    /// Offset 16: Number of tracks (`ipfa` items) in this playlist.
    pub track_count: u32,
    /// Offset 20..22: Unknown 2 bytes.
    pub unk_20_22: Unknown<[u8; 2]>,
    /// Offset 22..26: Playlist creation date (Apple timestamp).
    pub date_created: AppleTimestamp,
    /// Offset 26..30: Global counter.
    pub global_counter: u32,
    /// Offset 30..38: Playlist Persistent ID.
    pub playlist_persistent_id: u64,
    /// Offset 38..44: Unknown 6 bytes (offset 38 is typically 1).
    pub unk_38_44: Unknown<[u8; 6]>,
    /// Offset 44: Flag indicating the special master playlist (`####!####`, 1 = master, 0 = other).
    pub is_master_playlist: u8,
    /// Offset 45: Unknown byte.
    pub unk_45: Unknown<u8>,
    /// Offset 46..48: Smart playlist / folder flag 1 (`0x0100` on some smart playlists/folders).
    pub unk_46_smart_flag: Unknown<u16>,
    /// Offset 48: Unknown byte.
    pub unk_48: Unknown<u8>,
    /// Offset 49: Playlist folder flag (`1` = folder, `0` = playlist).
    pub is_folder: u8,
    /// Offset 50..58: Parent Playlist Folder Persistent ID (0 if root).
    pub parent_persistent_id: u64,
    /// Offset 58..70: Unknown 12 bytes.
    pub unk_58_70: Unknown<[u8; 12]>,
    /// Offset 70..78: iCloud Music Library Playlist `sagaId`.
    pub cloud_playlist_saga_id_1: u64,
    /// Offset 78..82: Special / Distinguished Kind bytes (e.g. offset 79: 4=Music, 19=Purchases, 26=Genius, 47=Music Videos, 64=TV & Movies, 65=Downloaded).
    pub distinguished_kind_bytes: [u8; 4],
    /// Offset 82..138: Unknown 56 bytes.
    pub unk_82_138: Unknown<[u8; 56]>,
    /// Offset 138..142: Playlist modification date (Apple timestamp).
    pub date_modified: AppleTimestamp,
    /// Offset 142..174: Unknown 32 bytes.
    pub unk_142_174: Unknown<[u8; 32]>,
    /// Offset 174..178: Playlist play count.
    pub play_count: u32,
    /// Offset 178..182: Playlist true play count.
    pub true_play_count: u32,
    /// Offset 182..186: Playlist last played date (Apple timestamp).
    pub date_last_played: AppleTimestamp,
    /// Offset 186..190: Unknown flags (typically `00 00 00 01` or `01 00 00 01`).
    pub unk_186_flags: Unknown<[u8; 4]>,
    /// Offset 190..192: Ownership / origin flags (`[1, 1]` = user-created playlist/folder, `[0, 0]` = subscribed external playlist, `[0, 1]` = system smart playlist, `[1, 0]` = Favourite Songs).
    pub ownership_flags: [u8; 2],
    /// Offset 192..196: Unknown u32 (0, 6, or 46).
    pub unk_192: Unknown<u32>,
    /// Offset 196..204: iCloud Music Library Playlist `sagaId` repeated.
    pub cloud_playlist_saga_id_2: u64,
    /// Offset 204..214: Unknown 10 bytes.
    pub unk_204_214: Unknown<[u8; 10]>,
    /// Offset 214..222: iCloud Music Library Playlist `sagaId` repeated.
    pub cloud_playlist_saga_id_3: u64,
    /// Offset 222: Unknown byte.
    pub unk_222: Unknown<u8>,
    /// Offset 223..225: Suggestion/like flag (0 = none, 1 = removed, 2 = favorite, 3 = suggest less).
    pub suggestion_flag: u16,
    /// Offset 225..256: Unknown 31 bytes.
    pub unk_225_256: Unknown<[u8; 31]>,
    /// Offset 256..260: Cloud refresh interval in seconds (7200 = 2 hours on subscribed playlists, 0 on user playlists).
    pub refresh_interval_seconds: u32,
    /// Offset 260..262: Unknown 2 bytes.
    pub unk_260_262: Unknown<[u8; 2]>,
    /// Offset 262: Checkbox "Show in My Profile and in Search" (1 = checked/public, 0 = unchecked/private).
    pub checkbox_show_in_profile_and_search: u8,
    /// Offset 263..279: Artwork UUID in `artwork.sqlite` (16 bytes).
    pub artwork_uuid: [u8; 16],
    /// Offset 279: Unknown byte.
    pub unk_279: Unknown<u8>,
    /// Offset 280..288: Secondary Playlist Persistent ID.
    pub playlist_persistent_id_2: u64,
    /// Offset 288..296: Unknown 8 bytes (typically zeros).
    pub unk_288_296: Unknown<[u8; 8]>,
    /// Offset 296..300: Artwork source kind in `artwork.sqlite` (`item_to_artwork.source_kind`; 63 = Cloud Playlist Cover Recipe, 102 = Custom/Uploaded Playlist Cover Artwork).
    pub artwork_source_kind: u32,
    /// Offset 300..304: Cover artwork / template kind (5 = custom uploaded cover art, 8 = auto 2x2 grid or subscribed cover, 10 = Apple cover-artwork-recipe or single album cover).
    pub artwork_template_kind: u32,
    /// Offset 304..308: Cloud publish / artwork generation date (UTC Apple timestamp).
    pub date_cloud_published: AppleTimestamp,
    /// Offset 308..312: Cloud share / visibility mode (0 = private user playlist, 2 = subscribed catalog playlist, 4 = public shared user playlist on profile).
    pub cloud_share_mode: u32,
    /// Offset 312..316: Unknown 4 bytes (0 on user playlists, sometimes non-zero on subscribed catalog playlists).
    pub unk_312: Unknown<u32>,
    /// Offset 316..320: Artwork cache size kind in `artwork.sqlite` (`cache_items.size_kind`; 1 = `_sk1` when artwork is present, 0 otherwise).
    pub artwork_size_kind: u32,
    /// Offset 320..324: Artwork cached image format in `artwork.sqlite` (`cache_items.extension`; 1 = `jpeg`, 2 = `png`, 0 = not cached / pending status 5).
    pub artwork_image_format: u32,
    /// Offset 324..328: Cloud sync / suggestion flag modified date (present when `header_size >= 328`).
    #[br(if(header_size >= 328))]
    pub date_suggestion_flag_modified: Option<AppleTimestamp>,
    /// Offset 328..340: Unknown 12 bytes (present when `header_size >= 368`).
    #[br(if(header_size >= 368))]
    pub unk_328_340: Option<Unknown<[u8; 12]>>,
    /// Offset 340..348: Author Social Profile Persistent ID (foreign key to `isma.profile_persistent_id`, present when `header_size >= 368`).
    #[br(if(header_size >= 368))]
    pub author_social_profile_persistent_id: Option<u64>,
    /// Offset 348..356: Unknown 8 bytes (present when `header_size >= 368`).
    #[br(if(header_size >= 368))]
    pub unk_348_356: Option<Unknown<u64>>,
    /// Offset 356..360: Secondary cloud sync timestamp (UTC Apple timestamp, present when `header_size >= 368`).
    #[br(if(header_size >= 368))]
    pub date_cloud_synced_2: Option<AppleTimestamp>,
    /// Offset 360..368: Unknown 8 bytes (present when `header_size >= 368`).
    #[br(if(header_size >= 368))]
    pub unk_360_368: Option<Unknown<u64>>,
    /// Remaining bytes up to `header_size` beyond 368/328/324.
    #[br(parse_with = read_unknown_tail, args(header_size, PlaylistItemHeader::tail_start_offset(header_size), "lpma"))]
    pub unk_tail: Unknown<Vec<u8>>,
}

impl PlaylistItemHeader {
    /// Returns the minimum byte offset at which `unk_tail` begins for a given `header_size`.
    #[must_use]
    pub(crate) const fn tail_start_offset(header_size: u32) -> u32 {
        if header_size >= 368 {
            368
        } else if header_size >= 328 {
            328
        } else {
            324
        }
    }
}

impl VisitUnknowns for PlaylistItemHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        let tail_start = usize::try_from(Self::tail_start_offset(self.header_size)).unwrap_or(324);
        visit_unknowns!(
            visitor,
            "lpma",
            [
                (self.unk_20_22, 20),
                (self.unk_38_44, 38),
                (self.unk_45, 45),
                (self.unk_46_smart_flag, 46),
                (self.unk_48, 48),
                (self.unk_58_70, 58),
                (self.unk_82_138, 82),
                (self.unk_142_174, 142),
                (self.unk_186_flags, 186),
                (self.unk_192, 192),
                (self.unk_204_214, 204),
                (self.unk_222, 222),
                (self.unk_225_256, 225),
                (self.unk_260_262, 260),
                (self.unk_279, 279),
                (self.unk_288_296, 288),
                (self.unk_312, 312),
                (?self.unk_328_340, 328),
                (?self.unk_348_356, 348),
                (?self.unk_360_368, 360),
                (tail: self.unk_tail, tail_start),
            ]
        );
    }
}

/// Complete playlist item section (`lpma` header + `boma` children).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaylistItem {
    /// The `lpma` playlist header.
    pub header: PlaylistItemHeader,
    /// Metadata and track item `boma` children attached to this playlist.
    #[br(parse_with = read_bounded_vec, args(header.boma_count, 16, "lpma.bomas"))]
    pub bomas: Vec<BomaSection>,
}

impl VisitUnknowns for PlaylistItem {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("lpma", visitor);
        for boma in &self.bomas {
            boma.visit_unknowns("lpma", visitor);
        }
    }
}

/// Payload of `boma` subtype `0x00CE` (`ipfa` Playlist Track Item, typically 68 bytes).
#[binrw]
#[brw(little, magic = b"ipfa")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaylistTrackItem {
    /// Offset 4: Section size in bytes (typically 68, minimum 52).
    pub section_size: u32,
    /// Offset 8..12: Global counter.
    pub global_counter: u32,
    /// Offset 12..20: Playlist item (`ipfa`) ID.
    pub item_persistent_id: u64,
    /// Offset 20..28: Track Persistent ID (`itma` reference).
    pub track_persistent_id: u64,
    /// Offset 28..40: Unknown 12 bytes.
    pub unk_28_40: Unknown<[u8; 12]>,
    /// Offset 40..44: Sequential order / counter value.
    pub unk_40_counter: Unknown<u32>,
    /// Offset 44..52: Repeated `item_persistent_id` (often non-zero on last track in playlist).
    pub item_persistent_id_2: u64,
    /// Offset 52..`section_size`: Remaining bytes (16 bytes `52..68` when `section_size == 68`).
    #[br(parse_with = read_unknown_tail, args(section_size, 52, "ipfa"))]
    pub unk_52_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for PlaylistTrackItem {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "ipfa",
            [
                (self.unk_28_40, 28),
                (self.unk_40_counter, 40),
                (tail: self.unk_52_end, 52),
            ]
        );
    }
}

/// Payload of `boma` subtype `0x00DA` (`IPfa` Compact Playlist Track Item introduced in format 23.1, typically 32 bytes).
#[binrw]
#[brw(little, magic = b"IPfa")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompactPlaylistTrackItem {
    /// Offset 4: Section size in bytes (typically 32, minimum 24).
    pub section_size: u32,
    /// Offset 8..16: Playlist item (`IPfa`) Persistent ID.
    pub item_persistent_id: u64,
    /// Offset 16..24: Track Persistent ID (`itma` reference).
    pub track_persistent_id: u64,
    /// Offset 24..`section_size`: Remaining bytes (8 bytes `24..32` when `section_size == 32`).
    #[br(parse_with = read_unknown_tail, args(section_size, 24, "IPfa"))]
    pub unk_24_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for CompactPlaylistTrackItem {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "IPfa", [(tail: self.unk_24_end, 24)]);
    }
}

/// Payload of `boma` subtype `0x00CA` (Smart Playlist Options, 112 bytes).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartPlaylistOptions {
    /// Offset 0: Checkbox "Live updating" (0 or 1).
    pub checkbox_live_updating: u8,
    /// Offset 1: Checkbox "Match [all/any] of the following rules" (0 or 1).
    pub checkbox_enable_matching_rules: u8,
    /// Offset 2: Checkbox "Limit to ..." (0 or 1).
    pub checkbox_enable_limit: u8,
    /// Offset 3: Limit unit (1=minutes, 2=MB, 3=items, 4=hours, 5=GB).
    pub limit_unit: u8,
    /// Offset 4: Selection method for limit (2=random, 5=title, 6=album, 7=artist, 9=genre, 0x15=recently added, 0x19=often played, 0x1a=recently played, 0x1c=rating).
    pub limit_selection_method: u8,
    /// Offset 5..8: Unknown 3 bytes.
    pub unk_5_8: Unknown<[u8; 3]>,
    /// Offset 8..12: Limit count.
    pub limit_count: u32,
    /// Offset 12: Checkbox "Match only checked items" (0 or 1).
    pub checkbox_match_only_checked_items: u8,
    /// Offset 13: Negate limit selection method (0 = highest/most, 1 = lowest/least).
    pub negate_limit_selection_method: u8,
    /// Offset 14: Unknown boolean flag (1 on Genius playlist, 0 otherwise).
    pub unk_14_genius_flag: Unknown<u8>,
    /// Offset 15: Unknown byte.
    pub unk_15: Unknown<u8>,
    /// Offset 16: Unknown byte (typically 7).
    pub unk_16: Unknown<u8>,
    /// Offset 17..112: Remaining 95 bytes.
    pub unk_17_112: Unknown<[u8; 95]>,
}

impl VisitUnknowns for SmartPlaylistOptions {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "SmartPlaylistOptions",
            [
                (self.unk_5_8, 5),
                (self.unk_14_genius_flag, 14),
                (self.unk_15, 15),
                (self.unk_16, 16),
                (self.unk_17_112, 17),
            ]
        );
    }
}

/// Payload of `boma` subtype `0x00C9` (`SLst` Smart Playlist Rules List, 136-byte BIG-ENDIAN header + rules).
#[binrw]
#[brw(big, magic = b"SLst")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SlstSection {
    /// Offset 4..8: Unknown version/flags (typically `0x00010001`).
    pub unk_4: Unknown<u32>,
    /// Offset 8..12: Number of `SmartPlaylistRule` children (Big-Endian).
    pub rule_count: u32,
    /// Offset 12..16: Conjunction (`0` = Match ALL, `1` = Match ANY) (Big-Endian).
    pub conjunction: u32,
    /// Offset 16..136: 120 bytes of header padding (typically zeros).
    pub unk_16_136: Unknown<[u8; 120]>,
    /// Child rules.
    #[br(parse_with = read_bounded_vec, args(rule_count, 56, "SLst.rules"))]
    pub rules: Vec<SmartPlaylistRule>,
}

impl VisitUnknowns for SlstSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "SLst", [(self.unk_4, 4), (self.unk_16_136, 16)]);
        for rule in &self.rules {
            rule.visit_unknowns("SmartPlaylistRule", visitor);
        }
    }
}

/// A single Smart Playlist Rule inside `SLst` (56-byte BIG-ENDIAN header + variable-length child).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartPlaylistRule {
    /// Offset 0..4: Field ID (Big-Endian; 0 = nested `SLst`).
    pub field_id: u32,
    /// Offset 4..8: Comparison method bitflags (Big-Endian).
    pub comparison_method: u32,
    /// Offset 8..12: Unknown (nonzero for nested `SLst`, e.g. `0x01000000` or `0x10000000`).
    pub unk_8: Unknown<u32>,
    /// Offset 12..44: Unknown 32 bytes.
    pub unk_12_44: Unknown<[u8; 32]>,
    /// Offset 44..48: Unknown (sometimes 1).
    pub unk_44: Unknown<u32>,
    /// Offset 48..52: Unknown 4 bytes.
    pub unk_48: Unknown<u32>,
    /// Offset 52..56: Byte length of `child` starting at offset 56 (Big-Endian).
    pub child_size: u32,
    /// Parsed child of this rule.
    pub child: SmartRuleChild,
}

impl VisitUnknowns for SmartPlaylistRule {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "SmartPlaylistRule",
            [
                (self.unk_8, 8),
                (self.unk_12_44, 12),
                (self.unk_44, 44),
                (self.unk_48, 48),
            ]
        );
        match &self.child {
            SmartRuleChild::Nested(slst) => slst.visit_unknowns("SLst", visitor),
            SmartRuleChild::Arguments(args) => {
                args.visit_unknowns("SmartPlaylistRuleArguments", visitor);
            }
            SmartRuleChild::StringUtf16Be { .. } | SmartRuleChild::Raw(_) => {}
        }
    }
}

/// Parsed child payload of a [`SmartPlaylistRule`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartRuleChild {
    /// Nested `SLst` rule group.
    Nested(Box<SlstSection>),
    /// 68-byte numeric/date/boolean/enum/playlist arguments block (Big-Endian).
    Arguments(SmartPlaylistRuleArguments),
    /// UTF-16 Big-Endian string argument.
    StringUtf16Be {
        /// Decoded UTF-16BE string value.
        text: String,
        /// Exact raw UTF-16BE payload bytes.
        raw_bytes: Vec<u8>,
    },
    /// Fallback raw bytes if a rule child does not match the above.
    Raw(Vec<u8>),
}

/// 68-byte Big-Endian arguments block for a `SmartPlaylistRule`.
#[binrw]
#[brw(big)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartPlaylistRuleArguments {
    /// Offset 0..8: Primary comparison value (integer, timestamp, enum value, or playlist persistent ID).
    pub arg_0: i64,
    /// Offset 8..16: Relative time unit multiplier or secondary parameter (typically `1`).
    pub arg_1: i64,
    /// Offset 16..24: Relative time unit seconds or tertiary parameter (typically `1`, `3600`, `86400`, etc.).
    pub arg_2: i64,
    /// Offset 24..32: Upper-bound comparison value for `IN_RANGE` rules (or repetition of `arg_0`).
    pub arg_3: i64,
    /// Offset 32..40: Secondary multiplier for upper bound (typically `1`).
    pub arg_4: i64,
    /// Offset 40..48: Secondary unit for upper bound (typically `1`).
    pub arg_5: i64,
    /// Offset 48..56: Reserved / unknown 64-bit word (typically `0`).
    pub arg_6: i64,
    /// Offset 56..64: Reserved / unknown 64-bit word (typically `0`).
    pub arg_7: i64,
    /// Offset 64..68: Trailing 32-bit word (typically `0`).
    pub unk_64: Unknown<u32>,
}

impl VisitUnknowns for SmartPlaylistRuleArguments {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "SmartPlaylistRuleArguments", [(self.unk_64, 64)]);
    }
}

impl BinRead for SmartPlaylistRule {
    type Args<'a> = ();

    #[expect(
        clippy::similar_names,
        reason = "unk_44 and unk_48 are named after their byte offsets in SmartPlaylistRule"
    )]
    fn read_options<R: Read + Seek>(
        reader: &mut R,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let field_id = u32::read_be(reader)?;
        let comparison_method = u32::read_be(reader)?;
        let unk_8 = Unknown::<u32>::read_be(reader)?;
        let unk_12_44 = Unknown::<[u8; 32]>::read_be(reader)?;
        let unk_44 = Unknown::<u32>::read_be(reader)?;
        let unk_48 = Unknown::<u32>::read_be(reader)?;
        let child_size = u32::read_be(reader)?;

        let payload_offset = reader.stream_position()?;
        let remaining = remaining_stream_bytes(reader)?;
        let needed_bytes = u64::from(child_size);
        if needed_bytes > remaining {
            return Err(binrw::Error::Custom {
                pos: payload_offset,
                err: Box::new(MusicDbError::TruncatedSection {
                    section: "SmartPlaylistRule.child".to_string(),
                    offset: payload_offset,
                    needed: needed_bytes,
                    available: remaining,
                }),
            });
        }
        let payload_len = usize::try_from(child_size).map_err(|_| binrw::Error::Custom {
            pos: payload_offset,
            err: Box::new(MusicDbError::TruncatedSection {
                section: "SmartPlaylistRule.child".to_string(),
                offset: payload_offset,
                needed: needed_bytes,
                available: remaining,
            }),
        })?;
        let mut child_bytes = vec![0u8; payload_len];
        reader.read_exact(&mut child_bytes)?;

        // Determine how to parse `child_bytes`
        let child = if child_bytes.len() >= 136 && child_bytes.get(0..4) == Some(b"SLst") {
            let mut cursor = Cursor::new(&child_bytes);
            match SlstSection::read_be(&mut cursor) {
                Ok(slst) if usize::try_from(cursor.position()).ok() == Some(child_bytes.len()) => {
                    SmartRuleChild::Nested(Box::new(slst))
                }
                _ => SmartRuleChild::Raw(child_bytes),
            }
        } else if SmartOperatorFlags::from_bits_retain(comparison_method)
            .contains(SmartOperatorFlags::STRING_MODE)
            && child_bytes.len().is_multiple_of(2)
        {
            // String comparisons in SLst have `SmartOperatorFlags::STRING_MODE` (0x01000000) set
            let text: String = char::decode_utf16(
                child_bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(u16::from_be_bytes),
            )
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
            SmartRuleChild::StringUtf16Be {
                text,
                raw_bytes: child_bytes,
            }
        } else if child_bytes.len() == 68 {
            let mut cursor = Cursor::new(&child_bytes);
            let args = SmartPlaylistRuleArguments::read_be(&mut cursor)?;
            SmartRuleChild::Arguments(args)
        } else {
            SmartRuleChild::Raw(child_bytes)
        };

        Ok(Self {
            field_id,
            comparison_method,
            unk_8,
            unk_12_44,
            unk_44,
            unk_48,
            child_size,
            child,
        })
    }
}

impl BinWrite for SmartPlaylistRule {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<()> {
        self.field_id.write_be(writer)?;
        self.comparison_method.write_be(writer)?;
        self.unk_8.write_be(writer)?;
        self.unk_12_44.write_be(writer)?;
        self.unk_44.write_be(writer)?;
        self.unk_48.write_be(writer)?;
        self.child_size.write_be(writer)?;

        match &self.child {
            SmartRuleChild::Nested(slst) => slst.write_be(writer)?,
            SmartRuleChild::Arguments(args) => args.write_be(writer)?,
            SmartRuleChild::StringUtf16Be { raw_bytes, .. } | SmartRuleChild::Raw(raw_bytes) => {
                writer.write_all(raw_bytes)?;
            }
        }
        Ok(())
    }
}

impl SmartPlaylistOptions {
    /// Human-readable name of `limit_unit`.
    #[must_use]
    pub fn limit_unit_name(&self) -> &'static str {
        match self.limit_unit {
            1 => "minutes",
            2 => "MB",
            3 => "items",
            4 => "hours",
            5 => "GB",
            _ => "unknown_unit",
        }
    }

    /// Human-readable name of `limit_selection_method` (combined with `negate_limit_selection_method`).
    #[must_use]
    pub fn selection_method_name(&self) -> &'static str {
        match (
            self.limit_selection_method,
            self.negate_limit_selection_method,
        ) {
            (0x02, _) => "random",
            (0x05, _) => "title",
            (0x06, _) => "album",
            (0x07, _) => "artist",
            (0x09, _) => "genre",
            (0x15, 0) => "most recently added",
            (0x15, _) => "least recently added",
            (0x19, 0) => "most often played",
            (0x19, _) => "least often played",
            (0x1A, 0) => "most recently played",
            (0x1A, _) => "least recently played",
            (0x1C, 0) => "highest rating",
            (0x1C, _) => "lowest rating",
            _ => "unknown_selection_method",
        }
    }
}

impl SmartPlaylistRule {
    /// Human-readable name of `field_id` (all 32 Smart Playlist rule fields proven via `TEST_SMART_FIELDS_1` & `TEST_SMART_FIELDS_2`).
    #[must_use]
    pub fn field_name(&self) -> &'static str {
        match self.field_id {
            0x00 => "Nested Group (SLst)",
            0x02 => "Title",
            0x03 => "Album",
            0x04 => "Artist",
            0x05 => "Bit Rate",
            0x06 => "Sample Rate",
            0x07 => "Year",
            0x08 => "Genre",
            0x09 => "Kind",
            0x0A => "Date Modified",
            0x0B => "Track Number",
            0x0C | 0x1C => "Size",
            0x0D => "Time",
            0x0E => "Comments",
            0x10 => "Date Added",
            0x12 => "Composer",
            0x16 => "Plays",
            0x17 => "Last Played",
            0x18 => "Disc Number",
            0x19 => "Rating",
            0x1D => "Checked",
            0x1F => "Compilation",
            0x23 => "BPM",
            0x25 => "Album Artwork",
            0x27 => "Grouping",
            0x28 => "Playlist",
            0x29 => "Purchased",
            0x36 => "Description",
            0x37 => "Category",
            0x39 | 0x5A => "Album Rating",
            0x3C => "Media Kind",
            0x44 => "Skips",
            0x45 => "Last Skipped",
            0x47 => "Album Artist",
            0x4E => "Sort Title",
            0x4F => "Sort Album",
            0x50 => "Sort Artist",
            0x51 => "Sort Album Artist",
            0x52 => "Sort Composer",
            0x53 => "Sort Show",
            0x59 => "Video Rating",
            0x85 => "Location",
            0x86 => "Cloud Status",
            0x9A => "Favorite",
            0x9C => "Album Favorite",
            0x9E | 0x9F => "Work",
            0xA0 => "Movement Name",
            0xA1 => "Movement Number",
            0xA4 => "Has Video",
            _ => "Unknown",
        }
    }

    /// Human-readable description of `comparison_method`.
    #[must_use]
    pub fn comparison_description(&self) -> &'static str {
        let flags = SmartOperatorFlags::from_bits_retain(self.comparison_method);
        let is_str = flags.contains(SmartOperatorFlags::STRING_MODE);
        let neg = flags.contains(SmartOperatorFlags::NEGATE);
        let base = flags
            & !(SmartOperatorFlags::STRING_MODE
                | SmartOperatorFlags::NEGATE
                | SmartOperatorFlags::SIGN_MODIFIER);

        match (base, is_str, neg) {
            (SmartOperatorFlags::IS, true, false) => "is (string)",
            (SmartOperatorFlags::IS, true, true) => "is not (string)",
            (SmartOperatorFlags::CONTAINS, _, false) => "contains",
            (SmartOperatorFlags::CONTAINS, _, true) => "does not contain",
            (SmartOperatorFlags::STARTS_WITH, _, false) => "starts with",
            (SmartOperatorFlags::ENDS_WITH, _, false) => "ends with",
            (SmartOperatorFlags::IS | SmartOperatorFlags::BITWISE_SET, false, false) => "is",
            (SmartOperatorFlags::IS | SmartOperatorFlags::BITWISE_SET, false, true) => "is not",
            (SmartOperatorFlags::GREATER_THAN, _, false) => "is greater than",
            (SmartOperatorFlags::LESS_THAN, _, false) => "is less than",
            (SmartOperatorFlags::IN_RANGE, _, false) => "is in the range",
            (SmartOperatorFlags::IN_RANGE, _, true) => "is not in the range",
            (SmartOperatorFlags::IN_THE_LAST, _, false) => "is in the last",
            (SmartOperatorFlags::IN_THE_LAST, _, true) => "is not in the last",
            _ => "unknown_comparison",
        }
    }
}
