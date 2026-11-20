//! Type-safe wrappers and visitor traits for undecoded `.musicdb` binary fields.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, Write};
use std::ops::{Deref, DerefMut};

use binrw::{BinRead, BinResult, BinWrite, Endian};

use super::{read_header_tail, read_header_tail_bytes};

/// Wrapper around an undecoded binary field whose bytes are preserved losslessly.
///
/// Wrapping unknown fields in [`Unknown<T>`] distinguishes undecoded regions from
/// settled format fields at the type level and enables automatic traversal via
/// [`VisitUnknowns`].
///
/// # Examples
/// ```
/// use musicdb::sections::Unknown;
///
/// let val = Unknown(0u32);
/// assert!(val.is_zero());
/// assert_eq!(val.format_unknown(), "0 (0x00000000)");
///
/// let constructed = Unknown::new(0x1234u32);
/// let Unknown(raw) = constructed;
/// assert_eq!(raw, 0x1234);
/// assert_eq!(constructed.into_inner(), 0x1234);
/// ```
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unknown<T>(pub T);

impl<T> Unknown<T> {
    /// Wraps a raw value in [`Unknown`].
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Unwraps and returns the inner value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }

    /// Returns a shared reference to the inner value.
    #[must_use]
    pub const fn as_inner(&self) -> &T {
        &self.0
    }

    /// Returns a mutable reference to the inner value.
    #[must_use]
    pub const fn as_inner_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: UnknownValue> Unknown<T> {
    /// Returns `true` if all bytes/bits of this unknown value are zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Formats this unknown value for human-readable analysis reports.
    #[must_use]
    pub fn format_unknown(&self) -> String {
        self.0.format_unknown()
    }
}

impl<T> From<T> for Unknown<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for Unknown<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Unknown<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> AsRef<T> for Unknown<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T> AsMut<T> for Unknown<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: fmt::Display> fmt::Display for Unknown<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: fmt::LowerHex> fmt::LowerHex for Unknown<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: fmt::UpperHex> fmt::UpperHex for Unknown<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: UnknownValue> Default for Unknown<T> {
    fn default() -> Self {
        Self(T::default_value())
    }
}

impl<T: BinRead> BinRead for Unknown<T> {
    type Args<'a> = T::Args<'a>;

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<Self> {
        T::read_options(reader, endian, args).map(Self)
    }
}

impl<T: BinWrite> BinWrite for Unknown<T> {
    type Args<'a> = T::Args<'a>;

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<()> {
        self.0.write_options(writer, endian, args)
    }
}

#[cfg(feature = "serde")]
impl<T: UnknownValue> serde::Serialize for Unknown<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize_unknown(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: UnknownValue> serde::Deserialize<'de> for Unknown<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize_unknown(deserializer).map(Self)
    }
}

/// Capability trait for primitive and byte-buffer types wrapped by [`Unknown`].
pub trait UnknownValue: Sized + Clone {
    /// Fixed byte length in the binary format, or `None` for variable-length trailing buffers.
    const WIRE_LEN: Option<usize>;

    /// Returns the zero/empty default value (supporting `[u8; N]` for any `N`).
    fn default_value() -> Self;

    /// Returns `true` if the value consists solely of zero bytes.
    fn is_zero(&self) -> bool;

    /// Formats a non-zero value consistently for unknown-byte reports.
    fn format_unknown(&self) -> String;

    /// Serializes the unknown value when the `serde` feature is enabled.
    ///
    /// # Errors
    /// Returns a serializer error if serialization fails.
    #[cfg(feature = "serde")]
    fn serialize_unknown<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;

    /// Deserializes the unknown value when the `serde` feature is enabled.
    ///
    /// # Errors
    /// Returns a deserializer error if the input cannot be decoded into `Self`.
    #[cfg(feature = "serde")]
    fn deserialize_unknown<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error>;
}

macro_rules! impl_unknown_int {
    ($ty:ty, $fmt:expr) => {
        impl UnknownValue for $ty {
            const WIRE_LEN: Option<usize> = Some(std::mem::size_of::<$ty>());

            fn default_value() -> Self {
                0
            }

            fn is_zero(&self) -> bool {
                *self == 0
            }

            fn format_unknown(&self) -> String {
                $fmt(*self)
            }

            #[cfg(feature = "serde")]
            fn serialize_unknown<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                serde::Serialize::serialize(self, serializer)
            }

            #[cfg(feature = "serde")]
            fn deserialize_unknown<'de, D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                serde::Deserialize::deserialize(deserializer)
            }
        }
    };
}

