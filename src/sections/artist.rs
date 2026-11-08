use binrw::binrw;
use serde::{Deserialize, Serialize};

use super::boma::BomaSection;

/// `lAma` (Artist List Master) header (typically 100 bytes).
#[binrw]
#[brw(little, magic = b"lAma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LAmaHeader {
    /// Offset 4: Header size in bytes (typically 100).
    pub header_size: u32,
    /// Offset 8: Number of `iAma` artist subsections that follow.
    pub artist_count: u32,
    /// Offset 12..100: Unknown / zero padding.
    #[br(count = header_size.saturating_sub(12))]
    pub unk_12_end: Vec<u8>,
}

/// Complete `lAma` section with all `iAma` artist items.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LAmaSection {
    pub header: LAmaHeader,
    #[br(count = header.artist_count)]
    pub artists: Vec<IAmaSection>,
}

/// `iAma` (Artist Item) header (132 bytes in modern Apple Music, 120 in Catalina).
#[binrw]
#[brw(little, magic = b"iAma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IAmaHeader {
    /// Offset 4: Header size in bytes (typically 132).
    pub header_size: u32,
    /// Offset 8: Total size of this `iAma` + its `boma` children.
    pub total_size: u32,
    /// Offset 12: Number of `boma` children.
    pub boma_count: u32,
    /// Offset 16..24: Artist Persistent ID.
    pub artist_persistent_id: u64,
    /// Offset 24..28: Unknown (typically 2).
    pub unk_24: u32,
    /// Offset 28..32: Unknown (typically 1).
    pub unk_28: u32,
    /// Offset 32..52: Unknown 20 bytes (all zeros).
    pub unk_32_52: [u8; 20],
    /// Offset 52..60: 64-bit Apple Music Store Artist ID (`atID` atom, 0 if custom local artist).
    pub apple_music_store_artist_id: u64,
    /// Offset 60..64: Suggestion flag modified date (Apple timestamp).
    pub date_suggestion_flag_modified: u32,
    /// Offset 64..80: Artwork UUID in `artwork.sqlite` (16 bytes).
    pub artwork_uuid: [u8; 16],
    /// Offset 80..88: Secondary Artist Persistent ID (often matches `artist_persistent_id` if from iTunes, or 0).
    pub artist_persistent_id_2: u64,
    /// Offset 88..96: Unknown 8 bytes (all zeros).
    pub unk_88_96: [u8; 8],
    /// Offset 96..100: Artwork source kind in `artwork.sqlite` (`item_to_artwork.source_kind`; 45 = Artist Artwork when `artwork_uuid` is non-zero, 0 otherwise).
    pub artwork_source_kind: u32,
    /// Offset 100: Unknown byte (typically 5 when `artwork_uuid` is non-zero, 0 otherwise).
    pub unk_100: u8,
    /// Offset 101..103: Suggestion/like flag (0 = none, 1 = removed, 2 = favorite, 3 = suggest less).
    pub suggestion_flag: u16,
    /// Offset 103: Unknown byte (typically 1).
    pub unk_103: u8,
    /// Offset 104..108: Artwork cache size kind in `artwork.sqlite` (`cache_items.size_kind`; 1 = `_sk1` when `artwork_uuid` is non-zero, 0 otherwise).
    pub artwork_size_kind: u32,
    /// Offset 108..112: Artwork cached image format in `artwork.sqlite` (`cache_items.extension`; 1 = `jpeg`, 2 = `png`, 0 = not cached / pending).
    pub artwork_image_format: u32,
    /// Offset 112..120: Repeated 64-bit Apple Music Store Artist ID (`atID` atom).
    pub apple_music_store_artist_id_2: u64,
    /// Offset 120..124: Cloud sync timestamp (UTC Apple timestamp, or 2; present when `header_size >= 124`).
    #[br(if(header_size >= 124))]
    pub date_cloud_synced: Option<u32>,
    /// Remaining bytes up to `header_size` (8 bytes `124..132` when `header_size == 132`, all zeros).
    #[br(count = header_size.saturating_sub(if header_size >= 124 { 124 } else { 120 }))]
    pub unk_tail: Vec<u8>,
}

/// Complete `iAma` section (header + `boma` children).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IAmaSection {
    pub header: IAmaHeader,
    #[br(count = header.boma_count)]
    pub bomas: Vec<BomaSection>,
}
