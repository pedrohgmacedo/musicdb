use binrw::binrw;

use super::read_bounded_vec;
use super::unknown::{Unknown, UnknownVisitor, VisitUnknowns, read_unknown_tail, visit_unknowns};

/// Pending offline cloud deletion list master header (`DCma`, `hsma` subtype 8, typically 92 bytes).
#[binrw]
#[brw(little, magic = b"DCma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CloudDeletionListHeader {
    /// Offset 4: Header size in bytes (typically 92, minimum 12).
    pub header_size: u32,
    /// Offset 8: Number of `dCma` pending deletion items that follow.
    pub item_count: u32,
    /// Offset 12..`header_size`: Unknown header bytes (offset 12..16 is typically `1`, followed by zeros).
    #[br(parse_with = read_unknown_tail, args(header_size, 12, "DCma"))]
    pub unk_12_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for CloudDeletionListHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "DCma", [(tail: self.unk_12_end, 12)]);
    }
}

/// Complete pending offline cloud deletion list section (`DCma`) with all deletion items (`dCma`).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CloudDeletionListSection {
    /// The `DCma` list header.
    pub header: CloudDeletionListHeader,
    /// All pending `dCma` cloud deletion records waiting to sync with iCloud Music Library.
    #[br(parse_with = read_bounded_vec, args(header.item_count, 24, "DCma.items"))]
    pub items: Vec<CloudDeletionItem>,
}

impl VisitUnknowns for CloudDeletionListSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("DCma", visitor);
        for item in &self.items {
            item.visit_unknowns("dCma", visitor);
        }
    }
}

/// Single pending offline cloud deletion entry (`dCma`, typically 40 bytes).
#[binrw]
#[brw(little, magic = b"dCma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CloudDeletionItem {
    /// Offset 4: Section size in bytes (typically 40, minimum 24).
    pub section_size: u32,
    /// Offset 8..16: iCloud Music Library `sagaId` of the deleted track.
    pub cloud_saga_id: u64,
    /// Offset 16: Deletion mode (`0` = full library deletion, `2` = removed from library while kept as `playlist_only`).
    pub deletion_kind: u8,
    /// Offset 17: Unknown byte (observed as `2`).
    pub unk_17: Unknown<u8>,
    /// Offset 18..20: Unknown 2 bytes (typically `[0, 0]`).
    pub unk_18_20: Unknown<[u8; 2]>,
    /// Offset 20..24: Unknown 4 bytes (observed as `1`).
    pub unk_20: Unknown<u32>,
    /// Offset 24..`section_size`: Remaining bytes (typically 16 bytes of zeros when `section_size == 40`).
    #[br(parse_with = read_unknown_tail, args(section_size, 24, "dCma"))]
    pub unk_24_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for CloudDeletionItem {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "dCma",
            [
                (self.unk_17, 17),
                (self.unk_18_20, 18),
                (self.unk_20, 20),
                (tail: self.unk_24_end, 24),
            ]
        );
    }
}