impl_unknown_int!(u8, |v: u8| format!("{v} (0x{v:02X})"));
impl_unknown_int!(u16, |v: u16| format!("{v} (0x{v:04X})"));
impl_unknown_int!(u32, |v: u32| format!("{v} (0x{v:08X})"));
impl_unknown_int!(u64, |v: u64| format!("0x{v:016X}"));
impl_unknown_int!(i32, |v: i32| format!("{v} (0x{v:08X})"));
impl_unknown_int!(i64, |v: i64| format!("{v} (0x{v:016X})"));

impl<const N: usize> UnknownValue for [u8; N] {
    const WIRE_LEN: Option<usize> = Some(N);

    fn default_value() -> Self {
        [0u8; N]
    }

    fn is_zero(&self) -> bool {
        self.iter().all(|&b| b == 0)
    }

    fn format_unknown(&self) -> String {
        hex::encode(self)
    }

    #[cfg(feature = "serde")]
    fn serialize_unknown<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self))
    }

    #[cfg(feature = "serde")]
    fn deserialize_unknown<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        let expected_hex_len = N.saturating_mul(2);
        if s.len() != expected_hex_len {
            return Err(D::Error::custom(format!(
                "expected {expected_hex_len} hex chars ({N} bytes), got {}",
                s.len()
            )));
        }
        let vec = hex::decode(&s).map_err(D::Error::custom)?;
        vec.try_into()
            .map_err(|v: Vec<u8>| D::Error::custom(format!("expected {N} bytes, got {}", v.len())))
    }
}

impl UnknownValue for Vec<u8> {
    const WIRE_LEN: Option<usize> = None;

    fn default_value() -> Self {
        Self::new()
    }

    fn is_zero(&self) -> bool {
        self.iter().all(|&b| b == 0)
    }

    fn format_unknown(&self) -> String {
        hex::encode(self)
    }

    #[cfg(feature = "serde")]
    fn serialize_unknown<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self))
    }

    #[cfg(feature = "serde")]
    fn deserialize_unknown<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        hex::decode(&s).map_err(D::Error::custom)
    }
}

#[derive(Clone)]
enum TailBytesRepr {
    Inline {
        len: u8,
        buf: [u8; TailBytes::INLINE_CAP],
    },
    Heap(Box<[u8]>),
}

/// Inline-optimized byte buffer for variable-length trailing binary header regions.
///
/// Stores up to [`TailBytes::INLINE_CAP`] (16) bytes inline without heap allocation and spills
/// transparently to a heap `Box<[u8]>` for longer tails, preserving 100% lossless round-tripping
/// at the same 24-byte stack footprint as `Vec<u8>`.
#[non_exhaustive]
#[derive(Clone)]
pub struct TailBytes(TailBytesRepr);

impl TailBytes {
    /// Maximum number of bytes stored inline without heap allocation.
    pub const INLINE_CAP: usize = 16;

    /// Creates an empty [`TailBytes`] without allocating.
    #[must_use]
    pub const fn new() -> Self {
        Self(TailBytesRepr::Inline {
            len: 0,
            buf: [0u8; Self::INLINE_CAP],
        })
    }

    /// Creates a [`TailBytes`] from a byte slice, storing inline when `bytes.len() <= 16`.
    #[must_use]
    pub fn from_slice(bytes: &[u8]) -> Self {
        if let Ok(len_u8) = u8::try_from(bytes.len())
            && bytes.len() <= Self::INLINE_CAP
        {
            let mut buf = [0u8; Self::INLINE_CAP];
            if let Some(dst) = buf.get_mut(..bytes.len()) {
                dst.copy_from_slice(bytes);
            }
            Self(TailBytesRepr::Inline { len: len_u8, buf })
        } else {
            Self(TailBytesRepr::Heap(bytes.into()))
        }
    }

