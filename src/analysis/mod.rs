//! Analysis modules for `.musicdb` libraries.
//!
//! General-purpose diagnostics and comparisons live directly under `analysis`, while
//! experimental or one-off reverse-engineering probes live under [`adhoc`] behind the
//! optional `adhoc` Cargo feature (`--features adhoc`).

pub(crate) mod diff;
pub(crate) mod unknowns;
pub(crate) mod xml_check;

#[cfg(feature = "adhoc")]
pub(crate) mod adhoc;

pub(crate) use diff::{LibraryDiffReport, diff_libraries};
pub(crate) use unknowns::{UnknownsReport, analyze_unknowns};
pub(crate) use xml_check::{XmlComparisonReport, compare_with_itunes_xml};

/// Serializes any little-endian `binrw` structure to bytes for diffing or inspection.
pub(crate) fn serialize_le<T: binrw::BinWrite>(item: &T) -> Vec<u8>
where
    for<'a> T::Args<'a>: Default,
{
    let mut cursor = std::io::Cursor::new(Vec::new());
    let _ = binrw::BinWrite::write_le(item, &mut cursor);
    cursor.into_inner()
}

/// Compares two serialized `binrw` values in 4-byte little-endian words, skipping `ignore_offsets`.
pub(crate) fn diff_4byte_words<T: binrw::BinWrite>(
    a: &T,
    b: &T,
    ignore_offsets: &[usize],
) -> Vec<(usize, u32, u32)>
where
    for<'a> T::Args<'a>: Default,
{
    let bytes_a = serialize_le(a);
    let bytes_b = serialize_le(b);
    let mut out = Vec::new();
    for (idx, (ba, bb)) in bytes_a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(bytes_b.as_chunks::<4>().0.iter())
        .enumerate()
    {
        let off = idx * 4;
        if ignore_offsets.contains(&off) {
            continue;
        }
        let va = u32::from_le_bytes(*ba);
        let vb = u32::from_le_bytes(*bb);
        if va != vb {
            out.push((off, va, vb));
        }
    }
    out
}
