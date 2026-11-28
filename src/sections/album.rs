use binrw::binrw;

use super::boma::BomaSection;
use super::timestamp::AppleTimestamp;
use super::unknown::{Unknown, UnknownVisitor, VisitUnknowns, read_unknown_tail, visit_unknowns};
use super::{read_bounded_vec, read_item_bomas};

/// Album list master header (`lama`, typically 48 bytes).
#[binrw]
#[brw(little, magic = b"lama")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlbumListHeader {
    /// Offset 4: Header size in bytes (typically 48, minimum 12).
    pub header_size: u32,
    /// Offset 8: Number of `iama` album subsections that follow.
    pub album_count: u32,
    /// Offset 12..48: Unknown / zero padding.
    #[br(parse_with = read_unknown_tail, args(header_size, 12, "lama"))]
    pub unk_12_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for AlbumListHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "lama", [(tail: self.unk_12_end, 12)]);
    }
}

/// Complete album list section (`lama`) with all album items (`iama`).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlbumListSection {
    /// The `lama` list header.
    pub header: AlbumListHeader,
    /// All `iama` album records in the library.
    #[br(parse_with = read_bounded_vec, args(header.album_count, 104, "lama.albums"))]
    pub albums: Vec<AlbumItem>,
}

impl VisitUnknowns for AlbumListSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("lama", visitor);
        for album in &self.albums {
            album.visit_unknowns("iama", visitor);
        }
    }
}

/// Album item header (`iama`, 140 bytes in modern Apple Music, 104 in Catalina).
#[binrw]
#[brw(little, magic = b"iama")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlbumItemHeader {
    /// Offset 4: Header size in bytes (typically 140).
    pub header_size: u32,
    /// Offset 8: Total size of this `iama` + its `boma` children.
    pub total_size: u32,
    /// Offset 12: Number of `boma` children.
    pub boma_count: u32,
    /// Offset 16..24: Album Persistent ID.
    pub album_persistent_id: u64,
    /// Offset 24: Unknown byte (typically 2).
    pub unk_24: Unknown<u8>,
    /// Offset 25: Compilation flag (1 = album is a compilation of songs by various artists, 0 = single artist).
    pub is_compilation: u8,
    /// Offset 26: Album playlist-only flag (`0` = album is in main library with `first_track_persistent_id != 0`, `1` = album is referenced only by playlist-only tracks with `first_track_persistent_id == 0`).
    pub playlist_only: u8,
    /// Offset 27: Unknown byte (typically `1`).
    pub unk_27: Unknown<u8>,
    /// Offset 28..32: Unknown (typically 1).
    pub unk_28: Unknown<u32>,
    /// Offset 32..40: Persistent ID of the first track in the album (or 0).
    pub first_track_persistent_id: u64,
    /// Offset 40: Star rating (0..=100 in steps of 20).
    pub star_rating: u8,
    /// Offset 41: Star rating inheritance (0x01 = direct user rating, 0x20 = computed from tracks).
    pub star_rating_inheritance: u8,
    /// Offset 42..44: Suggestion/like flag (0 = none, 1 = removed, 2 = favorite, 3 = suggest less).
    pub suggestion_flag: u16,
    /// Offset 44..64: Unknown 20 bytes (all zeros).
    pub unk_44_64: Unknown<[u8; 20]>,
    /// Offset 64..72: Secondary Album Persistent ID (often matches `album_persistent_id` if imported from iTunes, or 0).
    pub album_persistent_id_2: u64,
    /// Offset 72..96: Unknown 24 bytes (byte `85` / `unk_72_96[13]` is a verified 100% bijection across `2024`–`2026` libraries: `2` iff `boma 0x0136` `catalog_album_genre` is present and `0` otherwise; remaining 23 bytes are always zero).
    pub unk_72_96: Unknown<[u8; 24]>,
    /// Offset 96..100: Suggestion flag modified date (Apple timestamp).
    pub date_suggestion_flag_modified: AppleTimestamp,
    /// Offset 100..104: Most recent last-played date of any track in the album (Apple timestamp).
    pub date_last_played: AppleTimestamp,
    /// Offset 104..112: 64-bit Apple Music Store Album ID (`plID` atom, present when `header_size >= 112`).
    #[br(if(header_size >= 112))]
    pub apple_music_store_album_id: Option<u64>,
    /// Offset 112..120: Repeated 64-bit Apple Music Store Album ID (`plID` atom, present when `header_size >= 120`).
    #[br(if(header_size >= 120))]
    pub apple_music_store_album_id_2: Option<u64>,
    /// Offset 120..124: Cloud sync timestamp (UTC Apple timestamp, or 2 on local-only albums; present when `header_size >= 124`).
    #[br(if(header_size >= 124))]
    pub date_cloud_synced: Option<AppleTimestamp>,
    /// Offset 124..128: Album release date (UTC Apple timestamp; present when `header_size >= 128`).
    #[br(if(header_size >= 128))]
    pub date_released: Option<AppleTimestamp>,
    /// Offset 128..136: Unknown 8 bytes (present when `header_size >= 136`; bytes `128..132` hold a future `AppleTimestamp` — observed `2027-02-01T02:00:00Z`, matching the `02:00Z` local-DST-midnight convention of `TrackNumerics::unk_116_cloud_timestamp` — on select catalog albums in format `23.1`, with `132..136` zero).
    #[br(if(header_size >= 136))]
    pub unk_128_136: Option<Unknown<[u8; 8]>>,
    /// Offset 136..140: Apple Music catalog album release/edition type (`0` = none/local, `1` = Standard Album, `3` = Bonus/Deluxe/Expanded Edition, `4` = EP, `5` = Single; present when `header_size >= 140`).
    #[br(if(header_size >= 140))]
    pub catalog_album_type: Option<u32>,
    /// Remaining bytes up to `header_size` beyond 140 (e.g. `140..156`).
    #[br(parse_with = read_unknown_tail, args(header_size, AlbumItemHeader::tail_start_offset(header_size), "iama"))]
    pub unk_tail: Unknown<Vec<u8>>,
}

impl AlbumItemHeader {
    /// Returns the minimum byte offset at which `unk_tail` begins for a given `header_size`.
    #[must_use]
    pub(crate) const fn tail_start_offset(header_size: u32) -> u32 {
        if header_size >= 140 {
            140
        } else if header_size >= 136 {
            136
        } else if header_size >= 128 {
            128
        } else if header_size >= 124 {
            124
        } else if header_size >= 120 {
            120
        } else if header_size >= 112 {
            112
        } else {
            104
        }
    }
}

impl VisitUnknowns for AlbumItemHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        let tail_start = usize::try_from(Self::tail_start_offset(self.header_size)).unwrap_or(104);
        visit_unknowns!(
            visitor,
            "iama",
            [
                (self.unk_24, 24),
                (self.unk_27, 27),
                (self.unk_28, 28),
                (self.unk_44_64, 44),
                (self.unk_72_96, 72),
                (?self.unk_128_136, 128),
                (tail: self.unk_tail, tail_start),
            ]
        );
    }
}

/// Complete album item section (`iama` header + `boma` children).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlbumItem {
    /// The `iama` album header.
    pub header: AlbumItemHeader,
    /// Metadata `boma` children attached to this album.
    #[br(parse_with = read_item_bomas, args(header.header_size, header.total_size, header.boma_count, "iama", "iama.bomas"))]
    pub bomas: Vec<BomaSection>,
}

impl VisitUnknowns for AlbumItem {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("iama", visitor);
        for boma in &self.bomas {
            boma.visit_unknowns("iama", visitor);
        }
    }
}