    /// Creates a [`TailBytes`] from a `Vec<u8>`, storing inline when `bytes.len() <= 16`.
    #[must_use]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.len() <= Self::INLINE_CAP {
            Self::from_slice(&bytes)
        } else {
            Self(TailBytesRepr::Heap(bytes.into_boxed_slice()))
        }
    }

    /// Reads exactly `len` bytes from `reader`, storing inline when `len <= 16`.
    pub(crate) fn read_exact_len<R: Read>(reader: &mut R, len: usize) -> std::io::Result<Self> {
        if let Ok(len_u8) = u8::try_from(len)
            && len <= Self::INLINE_CAP
        {
            let mut buf = [0u8; Self::INLINE_CAP];
            if let Some(dst) = buf.get_mut(..len) {
                reader.read_exact(dst)?;
            }
            Ok(Self(TailBytesRepr::Inline { len: len_u8, buf }))
        } else {
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf)?;
            Ok(Self(TailBytesRepr::Heap(buf.into_boxed_slice())))
        }
    }

    /// Returns the stored bytes as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        match &self.0 {
            TailBytesRepr::Inline { len, buf } => {
                let n = usize::from(*len);
                buf.get(..n).unwrap_or(buf)
            }
            TailBytesRepr::Heap(b) => b,
        }
    }

    /// Returns the byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.0 {
            TailBytesRepr::Inline { len, .. } => usize::from(*len),
            TailBytesRepr::Heap(b) => b.len(),
        }
    }

    /// Returns `true` if the buffer contains zero bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copies the contents into an owned `Vec<u8>`.
    #[must_use]
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }
}

impl Default for TailBytes {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for TailBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl AsRef<[u8]> for TailBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl fmt::Debug for TailBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl PartialEq for TailBytes {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for TailBytes {}

impl PartialEq<[u8]> for TailBytes {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

impl PartialEq<&[u8]> for TailBytes {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_slice() == *other
    }
}

impl PartialEq<Vec<u8>> for TailBytes {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialOrd for TailBytes {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TailBytes {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl Hash for TailBytes {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl From<Vec<u8>> for TailBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_vec(bytes)
    }
}

impl From<&[u8]> for TailBytes {
    fn from(bytes: &[u8]) -> Self {
        Self::from_slice(bytes)
    }
}

impl<const N: usize> From<[u8; N]> for TailBytes {
    fn from(bytes: [u8; N]) -> Self {
        Self::from_slice(&bytes)
    }
}

impl BinWrite for TailBytes {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<()> {
        writer.write_all(self.as_slice())?;
        Ok(())
    }
}

impl UnknownValue for TailBytes {
    const WIRE_LEN: Option<usize> = None;

    fn default_value() -> Self {
        Self::new()
    }

    fn is_zero(&self) -> bool {
        self.as_slice().iter().all(|&b| b == 0)
    }

    fn format_unknown(&self) -> String {
        hex::encode(self.as_slice())
    }

    #[cfg(feature = "serde")]
    fn serialize_unknown<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.as_slice()))
    }

    #[cfg(feature = "serde")]
    fn deserialize_unknown<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        hex::decode(&s)
            .map(Self::from_vec)
            .map_err(D::Error::custom)
    }
}

/// Byte offset range of an unknown field within its parent section.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnknownSpan {
    /// Fixed-width field occupying `[start, end)`.
    Fixed {
        /// Inclusive start byte offset within the parent section.
        start: usize,
        /// Exclusive end byte offset within the parent section.
        end: usize,
    },
    /// Variable-length trailing slice starting at `start`.
    Tail {
        /// Inclusive start byte offset within the parent section.
        start: usize,
        /// Runtime byte length of the trailing slice.
        len: usize,
    },
}

impl UnknownSpan {
    /// Returns the inclusive start byte offset.
    #[must_use]
    pub const fn start(&self) -> usize {
        match *self {
            Self::Fixed { start, .. } | Self::Tail { start, .. } => start,
        }
    }

    /// Returns the exclusive end byte offset if it fits in `usize`.
    #[must_use]
    pub const fn end(&self) -> Option<usize> {
        match *self {
            Self::Fixed { end, .. } => Some(end),
            Self::Tail { start, len } => start.checked_add(len),
        }
    }
}

impl fmt::Display for UnknownSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed { start, end } => write!(f, "{start}..{end}"),
            Self::Tail { start, .. } => write!(f, "{start}..end"),
        }
    }
}

/// Visitor notified for each [`Unknown`] field in a section tree.
pub trait UnknownVisitor {
    /// Records one unknown field occurrence.
    fn visit_unknown<T: UnknownValue>(
        &mut self,
        section: &str,
        field_name: &str,
        span: UnknownSpan,
        value: &Unknown<T>,
    );
}

/// Trait implemented by raw section structs that expose their [`Unknown`] fields.
pub trait VisitUnknowns {
    /// Visits all unknown fields in `self`, using `section` as context label where applicable.
    fn visit_unknowns<V: UnknownVisitor>(&self, section: &str, visitor: &mut V);
}

