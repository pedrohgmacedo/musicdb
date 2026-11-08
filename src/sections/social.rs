use binrw::binrw;
use serde::{Deserialize, Serialize};

use super::boma::BomaSection;

/// `Lsma` (Social Profiles List Master, `hsma` subtype 15) header (typically 92 bytes).
#[binrw]
#[brw(little, magic = b"Lsma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LsmaHeader {
    /// Offset 4: Header size in bytes (typically 92).
    pub header_size: u32,
    /// Offset 8: Number of `isma` social profile items that follow.
    pub profile_count: u32,
    /// Offset 12..92: Unknown / zero padding.
    #[br(count = header_size.saturating_sub(12))]
    pub unk_12_end: Vec<u8>,
}

/// Complete `Lsma` section with all `isma` social profile items.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LsmaSection {
    pub header: LsmaHeader,
    #[br(count = header.profile_count)]
    pub profiles: Vec<IsmaSection>,
}

/// `isma` (Social Profile Item) header (typically 136 bytes).
#[binrw]
#[brw(little, magic = b"isma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsmaHeader {
    /// Offset 4: Header size in bytes (typically 136).
    pub header_size: u32,
    /// Offset 8: Total size of this `isma` + its `boma` children.
    pub total_size: u32,
    /// Offset 12: Number of `boma` children (`0x0320..=0x0324`).
    pub boma_count: u32,
    /// Offset 16..24: Social Profile Persistent ID (referenced by `lpma` playlist header offset 340..348).
    pub profile_persistent_id: u64,
    /// Offset 24..32: Unknown 8 bytes (typically 0).
    pub unk_24_32: u64,
    /// Offset 32..48: Avatar Artwork UUID (16 bytes).
    pub artwork_uuid: [u8; 16],
    /// Offset 48..52: Artwork source kind in `artwork.sqlite` (`item_to_artwork.source_kind`; 0x46 = 70 = Social Profile Avatar).
    pub artwork_source_kind: u32,
    /// Offset 52..56: Unknown flags (e.g. 0x00000005 or 0x00000105).
    pub unk_52: u32,
    /// Offset 56..60: Artwork cache size kind in `artwork.sqlite` (`cache_items.size_kind`; 1 = `_sk1`).
    pub artwork_size_kind: u32,
    /// Offset 60..64: Artwork cached image format in `artwork.sqlite` (`cache_items.extension`; 1 = `jpeg`, 2 = `png`, 0 = not cached).
    pub artwork_image_format: u32,
    /// Offset 64..header_size: Remaining bytes (typically 72 bytes of zeros).
    #[br(count = header_size.saturating_sub(64))]
    pub unk_64_end: Vec<u8>,
}

/// Complete `isma` section (header + `boma` children).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IsmaSection {
    pub header: IsmaHeader,
    #[br(count = header.boma_count)]
    pub bomas: Vec<BomaSection>,
}
