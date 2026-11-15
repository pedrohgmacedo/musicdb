use binrw::binrw;

use super::boma::BomaSection;
use super::{read_bounded_vec, read_header_tail};

/// Social profiles list master header (`Lsma`, `hsma` subtype 15, typically 92 bytes).
#[binrw]
#[brw(little, magic = b"Lsma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocialProfileListHeader {
    /// Offset 4: Header size in bytes (typically 92, minimum 12).
    pub header_size: u32,
    /// Offset 8: Number of `isma` social profile items that follow.
    pub profile_count: u32,
    /// Offset 12..92: Unknown / zero padding.
    #[br(parse_with = read_header_tail, args(header_size, 12, "Lsma"))]
    pub unk_12_end: Vec<u8>,
}

/// Complete social profile list section (`Lsma`) with all social profile items (`isma`).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocialProfileListSection {
    /// The `Lsma` list header.
    pub header: SocialProfileListHeader,
    /// All `isma` social profile records in the library.
    #[br(parse_with = read_bounded_vec, args(header.profile_count, 64, "Lsma.profiles"))]
    pub profiles: Vec<SocialProfileItem>,
}

/// Social profile item header (`isma`, typically 136 bytes).
#[binrw]
#[brw(little, magic = b"isma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocialProfileItemHeader {
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
    /// Offset 64..`header_size`: Remaining bytes (typically 72 bytes of zeros).
    #[br(parse_with = read_header_tail, args(header_size, 64, "isma"))]
    pub unk_64_end: Vec<u8>,
}

/// Complete social profile item section (`isma` header + `boma` children).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocialProfileItem {
    /// The `isma` social profile header.
    pub header: SocialProfileItemHeader,
    /// Metadata `boma` children attached to this social profile.
    #[br(parse_with = read_bounded_vec, args(header.boma_count, 16, "isma.bomas"))]
    pub bomas: Vec<BomaSection>,
}