/// Helper `binrw` parser that delegates to [`read_header_tail`] and wraps the result in [`Unknown`].
pub(crate) fn read_unknown_tail<R: Read + Seek>(
    reader: &mut R,
    endian: Endian,
    args: (u32, u32, &'static str),
) -> BinResult<Unknown<Vec<u8>>> {
    read_header_tail(reader, endian, args).map(Unknown)
}

/// Helper `binrw` parser that delegates to [`read_header_tail_bytes`] and wraps the inline-optimized
/// result in [`Unknown<TailBytes>`].
pub(crate) fn read_unknown_tail_bytes<R: Read + Seek>(
    reader: &mut R,
    endian: Endian,
    args: (u32, u32, &'static str),
) -> BinResult<Unknown<TailBytes>> {
    read_header_tail_bytes(reader, endian, args).map(Unknown)
}

/// Marker trait for fixed-width [`UnknownValue`] types (excludes `Vec<u8>`).
pub(crate) trait FixedUnknown: UnknownValue {}

impl FixedUnknown for u8 {}
impl FixedUnknown for u16 {}
impl FixedUnknown for u32 {}
impl FixedUnknown for u64 {}
impl FixedUnknown for i32 {}
impl FixedUnknown for i64 {}
impl<const N: usize> FixedUnknown for [u8; N] {}

/// Computes a fixed [`UnknownSpan`] from a start offset and a fixed-width [`UnknownValue`] type's wire length.
pub(crate) fn fixed_span<T: FixedUnknown>(start: usize, _val: &Unknown<T>) -> UnknownSpan {
    let len = T::WIRE_LEN.unwrap_or(0);
    UnknownSpan::Fixed {
        start,
        end: start.saturating_add(len),
    }
}

/// Records all [`Unknown`] fields of a section struct concisely next to its definition.
macro_rules! visit_unknowns {
    ($visitor:expr, $section:expr, [ $($item:tt),* $(,)? ]) => {
        $(
            visit_unknowns!(@one $visitor, $section, $item);
        )*
    };
    (@one $visitor:expr, $section:expr, (tail: $self:ident . $field:ident, $start:expr)) => {
        $visitor.visit_unknown(
            $section,
            stringify!($field),
            $crate::sections::unknown::UnknownSpan::Tail {
                start: $start,
                len: $self.$field.len(),
            },
            &$self.$field,
        );
    };
    (@one $visitor:expr, $section:expr, (? $self:ident . $field:ident, $start:expr)) => {
        if let Some(val) = &$self.$field {
            $visitor.visit_unknown(
                $section,
                stringify!($field),
                $crate::sections::unknown::fixed_span($start, val),
                val,
            );
        }
    };
    (@one $visitor:expr, $section:expr, ($self:ident . $field:ident, $start:expr)) => {
        $visitor.visit_unknown(
            $section,
            stringify!($field),
            $crate::sections::unknown::fixed_span($start, &$self.$field),
            &$self.$field,
        );
    };
}

pub(crate) use visit_unknowns;

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::Endian;

    use super::*;

    #[test]
    fn unknown_large_array_and_formatting() {
        let mut bytes = [0u8; 40];
        bytes[0] = 0xAB;
        bytes[39] = 0xCD;
        let unk = Unknown::new(bytes);

        assert!(!unk.is_zero());
        assert_eq!(unk[0], 0xAB);
        assert_eq!(unk[39], 0xCD);
        assert!(unk.format_unknown().starts_with("ab00"));
        assert!(unk.format_unknown().ends_with("00cd"));
        assert_eq!(Unknown::new([0u8; 40]), Unknown::default());
        assert!(Unknown::new([0u8; 40]).is_zero());

        let int_unk = Unknown::new(0x0001_0203u32);
        assert_eq!(format!("{int_unk:08X}"), "00010203");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn unknown_serde_roundtrip() {
        let large = Unknown::new([0x12u8; 40]);
        let json = serde_json::to_string(&large).expect("serialize [u8; 40]");
        assert_eq!(json, format!("\"{}\"", "12".repeat(40)));
        let decoded: Unknown<[u8; 40]> = serde_json::from_str(&json).expect("deserialize [u8; 40]");
        assert_eq!(decoded, large);

        let arr95 = Unknown::new([0x34u8; 95]);
        let json95 = serde_json::to_string(&arr95).expect("serialize [u8; 95]");
        let decoded95: Unknown<[u8; 95]> =
            serde_json::from_str(&json95).expect("deserialize [u8; 95]");
        assert_eq!(decoded95, arr95);

        let arr120 = Unknown::new([0x56u8; 120]);
        let json120 = serde_json::to_string(&arr120).expect("serialize [u8; 120]");
        let decoded120: Unknown<[u8; 120]> =
            serde_json::from_str(&json120).expect("deserialize [u8; 120]");
        assert_eq!(decoded120, arr120);

        let bad_short: Result<Unknown<[u8; 40]>, _> = serde_json::from_str("\"1234\"");
        assert!(bad_short.is_err(), "short hex string must be rejected");
        let bad_long: Result<Unknown<[u8; 40]>, _> =
            serde_json::from_str(&format!("\"{}\"", "ab".repeat(41)));
        assert!(bad_long.is_err(), "oversized hex string must be rejected");

        let tail = Unknown::new(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let tail_json = serde_json::to_string(&tail).expect("serialize Vec<u8>");
        assert_eq!(tail_json, "\"deadbeef\"");
        let tail_decoded: Unknown<Vec<u8>> =
            serde_json::from_str(&tail_json).expect("deserialize Vec<u8>");
        assert_eq!(tail_decoded, tail);

        let num = Unknown::new(42u32);
        let num_json = serde_json::to_string(&num).expect("serialize u32");
        assert_eq!(num_json, "42");
        let num_decoded: Unknown<u32> = serde_json::from_str(&num_json).expect("deserialize u32");
        assert_eq!(num_decoded, num);
    }

    #[test]
    fn read_unknown_tail_rejects_underflow_and_truncation() {
        use crate::error::MusicDbError;

        let mut cursor = Cursor::new(vec![0u8; 16]);
        let err = read_unknown_tail(&mut cursor, Endian::Little, (12, 20, "test"))
            .expect_err("header_size < fixed_size must fail");
        match err {
            binrw::Error::Custom { err, .. } => {
                let inner = err
                    .downcast_ref::<MusicDbError>()
                    .expect("expected MusicDbError");
                assert!(matches!(
                    inner,
                    MusicDbError::InvalidHeaderSize {
                        section: "test",
                        header_size: 12,
                        min_size: 20,
                    }
                ));
            }
            other => panic!("unexpected binrw error: {other:?}"),
        }

        let mut short_cursor = Cursor::new(vec![0u8; 2]);
        let err = read_unknown_tail(&mut short_cursor, Endian::Little, (28, 20, "test"))
            .expect_err("truncated tail must fail");
        match err {
            binrw::Error::Custom { err, .. } => {
                let inner = err
                    .downcast_ref::<MusicDbError>()
                    .expect("expected MusicDbError");
                assert!(matches!(
                    inner,
                    MusicDbError::TruncatedSection {
                        needed: 8,
                        available: 2,
                        ..
                    }
                ));
            }
            other => panic!("unexpected binrw error: {other:?}"),
        }
    }

    #[test]
    fn tail_bytes_inline_heap_and_size_invariant() {
        assert!(
            std::mem::size_of::<TailBytes>() <= std::mem::size_of::<Vec<u8>>(),
            "TailBytes should not exceed Vec<u8> stack size"
        );
        for len in [0usize, 1, 4, 8, 16, 17, 32] {
            let raw: Vec<u8> = (0..len)
                .map(|i| u8::try_from(i % 251).expect("byte"))
                .collect();
            let from_slice = TailBytes::from_slice(&raw);
            let from_vec = TailBytes::from_vec(raw.clone());
            let mut cur = Cursor::new(raw.as_slice());
            let from_reader = TailBytes::read_exact_len(&mut cur, len).expect("read_exact_len");
            assert_eq!(from_slice, from_vec);
            assert_eq!(from_slice, from_reader);
            assert_eq!(from_slice.as_slice(), raw.as_slice());
            assert_eq!(from_slice.len(), len);
            assert_eq!(from_slice.is_empty(), len == 0);

            let unk = Unknown::new(from_slice);
            assert_eq!(unk.is_zero(), raw.iter().all(|&b| b == 0));
            assert_eq!(unk.format_unknown(), hex::encode(&raw));

            #[cfg(feature = "serde")]
            {
                let json = serde_json::to_string(&unk).expect("serialize Unknown<TailBytes>");
                let back: Unknown<TailBytes> =
                    serde_json::from_str(&json).expect("deserialize Unknown<TailBytes>");
                assert_eq!(back, unk);
            }
        }
    }
}
