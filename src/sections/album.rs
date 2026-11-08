use binrw::binrw;
use serde::{Deserialize, Serialize};

use super::boma::BomaSection;

/// `lama` (Album List Master) header (typically 48 bytes).
#[binrw]
#[brw(little, magic = b"lama")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LamaHeader {
    /// Offset 4: Header size in bytes (typically 48).
    pub header_size: u32,
    /// Offset 8: Number of `iama` album subsections that follow.
    pub album_count: u32,
    /// Offset 12..48: Unknown / zero padding.
    #[br(count = header_size.saturating_sub(12))]
    pub unk_12_end: Vec<u8>,
}

/// Complete `lama` section with all `iama` album items.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LamaSection {
    pub header: LamaHeader,
    #[br(count = header.album_count)]
    pub albums: Vec<IamaSection>,
}

/// `iama` (Album Item) header (140 bytes in modern Apple Music, 104 in Catalina).
#[binrw]
#[brw(little, magic = b"iama")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamaHeader {
    /// Offset 4: Header size in bytes (typically 140).
    pub header_size: u32,
    /// Offset 8: Total size of this `iama` + its `boma` children.
    pub total_size: u32,
    /// Offset 12: Number of `boma` children.
    pub boma_count: u32,
    /// Offset 16..24: Album Persistent ID.
    pub album_persistent_id: u64,
    /// Offset 24: Unknown byte (typically 2).
    pub unk_24: u8,
    /// Offset 25: Compilation flag (1 = album is a compilation of songs by various artists, 0 = single artist).
    pub is_compilation: u8,
    /// Offset 26..28: Unknown 2 bytes (typically `[0, 1]` or `[1, 1]`).
    pub unk_26_28: [u8; 2],
    /// Offset 28..32: Unknown (typically 1).
    pub unk_28: u32,
    /// Offset 32..40: Persistent ID of the first track in the album (or 0).
    pub first_track_persistent_id: u64,
    /// Offset 40: Star rating (0..=100 in steps of 20).
    pub star_rating: u8,
    /// Offset 41: Star rating inheritance (0x01 = direct user rating, 0x20 = computed from tracks).
    pub star_rating_inheritance: u8,
    /// Offset 42..44: Suggestion/like flag (0 = none, 1 = removed, 2 = favorite, 3 = suggest less).
    pub suggestion_flag: u16,
    /// Offset 44..64: Unknown 20 bytes (all zeros).
    pub unk_44_64: [u8; 20],
    /// Offset 64..72: Secondary Album Persistent ID (often matches `album_persistent_id` if imported from iTunes, or 0).
    pub album_persistent_id_2: u64,
    /// Offset 72..96: Unknown 24 bytes (all zeros).
    pub unk_72_96: [u8; 24],
    /// Offset 96..100: Suggestion flag modified date (Apple timestamp).
    pub date_suggestion_flag_modified: u32,
    /// Offset 100..104: Most recent last-played date of any track in the album (Apple timestamp).
    pub date_last_played: u32,
    /// Offset 104..112: 64-bit Apple Music Store Album ID (`plID` atom, present when `header_size >= 112`).
    #[br(if(header_size >= 112))]
    pub apple_music_store_album_id: Option<u64>,
    /// Offset 112..120: Repeated 64-bit Apple Music Store Album ID (`plID` atom, present when `header_size >= 120`).
    #[br(if(header_size >= 120))]
    pub apple_music_store_album_id_2: Option<u64>,
    /// Offset 120..124: Cloud sync timestamp (UTC Apple timestamp, or 2 on local-only albums; present when `header_size >= 124`).
    #[br(if(header_size >= 124))]
    pub date_cloud_synced: Option<u32>,
    /// Offset 124..128: Album release date (UTC Apple timestamp; present when `header_size >= 128`).
    #[br(if(header_size >= 128))]
    pub date_released: Option<u32>,
    /// Remaining bytes up to `header_size` (`128..140` or `128..156`, all zeros).
    #[br(count = header_size.saturating_sub(if header_size >= 128 { 128 } else if header_size >= 124 { 124 } else if header_size >= 120 { 120 } else if header_size >= 112 { 112 } else { 104 }))]
    pub unk_tail: Vec<u8>,
}

/// Complete `iama` section (header + `boma` children).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IamaSection {
    pub header: IamaHeader,
    #[br(count = header.boma_count)]
    pub bomas: Vec<BomaSection>,
}
