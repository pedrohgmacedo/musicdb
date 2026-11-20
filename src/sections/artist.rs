use binrw::binrw;

use super::boma::BomaSection;
use super::read_bounded_vec;
use super::timestamp::AppleTimestamp;
use super::unknown::{
    TailBytes, Unknown, UnknownVisitor, VisitUnknowns, read_unknown_tail, read_unknown_tail_bytes,
    visit_unknowns,
};

/// Artist list master header (`lAma`, typically 100 bytes).
#[binrw]
#[brw(little, magic = b"lAma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArtistListHeader {
    /// Offset 4: Header size in bytes (typically 100, minimum 12).
    pub header_size: u32,
    /// Offset 8: Number of `iAma` artist subsections that follow.
    pub artist_count: u32,
    /// Offset 12..100: Unknown / zero padding.
    #[br(parse_with = read_unknown_tail, args(header_size, 12, "lAma"))]
    pub unk_12_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for ArtistListHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "lAma", [(tail: self.unk_12_end, 12)]);
    }
}

/// Complete artist list section (`lAma`) with all artist items (`iAma`).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArtistListSection {
    /// The `lAma` list header.
    pub header: ArtistListHeader,
    /// All `iAma` artist records in the library.
    #[br(parse_with = read_bounded_vec, args(header.artist_count, 120, "lAma.artists"))]
    pub artists: Vec<ArtistItem>,
}

impl VisitUnknowns for ArtistListSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("lAma", visitor);
        for artist in &self.artists {
            artist.visit_unknowns("iAma", visitor);
        }
    }
}

/// Artist item header (`iAma`, 132 bytes in modern Apple Music, 120 in Catalina).
#[binrw]
#[brw(little, magic = b"iAma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArtistItemHeader {
    /// Offset 4: Header size in bytes (typically 132).
    pub header_size: u32,
    /// Offset 8: Total size of this `iAma` + its `boma` children.
    pub total_size: u32,
    /// Offset 12: Number of `boma` children.
    pub boma_count: u32,
    /// Offset 16..24: Artist Persistent ID.
    pub artist_persistent_id: u64,
    /// Offset 24: Unknown byte (typically 2, matches `iama` offset 24).
    pub unk_24: Unknown<u8>,
    /// Offset 25: Artist playlist-only flag (`0` = artist in main library, `1` = referenced only by playlist-only tracks).
    pub playlist_only: u8,
    /// Offset 26..28: Unknown 2 bytes (typically `[0, 0]`).
    pub unk_26_28: Unknown<[u8; 2]>,
    /// Offset 28..32: Unknown (typically 1).
    pub unk_28: Unknown<u32>,
    /// Offset 32..52: Unknown 20 bytes (all zeros).
    pub unk_32_52: Unknown<[u8; 20]>,
    /// Offset 52..60: 64-bit Apple Music Store Artist ID (`atID` atom, 0 if custom local artist).
    pub apple_music_store_artist_id: u64,
    /// Offset 60..64: Suggestion flag modified date (Apple timestamp).
    pub date_suggestion_flag_modified: AppleTimestamp,
    /// Offset 64..80: Artwork UUID in `artwork.sqlite` (16 bytes).
    pub artwork_uuid: [u8; 16],
    /// Offset 80..88: Secondary Artist Persistent ID (often matches `artist_persistent_id` if from iTunes, or 0).
    pub artist_persistent_id_2: u64,
    /// Offset 88..96: Unknown 8 bytes (all zeros).
    pub unk_88_96: Unknown<[u8; 8]>,
    /// Offset 96..100: Artwork source kind in `artwork.sqlite` (`item_to_artwork.source_kind`; 45 = Artist Artwork when `artwork_uuid` is non-zero, 0 otherwise).
    pub artwork_source_kind: u32,
    /// Offset 100: Unknown byte (typically 5 when `artwork_uuid` is non-zero, 0 otherwise).
    pub unk_100: Unknown<u8>,
    /// Offset 101..103: Suggestion/like flag (0 = none, 1 = removed, 2 = favorite, 3 = suggest less).
    pub suggestion_flag: u16,
    /// Offset 103: Unknown byte (typically 1).
    pub unk_103: Unknown<u8>,
    /// Offset 104..108: Artwork cache size kind in `artwork.sqlite` (`cache_items.size_kind`; 1 = `_sk1` when `artwork_uuid` is non-zero, 0 otherwise).
    pub artwork_size_kind: u32,
    /// Offset 108..112: Artwork cached image format in `artwork.sqlite` (`cache_items.extension`; 1 = `jpeg`, 2 = `png`, 0 = not cached / pending).
    pub artwork_image_format: u32,
    /// Offset 112..120: Repeated 64-bit Apple Music Store Artist ID (`atID` atom).
    pub apple_music_store_artist_id_2: u64,
    /// Offset 120..124: Cloud sync timestamp (UTC Apple timestamp, or 2; present when `header_size >= 124`).
    #[br(if(header_size >= 124))]
    pub date_cloud_synced: Option<AppleTimestamp>,
    /// Remaining bytes up to `header_size` (8 bytes `124..132` when `header_size == 132`, all zeros),
    /// stored inline without heap allocation up to 16 bytes.
    #[br(parse_with = read_unknown_tail_bytes, args(header_size, ArtistItemHeader::tail_start_offset(header_size), "iAma"))]
    pub unk_tail: Unknown<TailBytes>,
}

impl ArtistItemHeader {
    /// Returns the minimum byte offset at which `unk_tail` begins for a given `header_size`.
    #[must_use]
    pub(crate) const fn tail_start_offset(header_size: u32) -> u32 {
        if header_size >= 124 { 124 } else { 120 }
    }
}

impl VisitUnknowns for ArtistItemHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        let tail_start = usize::try_from(Self::tail_start_offset(self.header_size)).unwrap_or(120);
        visit_unknowns!(
            visitor,
            "iAma",
            [
                (self.unk_24, 24),
                (self.unk_26_28, 26),
                (self.unk_28, 28),
                (self.unk_32_52, 32),
                (self.unk_88_96, 88),
                (self.unk_100, 100),
                (self.unk_103, 103),
                (tail: self.unk_tail, tail_start),
            ]
        );
    }
}

/// Complete artist item section (`iAma` header + `boma` children).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArtistItem {
    /// The `iAma` artist header.
    pub header: ArtistItemHeader,
    /// Metadata `boma` children attached to this artist.
    #[br(parse_with = read_bounded_vec, args(header.boma_count, 16, "iAma.bomas"))]
    pub bomas: Vec<BomaSection>,
}

impl VisitUnknowns for ArtistItem {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("iAma", visitor);
        for boma in &self.bomas {
            boma.visit_unknowns("iAma", visitor);
        }
    }
}
