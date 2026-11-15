//! Type-safe wrappers and visitor traits for undecoded `.musicdb` binary fields.

use std::fmt;
use std::io::{Read, Seek, Write};
use std::ops::{Deref, DerefMut};

use binrw::{BinRead, BinResult, BinWrite, Endian};

use super::read_header_tail;

/// Wrapper around an undecoded binary field whose bytes are preserved losslessly.
///
/// Wrapping unknown fields in [`Unknown<T>`] distinguishes undecoded regions from
/// settled format fields at the type level and enables automatic traversal via
/// [`VisitUnknowns`].
#[repr(transparent)]
#[non_exhaustive]
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

/// Byte offset range of an unknown field within its parent section.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Computes a fixed [`UnknownSpan`] from a start offset and an [`UnknownValue`] type's wire length.
pub(crate) fn fixed_span<T: UnknownValue>(start: usize, _val: &Unknown<T>) -> UnknownSpan {
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
        let mut cursor = Cursor::new(vec![0u8; 16]);
        let err = read_unknown_tail(&mut cursor, Endian::Little, (12, 20, "test"))
            .expect_err("header_size < fixed_size must fail");
        assert!(err.to_string().contains("test"));

        let mut short_cursor = Cursor::new(vec![0u8; 2]);
        let err = read_unknown_tail(&mut short_cursor, Endian::Little, (28, 20, "test"))
            .expect_err("truncated tail must fail");
        assert_ne!(err.to_string(), "");
    }
}
