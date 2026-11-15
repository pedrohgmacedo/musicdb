//! 32-bit Apple/Mac HFS+ epoch timestamp newtype (`seconds since 1904-01-01`).

use std::fmt;

use binrw::binrw;
use jiff::Timestamp;

/// Seconds between 1904-01-01 00:00:00 UTC (Apple/Mac HFS+ epoch) and 1970-01-01 00:00:00 UTC (Unix epoch).
pub(crate) const APPLE_TO_UNIX_EPOCH_OFFSET: i64 = 2_082_844_800;

/// 32-bit unsigned Apple/Mac HFS+ timestamp (seconds since 1904-01-01 UTC/local).
///
/// Values below `86_400` are treated as non-timestamp sentinel values (such as `0` for unset
/// or `2` on local `TrackNumerics[0].date_cloud_synced`) by [`AppleTimestamp::to_utc`].
///
/// # Examples
/// ```
/// use musicdb::AppleTimestamp;
///
/// let unset = AppleTimestamp(0);
/// assert_eq!(unset.to_utc(), None);
/// assert_eq!(unset.to_string(), "0");
///
/// let sentinel = AppleTimestamp(2);
/// assert_eq!(sentinel.to_utc(), None);
///
/// let valid = AppleTimestamp(3_800_000_000);
/// assert!(valid.to_utc().is_some());
/// assert_eq!(valid.raw(), 3_800_000_000);
///
/// let AppleTimestamp(raw) = valid;
/// assert_eq!(raw, 3_800_000_000);
/// ```
#[binrw]
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct AppleTimestamp(pub u32);

impl AppleTimestamp {
    /// Wraps a raw 32-bit Apple epoch timestamp.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the underlying raw 32-bit timestamp value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns `true` if the raw timestamp is `0`.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Converts this timestamp to a UTC [`Timestamp`], returning `None` for `0` or sentinel values `< 86_400`.
    #[must_use]
    pub fn to_utc(self) -> Option<Timestamp> {
        if self.0 < 86_400 {
            return None;
        }
        let unix_secs = i64::from(self.0) - APPLE_TO_UNIX_EPOCH_OFFSET;
        Timestamp::from_second(unix_secs).ok()
    }
}

impl From<u32> for AppleTimestamp {
    fn from(raw: u32) -> Self {
        Self(raw)
    }
}

impl From<AppleTimestamp> for u32 {
    fn from(ts: AppleTimestamp) -> Self {
        ts.0
    }
}

impl fmt::Display for AppleTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dt) = self.to_utc() {
            write!(f, "{dt}")
        } else if self.0 == 0 {
            f.write_str("0")
        } else {
            write!(f, "sentinel({})", self.0)
        }
    }
}

impl fmt::Debug for AppleTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dt) = self.to_utc() {
            write!(f, "AppleTimestamp({} /* {dt} */)", self.0)
        } else {
            write!(f, "AppleTimestamp({})", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::{BinRead, BinWrite};

    use super::*;

    #[test]
    fn apple_timestamp_conversions_and_formatting() {
        let zero = AppleTimestamp::default();
        assert!(zero.is_zero());
        assert_eq!(zero.raw(), 0);
        assert_eq!(zero.to_utc(), None);
        assert_eq!(zero.to_string(), "0");
        assert_eq!(format!("{zero:?}"), "AppleTimestamp(0)");

        let sentinel = AppleTimestamp::new(2);
        assert!(!sentinel.is_zero());
        assert_eq!(sentinel.raw(), 2);
        assert_eq!(sentinel.to_utc(), None);
        assert_eq!(sentinel.to_string(), "sentinel(2)");
        assert_eq!(format!("{sentinel:?}"), "AppleTimestamp(2)");

        let below_cutoff = AppleTimestamp::new(86_399);
        assert_eq!(below_cutoff.raw(), 86_399);
        assert_eq!(below_cutoff.to_utc(), None);
        assert_eq!(below_cutoff.to_string(), "sentinel(86399)");

        let at_cutoff = AppleTimestamp::new(86_400);
        let cutoff_dt = at_cutoff.to_utc().expect("86_400 must convert");
        assert_eq!(cutoff_dt.to_string(), "1904-01-02T00:00:00Z");
        assert_eq!(at_cutoff.to_string(), "1904-01-02T00:00:00Z");

        let max_ts = AppleTimestamp::new(u32::MAX);
        assert_eq!(max_ts.raw(), u32::MAX);
        assert!(max_ts.to_utc().is_some());

        let unix_epoch = AppleTimestamp::from(2_082_844_800u32);
        let dt = unix_epoch.to_utc().expect("Unix epoch must convert");
        assert_eq!(dt.as_second(), 0);
        assert_eq!(unix_epoch.raw(), 2_082_844_800);
        assert_eq!(u32::from(unix_epoch), 2_082_844_800);
        assert_eq!(unix_epoch.to_string(), "1970-01-01T00:00:00Z");
        assert_eq!(
            format!("{unix_epoch:?}"),
            "AppleTimestamp(2082844800 /* 1970-01-01T00:00:00Z */)"
        );
    }

    #[test]
    fn apple_timestamp_binrw_roundtrip() {
        let ts = AppleTimestamp::new(3_800_000_000);
        let mut le_buf = Vec::new();
        ts.write_le(&mut Cursor::new(&mut le_buf))
            .expect("write_le");
        assert_eq!(le_buf, 3_800_000_000u32.to_le_bytes());

        let parsed_le = AppleTimestamp::read_le(&mut Cursor::new(&le_buf)).expect("read_le");
        assert_eq!(parsed_le, ts);

        let mut be_buf = Vec::new();
        ts.write_be(&mut Cursor::new(&mut be_buf))
            .expect("write_be");
        assert_eq!(be_buf, 3_800_000_000u32.to_be_bytes());

        let parsed_be = AppleTimestamp::read_be(&mut Cursor::new(&be_buf)).expect("read_be");
        assert_eq!(parsed_be, ts);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn apple_timestamp_serde_roundtrip() {
        let ts = AppleTimestamp::new(3_800_000_000);
        let json = serde_json::to_string(&ts).expect("serialize");
        assert_eq!(json, "3800000000");
        let parsed: AppleTimestamp = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, ts);
    }
}
