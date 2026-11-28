//! High-level domain model for Apple Music `.musicdb` libraries (`Library`, `Track`, `Album`, `Artist`, `Playlist`, `SocialProfile`).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use bitflags::bitflags;
use jiff::Timestamp;

use crate::error::{MusicDbError, Result};
use crate::sections::boma::boma_subtype_name;
use crate::sections::{
    AlbumItem, AlbumItemHeader, AppleTimestamp, ArtistItem, ArtistItemHeader, BomaPayload,
    BomaSection, CloudDeletionItem, HfmaHeader, HsmaChild, LibraryMasterHeader,
    LibraryMasterSection, PlaylistItem, PlaylistItemHeader, RawLibraryFile, SlstSection,
    SmartPlaylistOptions, SmartPlaylistRule, SmartRuleChild, SocialProfileItem, TrackItem,
    TrackItemHeader, TrackNumerics, TrackPlaysSkips, VideoInfo,
};

/// Seconds between 1904-01-01 00:00:00 UTC (Apple/Mac HFS+ epoch) and 1970-01-01 00:00:00 UTC (Unix epoch).
pub const APPLE_TO_UNIX_EPOCH_OFFSET: i64 = crate::sections::timestamp::APPLE_TO_UNIX_EPOCH_OFFSET;

/// Converts an Apple 32-bit timestamp (seconds since 1904-01-01 UTC/local) to a [`Timestamp`].
/// Returns `None` if `apple_ts < 86_400` (0 or small non-timestamp sentinel values such as `2` on local `TrackNumerics[0].date_cloud_synced`).
#[must_use]
pub fn apple_timestamp_to_utc(apple_ts: u32) -> Option<Timestamp> {
    AppleTimestamp::new(apple_ts).to_utc()
}

/// Formats a 64-bit Persistent ID as a 16-character uppercase hexadecimal string (matching `iTunes Music Library.xml`).
#[must_use]
pub fn format_persistent_id(id: u64) -> String {
    format!("{id:016X}")
}

/// Formats a 16-byte artwork UUID as a standard `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` uppercase string.
/// Returns `None` if all 16 bytes are zero.
#[must_use]
pub fn format_artwork_uuid(bytes: &[u8; 16]) -> Option<String> {
    if bytes.iter().all(|&b| b == 0) {
        return None;
    }
    Some(format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

/// 64-bit Apple Music Persistent ID, formatted as 16 uppercase hexadecimal digits (`{:016X}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PersistentId(pub u64);

impl PersistentId {
    /// Zero Persistent ID (`0x0000_0000_0000_0000`).
    pub const ZERO: Self = Self(0);

    /// Creates a [`PersistentId`] from its raw `u64` value.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the underlying `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns `true` if the underlying `u64` is `0`.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Wraps `id` in `Some(PersistentId(id))` if `id != 0`, or `None` if `id == 0`.
    #[must_use]
    pub const fn from_non_zero(id: u64) -> Option<Self> {
        if id == 0 { None } else { Some(Self(id)) }
    }
}

impl fmt::Display for PersistentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self.0)
    }
}

impl fmt::UpperHex for PersistentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self.0)
    }
}

impl fmt::LowerHex for PersistentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl From<u64> for PersistentId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<PersistentId> for u64 {
    fn from(value: PersistentId) -> Self {
        value.0
    }
}

impl AsRef<u64> for PersistentId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

/// Error returned when parsing a [`PersistentId`] from a hexadecimal string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePersistentIdError(pub std::num::ParseIntError);

impl fmt::Display for ParsePersistentIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid hexadecimal persistent ID: {}", self.0)
    }
}

impl std::error::Error for ParsePersistentIdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl FromStr for PersistentId {
    type Err = ParsePersistentIdError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let trimmed = s
            .trim()
            .strip_prefix("0x")
            .or_else(|| s.trim().strip_prefix("0X"))
            .unwrap_or_else(|| s.trim());
        u64::from_str_radix(trimmed, 16)
            .map(Self)
            .map_err(ParsePersistentIdError)
    }
}

impl PartialEq<str> for PersistentId {
    fn eq(&self, other: &str) -> bool {
        other.parse::<Self>().ok() == Some(*self)
    }
}

impl PartialEq<&str> for PersistentId {
    fn eq(&self, other: &&str) -> bool {
        (*other).parse::<Self>().ok() == Some(*self)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PersistentId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PersistentId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<Self>().map_err(serde::de::Error::custom)
    }
}

/// Conversion helper trait allowing [`Library`] lookup methods (`track_by_id`, `album_by_id`, etc.)
/// to accept a [`PersistentId`], `&PersistentId`, `u64`, or hexadecimal `&str`/`&String`.
pub trait IntoPersistentId {
    /// Converts `self` into a [`PersistentId`] if valid.
    fn into_persistent_id(self) -> Option<PersistentId>;
}

impl IntoPersistentId for PersistentId {
    fn into_persistent_id(self) -> Option<PersistentId> {
        Some(self)
    }
}

impl IntoPersistentId for &PersistentId {
    fn into_persistent_id(self) -> Option<PersistentId> {
        Some(*self)
    }
}

impl IntoPersistentId for u64 {
    fn into_persistent_id(self) -> Option<PersistentId> {
        Some(PersistentId(self))
    }
}

impl IntoPersistentId for &str {
    fn into_persistent_id(self) -> Option<PersistentId> {
        self.parse::<PersistentId>().ok()
    }
}

impl IntoPersistentId for &String {
    fn into_persistent_id(self) -> Option<PersistentId> {
        self.as_str().parse::<PersistentId>().ok()
    }
}

impl IntoPersistentId for String {
    fn into_persistent_id(self) -> Option<PersistentId> {
        self.as_str().parse::<PersistentId>().ok()
    }
}

bitflags! {
    /// Raw bitflags for `suggestion_flag` (`itma` offset 62..64, `iama` offset 42..44, `iAma` offset 101..103, `lpma` offset 223..225).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct SuggestionFlags: u16 {
        /// Bit 0 (`0x0001`): Set on Removed (`0x0001`), SuggestLess (`0x0003`), and locally modified SuggestLess (`0x0101`).
        const BIT_0            = 0x0001;
        /// Bit 1 (`0x0002`): Set on Favorite (`0x0002`, `0x0102`) and synced SuggestLess (`0x0003`, `0x0103`).
        const FAVORITE         = 0x0002;
        /// Bits 0..=1 (`0x0003`): Synced SuggestLess (`BIT_0 | FAVORITE`).
        const SUGGEST_LESS     = Self::BIT_0.bits() | Self::FAVORITE.bits();
        /// Bit 8 (`0x0100`): Local user-modification flag set when modified before/without cloud sync.
        const LOCALLY_MODIFIED = 0x0100;
    }
}

/// User recommendation / favorite state (`Neutral`, `Favorite`, `SuggestLess`, `Removed`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SuggestionFlag {
    /// `0`: Default / unrated state.
    #[default]
    Neutral,
    /// `1`: Removed from library.
    Removed,
    /// `2` (`0x0002` or `0x0102`): Favorited / loved by the user.
    Favorite,
    /// `3` (`0x0003` or `0x0101` / `0x0103`): Marked "Suggest Less" / disliked by the user.
    SuggestLess,
    /// Unrecognized raw `u16` value.
    Unknown(u16),
}

impl From<u16> for SuggestionFlag {
    fn from(v: u16) -> Self {
        let flags = SuggestionFlags::from_bits_retain(v);
        let local = flags.contains(SuggestionFlags::LOCALLY_MODIFIED);
        let base = flags & !SuggestionFlags::LOCALLY_MODIFIED;

        if base.is_empty() && !local {
            Self::Neutral
        } else if base == SuggestionFlags::FAVORITE {
            Self::Favorite
        } else if base == SuggestionFlags::SUGGEST_LESS || (base == SuggestionFlags::BIT_0 && local)
        {
            Self::SuggestLess
        } else if base == SuggestionFlags::BIT_0 {
            Self::Removed
        } else {
            Self::Unknown(v)
        }
    }
}

impl SuggestionFlag {
    /// Returns the canonical raw `u16` code for this [`SuggestionFlag`].
    #[must_use]
    pub const fn raw(self) -> u16 {
        match self {
            Self::Neutral => 0,
            Self::Removed => 1,
            Self::Favorite => 2,
            Self::SuggestLess => 3,
            Self::Unknown(v) => v,
        }
    }

    /// Returns the [`SuggestionFlags`] bitmask representation of a raw `suggestion_flag` value.
    #[must_use]
    pub const fn flags(raw: u16) -> SuggestionFlags {
        SuggestionFlags::from_bits_retain(raw)
    }

    /// Returns `true` if the `0x0100` local user-modified bit is set (e.g. `0x0102` when favorited offline, `0x0101` when marked Suggest Less offline).
    #[must_use]
    pub const fn is_locally_modified(raw: u16) -> bool {
        SuggestionFlags::from_bits_retain(raw).contains(SuggestionFlags::LOCALLY_MODIFIED)
    }
}

impl fmt::Display for SuggestionFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Neutral => f.write_str("Neutral"),
            Self::Removed => f.write_str("Removed"),
            Self::Favorite => f.write_str("Favorite"),
            Self::SuggestLess => f.write_str("SuggestLess"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// iCloud Music Library status (`itma` offset 58, Smart Playlist field `0x86` = 134).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CloudStatus {
    /// `0`: Local track when iCloud Music Library is disabled or not applicable (`LocalOnly`).
    #[default]
    LocalOnly,
    /// `1`: Purchased from the iTunes Store (`Purchased`).
    Purchased,
    /// `2`: Local track matched by iTunes Match / Apple Music (`Matched`).
    Matched,
    /// `3`: Local track uploaded directly to iCloud Music Library (`Uploaded`).
    Uploaded,
    /// `4`: Track is ineligible for iCloud Music Library (`Ineligible`).
    Ineligible,
    /// `5`: Track failed to upload/match or was removed (`Error` / `Removed`).
    Error,
    /// `6`: Cloud sync error (`SyncError`).
    SyncError,
    /// `7`: Duplicate track skipped by iCloud Music Library (`Duplicate`).
    Duplicate,
    /// `8`: Apple Music subscription catalog track (`Apple Music`).
    AppleMusic,
    /// `9`: Track removed from Apple Music catalog (`No Longer Available`).
    NoLongerAvailable,
    /// `10`: Track not uploaded (`NotUploaded`).
    NotUploaded,
    /// Any other status value.
    Unknown(u8),
}

impl CloudStatus {
    /// Returns the raw `u8` status code stored at `itma` offset 58.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::LocalOnly => 0,
            Self::Purchased => 1,
            Self::Matched => 2,
            Self::Uploaded => 3,
            Self::Ineligible => 4,
            Self::Error => 5,
            Self::SyncError => 6,
            Self::Duplicate => 7,
            Self::AppleMusic => 8,
            Self::NoLongerAvailable => 9,
            Self::NotUploaded => 10,
            Self::Unknown(v) => v,
        }
    }
}

impl From<u8> for CloudStatus {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::LocalOnly,
            1 => Self::Purchased,
            2 => Self::Matched,
            3 => Self::Uploaded,
            4 => Self::Ineligible,
            5 => Self::Error,
            6 => Self::SyncError,
            7 => Self::Duplicate,
            8 => Self::AppleMusic,
            9 => Self::NoLongerAvailable,
            10 => Self::NotUploaded,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for CloudStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalOnly => f.write_str("LocalOnly"),
            Self::Purchased => f.write_str("Purchased"),
            Self::Matched => f.write_str("Matched"),
            Self::Uploaded => f.write_str("Uploaded"),
            Self::Ineligible => f.write_str("Ineligible"),
            Self::Error => f.write_str("Error"),
            Self::SyncError => f.write_str("SyncError"),
            Self::Duplicate => f.write_str("Duplicate"),
            Self::AppleMusic => f.write_str("AppleMusic"),
            Self::NoLongerAvailable => f.write_str("NoLongerAvailable"),
            Self::NotUploaded => f.write_str("NotUploaded"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Content advisory rating (`itma` offset 59).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ContentRating {
    /// `0`: No content advisory rating.
    #[default]
    None,
    /// `1`: Explicit content.
    Explicit,
    /// `2`: Clean version.
    Clean,
    /// `4`: Parental guidance suggested.
    ParentalGuidance,
    /// Unrecognized raw `u8` content rating value.
    Unknown(u8),
}

impl ContentRating {
    /// Returns the raw `u8` rating code stored at `itma` offset 59.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Explicit => 1,
            Self::Clean => 2,
            Self::ParentalGuidance => 4,
            Self::Unknown(v) => v,
        }
    }
}

impl From<u8> for ContentRating {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Explicit,
            2 => Self::Clean,
            4 => Self::ParentalGuidance,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for ContentRating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Explicit => f.write_str("Explicit"),
            Self::Clean => f.write_str("Clean"),
            Self::ParentalGuidance => f.write_str("ParentalGuidance"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Local download vs. cloud-only status (`itma` offset 57).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DownloadedStatus {
    /// `0`: Unspecified / unset download state (observed on legacy local tracks in macOS Monterey).
    Unspecified,
    /// `1`: Downloaded locally on disk.
    #[default]
    DownloadedLocal,
    /// `3`: Stored in cloud / not downloaded locally.
    CloudOnly,
    /// `5`: Track is no longer available in the Apple Music catalog.
    NoLongerAvailable,
    /// Unrecognized raw `u8` value.
    Unknown(u8),
}

impl DownloadedStatus {
    /// Returns the raw `u8` status code stored at `itma` offset 57.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Unspecified => 0,
            Self::DownloadedLocal => 1,
            Self::CloudOnly => 3,
            Self::NoLongerAvailable => 5,
            Self::Unknown(v) => v,
        }
    }
}

impl From<u8> for DownloadedStatus {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Unspecified,
            1 => Self::DownloadedLocal,
            3 => Self::CloudOnly,
            5 => Self::NoLongerAvailable,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for DownloadedStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unspecified => f.write_str("Unspecified"),
            Self::DownloadedLocal => f.write_str("DownloadedLocal"),
            Self::CloudOnly => f.write_str("CloudOnly"),
            Self::NoLongerAvailable => f.write_str("NoLongerAvailable"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

bitflags! {
    /// Apple Music catalog `audioTraits` bitmask (`itma` offset 331).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct AudioTraits: u8 {
        /// `0x01`: Dolby Atmos spatial audio (`atmos`).
        const DOLBY_ATMOS     = 0x01;
        /// `0x02`: Dolby Audio / Surround 5.1 (`surround`).
        const DOLBY_AUDIO     = 0x02;
        /// `0x04`: ALAC Lossless audio (`lossless`).
        const LOSSLESS        = 0x04;
        /// `0x08`: Lossy stereo / AAC audio (`lossy-stereo`).
        const LOSSY_STEREO    = 0x08;
        /// `0x10`: Hi-Res ALAC Lossless audio (`hi-res-lossless`).
        const HI_RES_LOSSLESS = 0x10;
        /// `0x20`: Spatial Audio capability flag (`spatial`, set with Dolby Atmos or Dolby Audio).
        const SPATIAL_AUDIO   = 0x20;
    }
}

impl fmt::Display for AudioTraits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("None");
        }
        let mut first = true;
        let known = [
            (Self::DOLBY_ATMOS, "DolbyAtmos"),
            (Self::DOLBY_AUDIO, "DolbyAudio"),
            (Self::HI_RES_LOSSLESS, "HiResLossless"),
            (Self::LOSSLESS, "Lossless"),
            (Self::LOSSY_STEREO, "LossyStereo"),
            (Self::SPATIAL_AUDIO, "SpatialAudio"),
        ];
        for (flag, name) in known {
            if self.contains(flag) {
                if !first {
                    f.write_str(", ")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        let unknown_bits = self.bits() & !Self::all().bits();
        if unknown_bits != 0 {
            if !first {
                f.write_str(", ")?;
            }
            write!(f, "Unknown(0x{unknown_bits:02X})")?;
        }
        Ok(())
    }
}

/// Apple Music catalog album release / edition type (`iama` offset 136..140).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CatalogAlbumType {
    /// `0`: None / local album without catalog release type metadata.
    #[default]
    None,
    /// `1`: Standard full-length album.
    StandardAlbum,
    /// `3`: Bonus, Deluxe, or Expanded Edition.
    BonusEdition,
    /// `4`: Extended Play (EP).
    Ep,
    /// `5`: Single.
    Single,
    /// Unrecognized raw `u32` catalog album type.
    Unknown(u32),
}

impl CatalogAlbumType {
    /// Returns the raw `u32` code stored at `iama` offset 136..140.
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::None => 0,
            Self::StandardAlbum => 1,
            Self::BonusEdition => 3,
            Self::Ep => 4,
            Self::Single => 5,
            Self::Unknown(v) => v,
        }
    }
}

impl From<u32> for CatalogAlbumType {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::StandardAlbum,
            3 => Self::BonusEdition,
            4 => Self::Ep,
            5 => Self::Single,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for CatalogAlbumType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::StandardAlbum => f.write_str("StandardAlbum"),
            Self::BonusEdition => f.write_str("BonusEdition"),
            Self::Ep => f.write_str("Ep"),
            Self::Single => f.write_str("Single"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Pending offline cloud deletion mode (`dCma` offset 16).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CloudDeletionKind {
    /// `0`: Full deletion from the user's iCloud Music Library.
    #[default]
    FullLibraryDeletion,
    /// `2`: Removed from main library while retained as a playlist-only track.
    KeepPlaylistOnly,
    /// Unrecognized raw `u8` deletion kind.
    Unknown(u8),
}

impl CloudDeletionKind {
    /// Returns the raw `u8` code stored at `dCma` offset 16.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::FullLibraryDeletion => 0,
            Self::KeepPlaylistOnly => 2,
            Self::Unknown(v) => v,
        }
    }
}

impl From<u8> for CloudDeletionKind {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::FullLibraryDeletion,
            2 => Self::KeepPlaylistOnly,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for CloudDeletionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FullLibraryDeletion => f.write_str("FullLibraryDeletion"),
            Self::KeepPlaylistOnly => f.write_str("KeepPlaylistOnly"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// A pending offline iCloud Music Library track deletion queued in `DCma` / `dCma` (`hsma` subtype 8).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PendingCloudDeletion {
    /// iCloud Music Library `sagaId` of the deleted track (`dCma` offset 8..16).
    pub cloud_saga_id: u64,
    /// Deletion mode (`dCma` offset 16).
    pub kind: CloudDeletionKind,
}

impl From<&CloudDeletionItem> for PendingCloudDeletion {
    fn from(item: &CloudDeletionItem) -> Self {
        Self {
            cloud_saga_id: item.cloud_saga_id,
            kind: CloudDeletionKind::from(item.deletion_kind),
        }
    }
}

bitflags! {
    /// Raw bitflags for a track's media kind (`itma` offset 108..112, Smart Playlist field `0x3C` = 60).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct MediaKindFlags: u32 {
        /// `0x0000_0001`: Music audio track.
        const MUSIC         = 0x0000_0001;
        /// `0x0000_0002`: Movie video track.
        const MOVIE         = 0x0000_0002;
        /// `0x0000_0004`: Podcast episode.
        const PODCAST       = 0x0000_0004;
        /// `0x0000_0008`: Audiobook track.
        const AUDIOBOOK     = 0x0000_0008;
        /// `0x0000_0010`: Digital PDF booklet.
        const BOOKLET_PDF   = 0x0000_0010;
        /// `0x0000_0020`: Music video track.
        const MUSIC_VIDEO   = 0x0000_0020;
        /// `0x0000_0040`: TV show episode.
        const TV_SHOW       = 0x0000_0040;
        /// `0x0000_0080`: Stage play.
        const STAGE_PLAY    = 0x0000_0080;
        /// `0x0000_0100`: Interactive digital book.
        const DIGITAL_BOOK  = 0x0000_0100;
        /// `0x0000_0400`: Home video.
        const HOME_VIDEO    = 0x0000_0400;
        /// `0x0000_4000`: Ringtone / alert tone.
        const RINGTONE      = 0x0000_4000;
        /// `0x0000_8000`: Digital audio item.
        const DIGITAL_AUDIO = 0x0000_8000;
        /// `0x0001_0000`: iTunes Extras package.
        const ITUNES_EXTRAS = 0x0001_0000;
        /// `0x0010_0000`: Voice memo recording.
        const VOICE_MEMO    = 0x0010_0000;
        /// `0x0020_0000`: iTunes U educational media.
        const ITUNES_U      = 0x0020_0000;

        /// Composite mask (`0x0000_0005`): Music + Podcast.
        const MUSIC_OR_PODCAST = Self::MUSIC.bits() | Self::PODCAST.bits();
        /// Composite mask (`0x0000_0042`): Movie + TV Show.
        const MOVIE_OR_TV_SHOW = Self::MOVIE.bits() | Self::TV_SHOW.bits();
        /// Composite mask (`0x0010_21B1`): All Music & Audio in Smart Playlists.
        const ALL_MUSIC_AND_AUDIO = 0x0010_21B1;
        /// Composite mask (`0x0020_8004`): Podcast + iTunes U + Digital Audio.
        const PODCAST_OR_VOICE_MEMO = Self::PODCAST.bits() | Self::DIGITAL_AUDIO.bits() | Self::ITUNES_U.bits();
        /// Composite mask (`0x0021_8004`): Podcast + iTunes Extras + iTunes U + Digital Audio.
        const PODCAST_ITUNES_U_OR_VOICE_MEMO = Self::PODCAST_OR_VOICE_MEMO.bits() | Self::ITUNES_EXTRAS.bits();
    }
}

/// Media kind (`itma` offset 108..112, Smart Playlist field `0x3C` = 60).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MediaKind {
    /// Standard music track (`0x01`).
    #[default]
    Music,
    /// Feature film / movie (`0x02`).
    Movie,
    /// Podcast episode (`0x04`).
    Podcast,
    /// Music or Podcast composite (`0x05`).
    MusicOrPodcast,
    /// Audiobook (`0x08`).
    Audiobook,
    /// Digital booklet PDF (`0x10`).
    BookletPdf,
    /// Music video (`0x20`).
    MusicVideo,
    /// TV show episode (`0x40`).
    TvShow,
    /// Movie or TV show composite (`0x42`).
    MovieOrTvShow,
    /// Home video (`0x400`).
    HomeVideo,
    /// Ringtone (`0x4000`).
    Ringtone,
    /// iTunes Extras (`0x10000`).
    ItunesExtras,
    /// Voice Memo (`0x100000`).
    VoiceMemo,
    /// Smart playlist mask matching all music and audio (`0x0010_21B1`).
    AllMusicAndAudio,
    /// Smart playlist mask matching podcasts and voice memos (`0x0020_8004`).
    PodcastOrVoiceMemo,
    /// Smart playlist mask matching podcasts, iTunes U, and voice memos (`0x0021_8004`).
    PodcastItunesUOrVoiceMemo,
    /// Unrecognized raw bitmask value.
    Unknown(u32),
}

impl MediaKind {
    /// Returns the raw `u32` media kind bitmask stored at `itma` offset 108..112.
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Music => MediaKindFlags::MUSIC.bits(),
            Self::Movie => MediaKindFlags::MOVIE.bits(),
            Self::Podcast => MediaKindFlags::PODCAST.bits(),
            Self::MusicOrPodcast => MediaKindFlags::MUSIC_OR_PODCAST.bits(),
            Self::Audiobook => MediaKindFlags::AUDIOBOOK.bits(),
            Self::BookletPdf => MediaKindFlags::BOOKLET_PDF.bits(),
            Self::MusicVideo => MediaKindFlags::MUSIC_VIDEO.bits(),
            Self::TvShow => MediaKindFlags::TV_SHOW.bits(),
            Self::MovieOrTvShow => MediaKindFlags::MOVIE_OR_TV_SHOW.bits(),
            Self::HomeVideo => MediaKindFlags::HOME_VIDEO.bits(),
            Self::Ringtone => MediaKindFlags::RINGTONE.bits(),
            Self::ItunesExtras => MediaKindFlags::ITUNES_EXTRAS.bits(),
            Self::VoiceMemo => MediaKindFlags::VOICE_MEMO.bits(),
            Self::AllMusicAndAudio => MediaKindFlags::ALL_MUSIC_AND_AUDIO.bits(),
            Self::PodcastOrVoiceMemo => MediaKindFlags::PODCAST_OR_VOICE_MEMO.bits(),
            Self::PodcastItunesUOrVoiceMemo => {
                MediaKindFlags::PODCAST_ITUNES_U_OR_VOICE_MEMO.bits()
            }
            Self::Unknown(v) => v,
        }
    }

    /// Returns the [`MediaKindFlags`] bitmask representation of a raw `media_kind` value.
    #[must_use]
    pub const fn flags(raw: u32) -> MediaKindFlags {
        MediaKindFlags::from_bits_retain(raw)
    }
}

impl From<u32> for MediaKind {
    fn from(v: u32) -> Self {
        match MediaKindFlags::from_bits_retain(v) {
            MediaKindFlags::MUSIC => Self::Music,
            MediaKindFlags::MOVIE => Self::Movie,
            MediaKindFlags::PODCAST => Self::Podcast,
            MediaKindFlags::MUSIC_OR_PODCAST => Self::MusicOrPodcast,
            MediaKindFlags::AUDIOBOOK => Self::Audiobook,
            MediaKindFlags::BOOKLET_PDF => Self::BookletPdf,
            MediaKindFlags::MUSIC_VIDEO => Self::MusicVideo,
            MediaKindFlags::TV_SHOW => Self::TvShow,
            MediaKindFlags::MOVIE_OR_TV_SHOW => Self::MovieOrTvShow,
            MediaKindFlags::HOME_VIDEO => Self::HomeVideo,
            MediaKindFlags::RINGTONE => Self::Ringtone,
            MediaKindFlags::ITUNES_EXTRAS => Self::ItunesExtras,
            MediaKindFlags::VOICE_MEMO => Self::VoiceMemo,
            MediaKindFlags::ALL_MUSIC_AND_AUDIO => Self::AllMusicAndAudio,
            MediaKindFlags::PODCAST_OR_VOICE_MEMO => Self::PodcastOrVoiceMemo,
            MediaKindFlags::PODCAST_ITUNES_U_OR_VOICE_MEMO => Self::PodcastItunesUOrVoiceMemo,
            _ => Self::Unknown(v),
        }
    }
}

impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Music => f.write_str("Music"),
            Self::Movie => f.write_str("Movie"),
            Self::Podcast => f.write_str("Podcast"),
            Self::MusicOrPodcast => f.write_str("MusicOrPodcast"),
            Self::Audiobook => f.write_str("Audiobook"),
            Self::BookletPdf => f.write_str("BookletPdf"),
            Self::MusicVideo => f.write_str("MusicVideo"),
            Self::TvShow => f.write_str("TvShow"),
            Self::MovieOrTvShow => f.write_str("MovieOrTvShow"),
            Self::HomeVideo => f.write_str("HomeVideo"),
            Self::Ringtone => f.write_str("Ringtone"),
            Self::ItunesExtras => f.write_str("ItunesExtras"),
            Self::VoiceMemo => f.write_str("VoiceMemo"),
            Self::AllMusicAndAudio => f.write_str("AllMusicAndAudio"),
            Self::PodcastOrVoiceMemo => f.write_str("PodcastOrVoiceMemo"),
            Self::PodcastItunesUOrVoiceMemo => f.write_str("PodcastItunesUOrVoiceMemo"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Embedded / scanned artwork classification on a track (`TrackNumerics` offset 80..82).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EmbeddedArtworkKind {
    /// `0`: No embedded artwork detected.
    #[default]
    None,
    /// `-1` (`0xFFFF`): Embedded artwork present in the local audio file container.
    Embedded,
    /// `1`: Cloud-sourced artwork reference.
    Cloud,
    /// `128` (`0x0080`): Auto-extracted poster frame from a local video file.
    VideoPosterFrame,
    /// Unrecognized raw `i16` value.
    Unknown(i16),
}

impl EmbeddedArtworkKind {
    /// Returns the raw `i16` value stored at `TrackNumerics` offset 80..82.
    #[must_use]
    pub const fn raw(self) -> i16 {
        match self {
            Self::None => 0,
            Self::Embedded => -1,
            Self::Cloud => 1,
            Self::VideoPosterFrame => 128,
            Self::Unknown(v) => v,
        }
    }

    /// Returns `true` if this value represents local artwork (`Embedded` or `VideoPosterFrame`).
    #[must_use]
    pub const fn has_local_artwork(self) -> bool {
        matches!(self, Self::Embedded | Self::VideoPosterFrame)
    }
}

impl From<i16> for EmbeddedArtworkKind {
    fn from(v: i16) -> Self {
        match v {
            0 => Self::None,
            -1 => Self::Embedded,
            1 => Self::Cloud,
            128 => Self::VideoPosterFrame,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for EmbeddedArtworkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Embedded => f.write_str("Embedded"),
            Self::Cloud => f.write_str("Cloud"),
            Self::VideoPosterFrame => f.write_str("VideoPosterFrame"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Decoded video stream metadata for a video track (`boma 0x0024` [`VideoInfo`]).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackVideoInfo {
    /// Frame width in pixels (`VideoInfo` offset 0..4).
    pub width: u32,
    /// Frame height in pixels (`VideoInfo` offset 4..8).
    pub height: u32,
    /// 4-character video codec code (e.g. `"avc1"`, `VideoInfo` offset 12..16).
    pub codec_fourcc: Option<String>,
    /// Video stream bit rate in kbps (`VideoInfo` offset 20..24).
    pub video_bit_rate_kbps: u32,
    /// H.264/AVC `profile_idc` (e.g. `100` = High Profile, `VideoInfo` offset 32..36).
    pub avc_profile_idc: u32,
    /// H.264/AVC `level_idc` (e.g. `31` = Level 3.1, `40` = Level 4.0, `VideoInfo` offset 36..40).
    pub avc_level_idc: u32,
    /// Frame rate in milli-frames per second (e.g. `25_000` = 25 fps, `29_970` = 29.97 fps, `VideoInfo` offset 44..48).
    pub frame_rate_milli: u32,
    /// Color bit depth (e.g. `24` for 8-bit RGB/YUV, `VideoInfo` offset 48..52).
    pub color_bit_depth: u32,
}

impl TrackVideoInfo {
    /// Returns the frame rate in frames per second (`frame_rate_milli / 1000.0`).
    #[must_use]
    pub fn frame_rate_fps(&self) -> f64 {
        f64::from(self.frame_rate_milli) / 1000.0
    }
}

impl From<&VideoInfo> for TrackVideoInfo {
    fn from(v: &VideoInfo) -> Self {
        Self {
            width: v.width,
            height: v.height,
            codec_fourcc: v.codec_fourcc_str(),
            video_bit_rate_kbps: v.video_bit_rate_kbps,
            avc_profile_idc: v.avc_profile_idc,
            avc_level_idc: v.avc_level_idc,
            frame_rate_milli: v.frame_rate_milli,
            color_bit_depth: v.color_bit_depth,
        }
    }
}

bitflags! {
    /// Raw bitflags for a sortable text field's sort mode (`itma` offset 66..72).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct SortModeFlags: u8 {
        /// Bit 0 (`0x01`): A sort string (`boma` `0x001E..0x0022`) is populated on the track.
        const HAS_SORT_STRING = 0x01;
        /// Bit 1 (`0x02`): User explicitly customized/overrode the sort string.
        const CUSTOM_OVERRIDE = 0x02;
        /// Bit 7 (`0x80`): Sort string was auto-generated by Apple Music (e.g. stripping leading `"The "` or `"A "`).
        const AUTO_GENERATED  = 0x80;
    }
}

/// Sort string generation mode (`itma` offset 66..72).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SortMode {
    /// `0x00`: No sort string override is present.
    #[default]
    None,
    /// `0x80` / `0x81`: Auto-generated sort string (e.g. stripping leading `"The "`).
    AutoGenerated,
    /// `0x01` / `0x03`: Explicit catalog-supplied (`0x01`) or user/tag custom (`0x03`) sort string override.
    CustomUserOverride,
    /// Any other raw sort mode byte.
    Unknown(u8),
}

impl SortMode {
    /// Returns the raw byte representation of this [`SortMode`].
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::None => 0x00,
            Self::AutoGenerated => 0x80,
            Self::CustomUserOverride => 0x01,
            Self::Unknown(v) => v,
        }
    }

    /// Returns the [`SortModeFlags`] bitmask representation of a raw sort mode byte.
    #[must_use]
    pub const fn flags(raw: u8) -> SortModeFlags {
        SortModeFlags::from_bits_retain(raw)
    }
}

impl From<u8> for SortMode {
    fn from(v: u8) -> Self {
        let flags = SortModeFlags::from_bits_retain(v);
        if flags.is_empty() {
            Self::None
        } else if flags.contains(SortModeFlags::AUTO_GENERATED)
            && (flags & !(SortModeFlags::AUTO_GENERATED | SortModeFlags::HAS_SORT_STRING))
                .is_empty()
        {
            Self::AutoGenerated
        } else if flags.contains(SortModeFlags::HAS_SORT_STRING)
            && (flags & !(SortModeFlags::HAS_SORT_STRING | SortModeFlags::CUSTOM_OVERRIDE))
                .is_empty()
        {
            Self::CustomUserOverride
        } else {
            Self::Unknown(v)
        }
    }
}

impl fmt::Display for SortMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::AutoGenerated => f.write_str("AutoGenerated"),
            Self::CustomUserOverride => f.write_str("CustomUserOverride"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Decoded sort modes for a track's 6 sortable text fields (`itma` offset 66..72).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackSortModes {
    /// Sort mode for `sort_title`.
    pub title: SortMode,
    /// Sort mode for `sort_album`.
    pub album: SortMode,
    /// Sort mode for `sort_artist`.
    pub artist: SortMode,
    /// Sort mode for `sort_album_artist`.
    pub album_artist: SortMode,
    /// Sort mode for `sort_composer`.
    pub composer: SortMode,
    /// Sort mode for `sort_show` (`SmartField::SortShow`, offset 71).
    pub show: SortMode,
}

impl From<[u8; 6]> for TrackSortModes {
    fn from(bytes: [u8; 6]) -> Self {
        Self {
            title: SortMode::from(bytes[0]),
            album: SortMode::from(bytes[1]),
            artist: SortMode::from(bytes[2]),
            album_artist: SortMode::from(bytes[3]),
            composer: SortMode::from(bytes[4]),
            show: SortMode::from(bytes[5]),
        }
    }
}

/// Decoded Equalizer preset (`boma 0x0007`, e.g. `"#!#116#!#"` -> `preset_id: Some(116), name: "Rock"`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EqualizerPreset {
    /// Raw string stored in `boma 0x0007` (e.g. `"#!#116#!#"` or custom preset name).
    pub raw: String,
    /// Numeric built-in preset ID (`100..=121`) if `raw` uses the `#!#<id>#!#` encoding.
    pub preset_id: Option<u32>,
    /// Human-readable equalizer preset name (e.g. `"Rock"`, `"Acoustic"`).
    pub name: String,
}

impl From<&str> for EqualizerPreset {
    fn from(raw: &str) -> Self {
        Self::parse(raw)
    }
}

impl EqualizerPreset {
    /// Parses a raw `boma 0x0007` equalizer string into an [`EqualizerPreset`].
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        if let Some(inner) = raw.strip_prefix("#!#").and_then(|s| s.strip_suffix("#!#"))
            && let Ok(id) = inner.parse::<u32>()
        {
            let name = match id {
                100 => "Acoustic",
                101 => "Bass Booster",
                102 => "Bass Reducer",
                103 => "Classical",
                104 => "Dance",
                105 => "Deep",
                106 => "Electronic",
                107 => "Flat",
                108 => "Hip-Hop",
                109 => "Jazz",
                110 => "Latin",
                111 => "Loudness",
                112 => "Lounge",
                113 => "Piano",
                114 => "Pop",
                115 => "R&B",
                116 => "Rock",
                117 => "Small Speakers",
                118 => "Spoken Word",
                119 => "Treble Booster",
                120 => "Treble Reducer",
                121 => "Vocal Booster",
                _ => "Built-in Preset",
            };
            return Self {
                raw: raw.to_string(),
                preset_id: Some(id),
                name: name.to_string(),
            };
        }
        Self {
            raw: raw.to_string(),
            preset_id: None,
            name: raw.to_string(),
        }
    }
}

/// Cloud visibility / sharing mode of a playlist (`lpma` offset 308..312).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlaylistShareMode {
    /// `0`: Private user playlist.
    #[default]
    Private,
    /// `1`: Subscribed personalized Apple Music playlist (e.g. Apple Music Replay).
    Personalized,
    /// `2`: Subscribed Apple Music catalog playlist.
    SubscribedCatalog,
    /// `4`: Publicly shared user playlist shown on profile.
    PublicShared,
    /// Unrecognized raw `u32` share mode value.
    Unknown(u32),
}

impl PlaylistShareMode {
    /// Returns the raw `u32` share mode code stored at `lpma` offset 308..312.
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Private => 0,
            Self::Personalized => 1,
            Self::SubscribedCatalog => 2,
            Self::PublicShared => 4,
            Self::Unknown(v) => v,
        }
    }
}

impl From<u32> for PlaylistShareMode {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::Private,
            1 => Self::Personalized,
            2 => Self::SubscribedCatalog,
            4 => Self::PublicShared,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for PlaylistShareMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Private => f.write_str("Private"),
            Self::Personalized => f.write_str("Personalized"),
            Self::SubscribedCatalog => f.write_str("SubscribedCatalog"),
            Self::PublicShared => f.write_str("PublicShared"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Built-in Apple Music system playlist kind (`lpma` offset 79..81).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DistinguishedPlaylistKind {
    /// `0`: Regular user playlist or folder (not a distinguished system playlist).
    #[default]
    None,
    /// `2`: Movies system playlist.
    Movies,
    /// `3`: TV Shows system playlist.
    TvShows,
    /// `4`: Music system playlist.
    Music,
    /// `5`: Audiobooks system playlist.
    Audiobooks,
    /// `10`: Podcasts system playlist.
    Podcasts,
    /// `14`: Voice Memos system playlist.
    VoiceMemos,
    /// `19`: Purchased music playlist.
    Purchased,
    /// `22`: Legacy iTunes DJ playlist.
    ITunesDj,
    /// `26`: Genius playlist.
    Genius,
    /// `42`: My Top Rated smart playlist.
    MyTopRated,
    /// `43`: Recently Played smart playlist.
    RecentlyPlayed,
    /// `44`: Recently Added smart playlist.
    RecentlyAdded,
    /// `47`: Music Videos system playlist.
    MusicVideos,
    /// `48`: Classical Music system playlist.
    ClassicalMusic,
    /// `49` / `61`: Favourite Songs system playlist.
    FavoriteSongs,
    /// `63`: Internal `Hidden Cloud PlaylistOnly Tracks` container.
    HiddenCloudPlaylistOnlyTracks,
    /// `64`: TV & Movies system playlist.
    TvAndMovies,
    /// `65` / `66`: Downloaded music system playlist.
    Downloaded,
    /// Any other raw distinguished kind value.
    Other(u16),
}

impl DistinguishedPlaylistKind {
    /// Returns the raw `u16` distinguished kind code stored at `lpma` offset 79..81.
    #[must_use]
    pub const fn raw(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Movies => 2,
            Self::TvShows => 3,
            Self::Music => 4,
            Self::Audiobooks => 5,
            Self::Podcasts => 10,
            Self::VoiceMemos => 14,
            Self::Purchased => 19,
            Self::ITunesDj => 22,
            Self::Genius => 26,
            Self::MyTopRated => 42,
            Self::RecentlyPlayed => 43,
            Self::RecentlyAdded => 44,
            Self::MusicVideos => 47,
            Self::ClassicalMusic => 48,
            Self::FavoriteSongs => 61,
            Self::HiddenCloudPlaylistOnlyTracks => 63,
            Self::TvAndMovies => 64,
            Self::Downloaded => 65,
            Self::Other(v) => v,
        }
    }
}

impl From<u16> for DistinguishedPlaylistKind {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::None,
            2 => Self::Movies,
            3 => Self::TvShows,
            4 => Self::Music,
            5 => Self::Audiobooks,
            10 => Self::Podcasts,
            14 => Self::VoiceMemos,
            19 => Self::Purchased,
            22 => Self::ITunesDj,
            26 => Self::Genius,
            42 => Self::MyTopRated,
            43 => Self::RecentlyPlayed,
            44 => Self::RecentlyAdded,
            47 => Self::MusicVideos,
            48 => Self::ClassicalMusic,
            49 | 61 => Self::FavoriteSongs,
            63 => Self::HiddenCloudPlaylistOnlyTracks,
            64 => Self::TvAndMovies,
            65 | 66 => Self::Downloaded,
            other => Self::Other(other),
        }
    }
}

impl fmt::Display for DistinguishedPlaylistKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Movies => f.write_str("Movies"),
            Self::TvShows => f.write_str("TvShows"),
            Self::Music => f.write_str("Music"),
            Self::Audiobooks => f.write_str("Audiobooks"),
            Self::Podcasts => f.write_str("Podcasts"),
            Self::VoiceMemos => f.write_str("VoiceMemos"),
            Self::Purchased => f.write_str("Purchased"),
            Self::ITunesDj => f.write_str("ITunesDj"),
            Self::Genius => f.write_str("Genius"),
            Self::MyTopRated => f.write_str("MyTopRated"),
            Self::RecentlyPlayed => f.write_str("RecentlyPlayed"),
            Self::RecentlyAdded => f.write_str("RecentlyAdded"),
            Self::MusicVideos => f.write_str("MusicVideos"),
            Self::ClassicalMusic => f.write_str("ClassicalMusic"),
            Self::FavoriteSongs => f.write_str("FavoriteSongs"),
            Self::HiddenCloudPlaylistOnlyTracks => f.write_str("HiddenCloudPlaylistOnlyTracks"),
            Self::TvAndMovies => f.write_str("TvAndMovies"),
            Self::Downloaded => f.write_str("Downloaded"),
            Self::Other(v) => write!(f, "Other({v})"),
        }
    }
}

/// High-level semantic classification of a playlist.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlaylistKind {
    /// Internal master library playlist (`####!####`).
    Master,
    /// Built-in Apple Music system playlist (`Favourite Songs`, `Music Videos`, etc.).
    Distinguished,
    /// Playlist folder containing child playlists/folders.
    Folder,
    /// User-created or system Smart Playlist evaluated via `SLst` rules.
    Smart,
    /// Subscribed Apple Music catalog playlist.
    SubscribedCatalog,
    /// Standard user-curated track playlist.
    #[default]
    Standard,
}

impl fmt::Display for PlaylistKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Master => f.write_str("Master"),
            Self::Distinguished => f.write_str("Distinguished"),
            Self::Folder => f.write_str("Folder"),
            Self::Smart => f.write_str("Smart"),
            Self::SubscribedCatalog => f.write_str("SubscribedCatalog"),
            Self::Standard => f.write_str("Standard"),
        }
    }
}

/// Smart Playlist configuration (`boma 0x00CA` options + `boma 0x00C9` `SLst` rule tree).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartPlaylistConfig {
    /// Whether "Live updating" is enabled.
    pub live_updating: bool,
    /// Whether rule matching is enabled.
    pub match_rules: bool,
    /// Whether "Match only checked items" is enabled.
    pub match_only_checked_items: bool,
    /// Optional maximum item/duration/size limit (`Limit to ...`).
    pub limit: Option<SmartPlaylistLimit>,
    /// Root `SLst` rule group tree.
    pub rule_group: Option<SmartRuleGroup>,
}

/// Maximum item, duration, or storage size limit on a [`SmartPlaylistConfig`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartPlaylistLimit {
    /// Maximum count in units of [`Self::unit`].
    pub count: u32,
    /// Unit for [`Self::count`] (`Items`, `Minutes`, `Hours`, `Megabytes`, `Gigabytes`).
    pub unit: SmartLimitUnit,
    /// Selection ordering when truncating to the limit (`Random`, `MostRecentlyAdded`, etc.).
    pub selection: SmartSelectionMethod,
}

/// Unit for a [`SmartPlaylistLimit`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartLimitUnit {
    /// `1`: Limit by duration in minutes.
    Minutes,
    /// `2`: Limit by size in megabytes (MB).
    Megabytes,
    /// `3`: Limit by number of items/tracks.
    #[default]
    Items,
    /// `4`: Limit by duration in hours.
    Hours,
    /// `5`: Limit by size in gigabytes (GB).
    Gigabytes,
    /// Unrecognized raw `u8` limit unit.
    Unknown(u8),
}

impl From<u8> for SmartLimitUnit {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Minutes,
            2 => Self::Megabytes,
            3 => Self::Items,
            4 => Self::Hours,
            5 => Self::Gigabytes,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for SmartLimitUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Minutes => f.write_str("Minutes"),
            Self::Megabytes => f.write_str("Megabytes"),
            Self::Items => f.write_str("Items"),
            Self::Hours => f.write_str("Hours"),
            Self::Gigabytes => f.write_str("Gigabytes"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Selection ordering method for a [`SmartPlaylistLimit`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartSelectionMethod {
    /// `0x02`: Random selection.
    #[default]
    Random,
    /// `0x05`: Alphabetical by title.
    Title,
    /// `0x06`: Alphabetical by album.
    Album,
    /// `0x07`: Alphabetical by artist.
    Artist,
    /// `0x09`: Alphabetical by genre.
    Genre,
    /// `0x15` (`negate = 0`): Most recently added.
    MostRecentlyAdded,
    /// `0x15` (`negate = 1`): Least recently added.
    LeastRecentlyAdded,
    /// `0x19` (`negate = 0`): Most often played.
    MostOftenPlayed,
    /// `0x19` (`negate = 1`): Least often played.
    LeastOftenPlayed,
    /// `0x1A` (`negate = 0`): Most recently played.
    MostRecentlyPlayed,
    /// `0x1A` (`negate = 1`): Least recently played.
    LeastRecentlyPlayed,
    /// `0x1C` (`negate = 0`): Highest star rating.
    HighestRating,
    /// `0x1C` (`negate = 1`): Lowest star rating.
    LowestRating,
    /// `0x7B` (`negate = 0`): Most recently favorited (inferred from Apple Music's built-in `"Favourite Songs"` smart playlist).
    MostRecentlyFavorited,
    /// `0x7B` (`negate = 1`): Least recently favorited (inferred from `0x7B` with `negate = 1`).
    LeastRecentlyFavorited,
    /// Unrecognized raw `(method, negate)` pair.
    Unknown {
        /// Raw `limit_selection_method` byte.
        method: u8,
        /// Raw `negate_limit_selection_method` byte.
        negate: u8,
    },
}

impl From<(u8, u8)> for SmartSelectionMethod {
    fn from((method, negate): (u8, u8)) -> Self {
        match (method, negate) {
            (0x02, _) => Self::Random,
            (0x05, _) => Self::Title,
            (0x06, _) => Self::Album,
            (0x07, _) => Self::Artist,
            (0x09, _) => Self::Genre,
            (0x15, 0) => Self::MostRecentlyAdded,
            (0x15, _) => Self::LeastRecentlyAdded,
            (0x19, 0) => Self::MostOftenPlayed,
            (0x19, _) => Self::LeastOftenPlayed,
            (0x1A, 0) => Self::MostRecentlyPlayed,
            (0x1A, _) => Self::LeastRecentlyPlayed,
            (0x1C, 0) => Self::HighestRating,
            (0x1C, _) => Self::LowestRating,
            (0x7B, 0) => Self::MostRecentlyFavorited,
            (0x7B, _) => Self::LeastRecentlyFavorited,
            (m, n) => Self::Unknown {
                method: m,
                negate: n,
            },
        }
    }
}

impl fmt::Display for SmartSelectionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random => f.write_str("Random"),
            Self::Title => f.write_str("Title"),
            Self::Album => f.write_str("Album"),
            Self::Artist => f.write_str("Artist"),
            Self::Genre => f.write_str("Genre"),
            Self::MostRecentlyAdded => f.write_str("MostRecentlyAdded"),
            Self::LeastRecentlyAdded => f.write_str("LeastRecentlyAdded"),
            Self::MostOftenPlayed => f.write_str("MostOftenPlayed"),
            Self::LeastOftenPlayed => f.write_str("LeastOftenPlayed"),
            Self::MostRecentlyPlayed => f.write_str("MostRecentlyPlayed"),
            Self::LeastRecentlyPlayed => f.write_str("LeastRecentlyPlayed"),
            Self::HighestRating => f.write_str("HighestRating"),
            Self::LowestRating => f.write_str("LowestRating"),
            Self::MostRecentlyFavorited => f.write_str("MostRecentlyFavorited"),
            Self::LeastRecentlyFavorited => f.write_str("LeastRecentlyFavorited"),
            Self::Unknown { method, negate } => {
                write!(f, "Unknown {{ method: {method}, negate: {negate} }}")
            }
        }
    }
}

/// Logical conjunction (`All` = `AND`, `Any` = `OR`) of a [`SmartRuleGroup`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartConjunction {
    /// `0`: Match ALL of the child rules (`AND`).
    #[default]
    All,
    /// `1`: Match ANY of the child rules (`OR`).
    Any,
}

impl fmt::Display for SmartConjunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("All"),
            Self::Any => f.write_str("Any"),
        }
    }
}

/// A group of Smart Playlist rules joined by a [`SmartConjunction`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartRuleGroup {
    /// Logical conjunction (`All` or `Any`) combining `rules`.
    pub conjunction: SmartConjunction,
    /// Whether this nested rule group is logically negated (`NOT`, bit `0x0200_0000` on the enclosing `SLst` rule's `comparison_method`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub negated: bool,
    /// Ordered child rules or nested rule groups.
    pub rules: Vec<SmartRuleNode>,
}

/// A node in a [`SmartRuleGroup`] tree (either a nested group or a leaf condition).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "node_type"))]
pub enum SmartRuleNode {
    /// Nested `SLst` rule group (`AND` / `OR` sub-tree).
    Group(SmartRuleGroup),
    /// Single leaf comparison rule.
    Condition(SmartCondition),
}

/// Target metadata field of a [`SmartCondition`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartField {
    /// `0x02`: Track title.
    Title,
    /// `0x03`: Album title.
    Album,
    /// `0x04`: Artist name.
    Artist,
    /// `0x05`: Audio bit rate in kbps.
    BitRate,
    /// `0x06`: Audio sample rate in Hz.
    SampleRate,
    /// `0x07`: Release year.
    Year,
    /// `0x08`: Genre name.
    Genre,
    /// `0x09`: File kind description string.
    Kind,
    /// `0x0A`: Date modified.
    DateModified,
    /// `0x0B`: Track number on disc.
    TrackNumber,
    /// `0x0C` / `0x1C`: File size.
    Size,
    /// `0x0D`: Track duration.
    Time,
    /// `0x0E`: Comments text.
    Comments,
    /// `0x10`: Date added to library.
    DateAdded,
    /// `0x12`: Composer name.
    Composer,
    /// `0x16`: Play count.
    Plays,
    /// `0x17`: Date last played.
    LastPlayed,
    /// `0x18`: Disc number.
    DiscNumber,
    /// `0x19`: User star rating.
    Rating,
    /// `0x1D`: Track checkbox state (`Checked`).
    Checked,
    /// `0x1F`: Compilation flag.
    Compilation,
    /// `0x23`: Beats per minute (BPM).
    Bpm,
    /// `0x25`: Has album artwork boolean.
    AlbumArtwork,
    /// `0x27`: Grouping text.
    Grouping,
    /// `0x28`: Membership in another playlist (`Playlist is / is not ...`).
    Playlist,
    /// `0x29`: Purchased flag.
    Purchased,
    /// `0x36`: Description text.
    Description,
    /// `0x37`: Category text.
    Category,
    /// `0x39` / `0x5A`: Album star rating.
    AlbumRating,
    /// `0x3C`: Media kind bitmask.
    MediaKind,
    /// `0x44`: Skip count.
    Skips,
    /// `0x45`: Date last skipped.
    LastSkipped,
    /// `0x47`: Album artist name.
    AlbumArtist,
    /// `0x4E`: Sort title.
    SortTitle,
    /// `0x4F`: Sort album.
    SortAlbum,
    /// `0x50`: Sort artist.
    SortArtist,
    /// `0x51`: Sort album artist.
    SortAlbumArtist,
    /// `0x52`: Sort composer.
    SortComposer,
    /// `0x53`: Sort show.
    SortShow,
    /// `0x59`: Video rating.
    VideoRating,
    /// `0x85`: Track storage location (`Computer`, `iCloud`).
    Location,
    /// `0x86`: iCloud Music Library status (`Matched`, `Uploaded`, `Apple Music`, etc.).
    CloudStatus,
    /// `0x9A`: Track favorite / suggest-less flag.
    Favorite,
    /// `0x9C`: Album favorite / suggest-less flag.
    AlbumFavorite,
    /// `0x9E` / `0x9F`: Classical work name.
    Work,
    /// `0xA0`: Classical movement name.
    MovementName,
    /// `0xA1`: Classical movement number.
    MovementNumber,
    /// `0xA4`: Has video boolean.
    HasVideo,
    /// Unrecognized `SLst` field ID.
    Unknown(u32),
}

impl From<u32> for SmartField {
    fn from(id: u32) -> Self {
        match id {
            0x02 => Self::Title,
            0x03 => Self::Album,
            0x04 => Self::Artist,
            0x05 => Self::BitRate,
            0x06 => Self::SampleRate,
            0x07 => Self::Year,
            0x08 => Self::Genre,
            0x09 => Self::Kind,
            0x0A => Self::DateModified,
            0x0B => Self::TrackNumber,
            0x0C | 0x1C => Self::Size,
            0x0D => Self::Time,
            0x0E => Self::Comments,
            0x10 => Self::DateAdded,
            0x12 => Self::Composer,
            0x16 => Self::Plays,
            0x17 => Self::LastPlayed,
            0x18 => Self::DiscNumber,
            0x19 => Self::Rating,
            0x1D => Self::Checked,
            0x1F => Self::Compilation,
            0x23 => Self::Bpm,
            0x25 => Self::AlbumArtwork,
            0x27 => Self::Grouping,
            0x28 => Self::Playlist,
            0x29 => Self::Purchased,
            0x36 => Self::Description,
            0x37 => Self::Category,
            0x39 | 0x5A => Self::AlbumRating,
            0x3C => Self::MediaKind,
            0x44 => Self::Skips,
            0x45 => Self::LastSkipped,
            0x47 => Self::AlbumArtist,
            0x4E => Self::SortTitle,
            0x4F => Self::SortAlbum,
            0x50 => Self::SortArtist,
            0x51 => Self::SortAlbumArtist,
            0x52 => Self::SortComposer,
            0x53 => Self::SortShow,
            0x59 => Self::VideoRating,
            0x85 => Self::Location,
            0x86 => Self::CloudStatus,
            0x9A => Self::Favorite,
            0x9C => Self::AlbumFavorite,
            0x9E | 0x9F => Self::Work,
            0xA0 => Self::MovementName,
            0xA1 => Self::MovementNumber,
            0xA4 => Self::HasVideo,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for SmartField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Title => f.write_str("Title"),
            Self::Album => f.write_str("Album"),
            Self::Artist => f.write_str("Artist"),
            Self::BitRate => f.write_str("BitRate"),
            Self::SampleRate => f.write_str("SampleRate"),
            Self::Year => f.write_str("Year"),
            Self::Genre => f.write_str("Genre"),
            Self::Kind => f.write_str("Kind"),
            Self::DateModified => f.write_str("DateModified"),
            Self::TrackNumber => f.write_str("TrackNumber"),
            Self::Size => f.write_str("Size"),
            Self::Time => f.write_str("Time"),
            Self::Comments => f.write_str("Comments"),
            Self::DateAdded => f.write_str("DateAdded"),
            Self::Composer => f.write_str("Composer"),
            Self::Plays => f.write_str("Plays"),
            Self::LastPlayed => f.write_str("LastPlayed"),
            Self::DiscNumber => f.write_str("DiscNumber"),
            Self::Rating => f.write_str("Rating"),
            Self::Checked => f.write_str("Checked"),
            Self::Compilation => f.write_str("Compilation"),
            Self::Bpm => f.write_str("Bpm"),
            Self::AlbumArtwork => f.write_str("AlbumArtwork"),
            Self::Grouping => f.write_str("Grouping"),
            Self::Playlist => f.write_str("Playlist"),
            Self::Purchased => f.write_str("Purchased"),
            Self::Description => f.write_str("Description"),
            Self::Category => f.write_str("Category"),
            Self::AlbumRating => f.write_str("AlbumRating"),
            Self::MediaKind => f.write_str("MediaKind"),
            Self::Skips => f.write_str("Skips"),
            Self::LastSkipped => f.write_str("LastSkipped"),
            Self::AlbumArtist => f.write_str("AlbumArtist"),
            Self::SortTitle => f.write_str("SortTitle"),
            Self::SortAlbum => f.write_str("SortAlbum"),
            Self::SortArtist => f.write_str("SortArtist"),
            Self::SortAlbumArtist => f.write_str("SortAlbumArtist"),
            Self::SortComposer => f.write_str("SortComposer"),
            Self::SortShow => f.write_str("SortShow"),
            Self::VideoRating => f.write_str("VideoRating"),
            Self::Location => f.write_str("Location"),
            Self::CloudStatus => f.write_str("CloudStatus"),
            Self::Favorite => f.write_str("Favorite"),
            Self::AlbumFavorite => f.write_str("AlbumFavorite"),
            Self::Work => f.write_str("Work"),
            Self::MovementName => f.write_str("MovementName"),
            Self::MovementNumber => f.write_str("MovementNumber"),
            Self::HasVideo => f.write_str("HasVideo"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

bitflags! {
    /// Raw bitflags for a Smart Playlist comparison operator (`SLst` rule offset 4..8, Big-Endian).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct SmartOperatorFlags: u32 {
        /// `0x0000_0001`: Equality (`is`).
        const IS            = 0x0000_0001;
        /// `0x0000_0002`: Substring match (`contains`).
        const CONTAINS      = 0x0000_0002;
        /// `0x0000_0004`: Prefix match (`starts with`).
        const STARTS_WITH   = 0x0000_0004;
        /// `0x0000_0008`: Suffix match (`ends with`).
        const ENDS_WITH     = 0x0000_0008;
        /// `0x0000_0010`: Strictly greater than (`is greater than`).
        const GREATER_THAN  = 0x0000_0010;
        /// `0x0000_0020`: Greater than or equal to.
        const GREATER_OR_EQ = 0x0000_0020;
        /// `0x0000_0040`: Strictly less than (`is less than`).
        const LESS_THAN     = 0x0000_0040;
        /// `0x0000_0080`: Less than or equal to.
        const LESS_OR_EQ    = 0x0000_0080;
        /// `0x0000_0100`: Inclusive range (`is in the range`).
        const IN_RANGE      = 0x0000_0100;
        /// `0x0000_0200`: Relative time window (`is in the last`).
        const IN_THE_LAST   = 0x0000_0200;
        /// `0x0000_0400`: Bitmask intersection (`MediaKind`).
        const BITWISE_SET   = 0x0000_0400;
        /// `0x0100_0000`: UTF-16BE string comparison mode.
        const STRING_MODE   = 0x0100_0000;
        /// `0x0200_0000`: Logical negation (`not`).
        const NEGATE        = 0x0200_0000;
        /// `0x0400_0000`: Sign modifier on relative date comparisons.
        const SIGN_MODIFIER = 0x0400_0000;
    }
}

/// Comparison operator of a [`SmartCondition`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartOperator {
    /// Exact match (`is`).
    Is,
    /// Negated exact match (`is not`).
    IsNot,
    /// Substring match (`contains`).
    Contains,
    /// Negated substring match (`does not contain`).
    DoesNotContain,
    /// Prefix match (`starts with`).
    StartsWith,
    /// Suffix match (`ends with`).
    EndsWith,
    /// Strictly greater than (`is greater than`).
    GreaterThan,
    /// Strictly less than (`is less than`).
    LessThan,
    /// Inclusive range (`is in the range`).
    InRange,
    /// Negated inclusive range (`is not in the range`).
    NotInRange,
    /// Relative rolling time window (`is in the last`).
    InTheLast,
    /// Negated relative rolling time window (`is not in the last`).
    NotInTheLast,
    /// Unrecognized raw `u32` operator bitmask.
    Unknown(u32),
}

impl SmartOperator {
    /// Returns the [`SmartOperatorFlags`] bitmask representation of a raw `SLst` operator value.
    #[must_use]
    pub const fn flags(raw: u32) -> SmartOperatorFlags {
        SmartOperatorFlags::from_bits_retain(raw)
    }
}

impl From<u32> for SmartOperator {
    fn from(cmp: u32) -> Self {
        let flags = SmartOperatorFlags::from_bits_retain(cmp);
        let negated = flags.contains(SmartOperatorFlags::NEGATE);
        let base = flags
            & !(SmartOperatorFlags::STRING_MODE
                | SmartOperatorFlags::NEGATE
                | SmartOperatorFlags::SIGN_MODIFIER);

        match (base, negated) {
            (SmartOperatorFlags::IS | SmartOperatorFlags::BITWISE_SET, false) => Self::Is,
            (SmartOperatorFlags::IS | SmartOperatorFlags::BITWISE_SET, true) => Self::IsNot,
            (SmartOperatorFlags::CONTAINS, false) => Self::Contains,
            (SmartOperatorFlags::CONTAINS, true) => Self::DoesNotContain,
            (SmartOperatorFlags::STARTS_WITH, false) => Self::StartsWith,
            (SmartOperatorFlags::ENDS_WITH, false) => Self::EndsWith,
            (SmartOperatorFlags::GREATER_THAN, false) => Self::GreaterThan,
            (SmartOperatorFlags::LESS_THAN, false) => Self::LessThan,
            (SmartOperatorFlags::IN_RANGE, false) => Self::InRange,
            (SmartOperatorFlags::IN_RANGE, true) => Self::NotInRange,
            (SmartOperatorFlags::IN_THE_LAST, false) => Self::InTheLast,
            (SmartOperatorFlags::IN_THE_LAST, true) => Self::NotInTheLast,
            _ => Self::Unknown(cmp),
        }
    }
}

impl fmt::Display for SmartOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Is => f.write_str("Is"),
            Self::IsNot => f.write_str("IsNot"),
            Self::Contains => f.write_str("Contains"),
            Self::DoesNotContain => f.write_str("DoesNotContain"),
            Self::StartsWith => f.write_str("StartsWith"),
            Self::EndsWith => f.write_str("EndsWith"),
            Self::GreaterThan => f.write_str("GreaterThan"),
            Self::LessThan => f.write_str("LessThan"),
            Self::InRange => f.write_str("InRange"),
            Self::NotInRange => f.write_str("NotInRange"),
            Self::InTheLast => f.write_str("InTheLast"),
            Self::NotInTheLast => f.write_str("NotInTheLast"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Time unit for `InTheLast` / `NotInTheLast` Smart Playlist rules.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RelativeTimeUnit {
    /// Days (`86_400` seconds).
    Days,
    /// Weeks (`604_800` seconds).
    Weeks,
    /// Months (`2_628_000` seconds).
    Months,
    /// Unrecognized raw unit value.
    Unknown(i64),
}

impl fmt::Display for RelativeTimeUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Days => f.write_str("Days"),
            Self::Weeks => f.write_str("Weeks"),
            Self::Months => f.write_str("Months"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Typed comparison argument value of a [`SmartCondition`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "value_type"))]
pub enum SmartRuleValue {
    /// UTF-16BE string comparison operand.
    Text {
        /// Decoded text string.
        text: String,
    },
    /// Boolean comparison operand (`Checked`, `Compilation`, `Purchased`, `AlbumArtwork`, `HasVideo`).
    Boolean {
        /// Decoded boolean value.
        value: bool,
    },
    /// Single 64-bit signed integer operand.
    Integer {
        /// Integer value.
        value: i64,
    },
    /// Inclusive integer range (`min..=max`).
    IntegerRange {
        /// Lower bound (inclusive).
        min: i64,
        /// Upper bound (inclusive).
        max: i64,
    },
    /// Single star rating operand (`0..=5` stars, stored as `0..=100` in steps of 20).
    StarRating {
        /// Number of stars (`0..=5`).
        stars: u8,
        /// Raw `0..=100` rating value.
        raw_0_100: i64,
    },
    /// Star rating range (`min_stars..=max_stars`).
    StarRatingRange {
        /// Minimum stars (`0..=5`).
        min_stars: u8,
        /// Maximum stars (`0..=5`).
        max_stars: u8,
        /// Minimum raw `0..=100` value.
        min_raw: i64,
        /// Maximum raw `0..=100` value.
        max_raw: i64,
    },
    /// Relative rolling time window (`in the last N days/weeks/months`).
    RelativeTime {
        /// Number of units.
        amount: u64,
        /// Time unit (`Days`, `Weeks`, `Months`).
        unit: RelativeTimeUnit,
        /// Raw seconds per unit from `arg_2`.
        seconds_per_unit: i64,
    },
    /// Absolute timestamp comparison operand.
    Date {
        /// Raw Apple HFS+ timestamp (seconds since 1904-01-01).
        apple_timestamp: i64,
        /// Converted UTC timestamp if valid.
        utc: Option<Timestamp>,
    },
    /// Inclusive date range comparison operand.
    DateRange {
        /// Start Apple HFS+ timestamp.
        start_apple: i64,
        /// End Apple HFS+ timestamp.
        end_apple: i64,
        /// Converted start UTC timestamp.
        start_utc: Option<Timestamp>,
        /// Converted end UTC timestamp.
        end_utc: Option<Timestamp>,
    },
    /// Reference to another playlist by [`PersistentId`].
    PlaylistRef {
        /// Target playlist Persistent ID.
        playlist_persistent_id: PersistentId,
        /// Resolved name of the target playlist if present in the library.
        playlist_name: Option<String>,
    },
    /// [`CloudStatus`] comparison operand.
    CloudStatus {
        /// Target cloud status.
        status: CloudStatus,
    },
    /// [`SuggestionFlag`] comparison operand (`Favorite`, `AlbumFavorite`).
    SuggestionFlag {
        /// Target suggestion flag.
        flag: SuggestionFlag,
    },
    /// [`MediaKind`] comparison operand.
    MediaKind {
        /// Target media kind.
        kind: MediaKind,
    },
    /// Track storage location comparison operand (`Computer`, `iCloud`, `On This Computer`).
    LocationKind {
        /// Raw location code.
        code: i64,
        /// Human-readable location name.
        name: String,
    },
    /// Unparsed raw rule child bytes encoded as hex.
    Raw {
        /// Lowercase hexadecimal string of the raw payload bytes.
        hex: String,
    },
}

/// A single decoded Smart Playlist condition inside a [`SmartRuleGroup`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartCondition {
    /// Raw `SLst` field ID.
    pub field_id: u32,
    /// Decoded target [`SmartField`].
    pub field: SmartField,
    /// Decoded [`SmartOperator`].
    pub operator: SmartOperator,
    /// Raw 32-bit comparison method bitflags.
    pub comparison_raw: u32,
    /// Decoded comparison operand value.
    pub value: SmartRuleValue,
    /// Human-readable summary of the condition (e.g. `"Rating is greater than 3 stars"`).
    pub description: String,
}

/// View & sorting preferences loaded from `.musiclibrary/preferences/Playlist_<ID>.plist` or `Songs.plist`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaylistViewPreferences {
    /// View mode (e.g. `"List"`, `"Songs"`, `"Albums"`, `"Playlist"`).
    pub view_as: Option<String>,
    /// Primary sort column name (e.g. `"CloudOrArtist"`, `"Album"`, `"Title"`).
    pub sort_field: Option<String>,
    /// Whether the primary sort order is ascending (`true`) or descending (`false`).
    pub sort_ascending: Option<bool>,
    /// Ordered list of visible column names in List/Songs view.
    pub visible_columns: Vec<String>,
}

/// High-level Domain Model of a parsed `.musicdb` library.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Library {
    /// Library-wide metadata from `hfma`, `plma`, and `Preferences.plist`.
    pub metadata: LibraryMetadata,
    /// All album records (`iama`) in the library.
    pub albums: Vec<Album>,
    /// All artist records (`iAma`) in the library.
    pub artists: Vec<Artist>,
    /// All track records (`itma`) in the library.
    pub tracks: Vec<Track>,
    /// All playlist records (`lpma`) in the library.
    pub playlists: Vec<Playlist>,
    /// All Apple Music social profile records (`isma`) in the library.
    pub social_profiles: Vec<SocialProfile>,
    /// Pending offline iCloud Music Library track deletions (`DCma` / `dCma`).
    pub pending_cloud_deletions: Vec<PendingCloudDeletion>,
    #[cfg_attr(feature = "serde", serde(skip))]
    track_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    album_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    artist_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    playlist_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    social_profile_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    tracks_by_album: HashMap<PersistentId, Vec<usize>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    tracks_by_artist: HashMap<PersistentId, Vec<usize>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    sidecar_warnings: Vec<String>,
}

impl PartialEq for Library {
    fn eq(&self, other: &Self) -> bool {
        self.metadata == other.metadata
            && self.albums == other.albums
            && self.artists == other.artists
            && self.tracks == other.tracks
            && self.playlists == other.playlists
            && self.social_profiles == other.social_profiles
            && self.pending_cloud_deletions == other.pending_cloud_deletions
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Library {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct RawDeserializedLibrary {
            #[serde(default)]
            metadata: LibraryMetadata,
            #[serde(default)]
            albums: Vec<Album>,
            #[serde(default)]
            artists: Vec<Artist>,
            #[serde(default)]
            tracks: Vec<Track>,
            #[serde(default)]
            playlists: Vec<Playlist>,
            #[serde(default)]
            social_profiles: Vec<SocialProfile>,
            #[serde(default)]
            pending_cloud_deletions: Vec<PendingCloudDeletion>,
        }

        let raw = RawDeserializedLibrary::deserialize(deserializer)?;
        let mut lib = Self {
            metadata: raw.metadata,
            albums: raw.albums,
            artists: raw.artists,
            tracks: raw.tracks,
            playlists: raw.playlists,
            social_profiles: raw.social_profiles,
            pending_cloud_deletions: raw.pending_cloud_deletions,
            ..Self::default()
        };
        lib.rebuild_indexes();
        Ok(lib)
    }
}

/// Apple Music social profile record (`isma`, `hsma` subtype 15).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocialProfile {
    /// Social Profile Persistent ID (referenced by [`Playlist::author_social_profile_persistent_id`]).
    pub persistent_id: PersistentId,
    /// Public Apple Music profile ID (`boma 0x0320`, e.g. `"p.7J6g4W8v0R"`).
    pub profile_id: Option<String>,
    /// Display name (`boma 0x0321`).
    pub name: Option<String>,
    /// User handle without `@` (`boma 0x0322`).
    pub handle: Option<String>,
    /// Avatar identifier (`boma 0x0323`).
    pub avatar_id: Option<String>,
    /// Avatar image URL (`boma 0x0324`).
    pub avatar_url: Option<String>,
    /// Artwork UUID in `artwork.sqlite` (`isma` offset 24..40).
    pub artwork_uuid: Option<String>,
}

/// Library-level metadata combined from `hfma`, `plma`, `Preferences.plist`, and `preferences/Songs.plist`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibraryMetadata {
    /// Primary 64-bit Library Persistent ID.
    pub persistent_id: PersistentId,
    /// Original iTunes Library Persistent ID if migrated from `.itl`.
    pub itunes_persistent_id: Option<PersistentId>,
    /// Revision hash updated on every save (shared with `Library Preferences.musicdb`).
    pub revision_hash: PersistentId,
    /// Secondary file size mirror in format >= 23.1.
    pub file_size_mirror: u32,
    /// Apple Music / iTunes application version string (e.g. `"1.6.0.151"`).
    pub app_version: String,
    /// Binary format major version (e.g. `20`, `21`, `23`).
    pub format_major_version: u16,
    /// Binary format minor version (e.g. `2`, `20`, `40`, `1`).
    pub format_minor_version: u16,
    /// Number of `hsma` major sections declared in `hfma` (`hfma` offset 56..60).
    pub hsma_count: u32,
    /// Declared track count in `hfma`.
    pub declared_track_count: u32,
    /// Declared album count in `hfma`.
    pub declared_album_count: u32,
    /// Declared artist count in `hfma`.
    pub declared_artist_count: u32,
    /// Declared playlist count in `hfma`.
    pub declared_playlist_count: u32,
    /// Local timezone offset from UTC in seconds (e.g. `-10800` for UTC-3).
    pub timezone_offset_seconds: i32,
    /// Signed-in Apple Store account `dsid` (`None` if signed out).
    pub apple_store_account_id: Option<u64>,
    /// Library modification timestamp from `hfma`.
    pub date_modified: Option<Timestamp>,
    /// Last iCloud Music Library sync timestamp from `plma` (`plma` offset 16..20).
    pub date_cloud_synced: Option<Timestamp>,
    /// Last local mutation / sync-check timestamp from `plma` (`plma` offset 20..24).
    pub date_last_modified: Option<Timestamp>,
    /// Monotonic iCloud Music Library database revision from `plma` (`plma` offset 66..68).
    pub cloud_database_revision: Option<u16>,
    /// Cloud purchase-history (`Jalisco`) revision / last-modified timestamp from `plma` (`plma` offset 112..120, stored as Unix epoch seconds).
    pub date_cloud_purchase_history_modified: Option<Timestamp>,
    /// Future iCloud refresh / lease expiration timestamp from `plma` (`plma` offset 120..124).
    pub date_cloud_lease_expires: Option<Timestamp>,
    /// Future local iCloud authorization / lease expiration timestamp from `plma` (`plma` offset 182..186, when `header_size >= 186`).
    pub date_local_lease_expires: Option<Timestamp>,
    /// "Settings -> General -> Show -> Songs list checkboxes" setting from `plma`.
    pub show_song_list_checkboxes: Option<bool>,
    /// "Settings -> Files -> Keep Music Media folder organized" setting from `plma`.
    pub keep_media_folder_organized: Option<bool>,
    /// "Settings -> Files -> Copy files to Music Media folder when adding to library" from `Preferences.plist`.
    pub copy_files_to_media_folder: Option<bool>,
    /// Media folder `file://` URI from `plma` `boma 0x01F8`.
    pub media_folder_uri: Option<String>,
    /// Media folder filesystem path from `plma` `boma 0x01FD` / `0x0200`.
    pub media_folder_path: Option<String>,
    /// Legacy `.itl` file path from `plma` `boma 0x01FC`.
    pub imported_itl_path: Option<String>,
    /// Global Songs view preferences loaded from `preferences/Songs.plist`.
    pub songs_view_preferences: Option<PlaylistViewPreferences>,
}

/// Decoded Album record (`iama`).
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Albums have 4 orthogonal boolean flags (is_compilation, playlist_only, star_rating_is_computed, suggestion_locally_modified)"
)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Album {
    /// Album Persistent ID (`iama` offset 16..24).
    pub persistent_id: PersistentId,
    /// Secondary / iTunes Album Persistent ID (`iama` offset 64..72).
    pub itunes_persistent_id: Option<PersistentId>,
    /// Persistent ID of the first track in the album (`iama` offset 32..40).
    pub first_track_persistent_id: Option<PersistentId>,
    /// Album title (`boma 0x012C`).
    pub title: Option<String>,
    /// Primary artist name (`boma 0x012D`).
    pub artist: Option<String>,
    /// Album artist name (`boma 0x012E`).
    pub album_artist: Option<String>,
    /// iCloud Music Library album ID (`boma 0x0133`).
    pub cloud_album_id: Option<String>,
    /// Album editorial notes (`boma 0x0134`).
    pub editorial_notes: Option<String>,
    /// Apple Music catalog genre (`boma 0x0136`). Note: user genre edits update [`Track::genre`], not this catalog field.
    pub catalog_genre: Option<String>,
    /// Animated motion artwork `.m3u8` URL from `boma 0x0135` binary plist.
    pub motion_artwork_url: Option<String>,
    /// Motion artwork static preview `.png` URL from `boma 0x0135` binary plist.
    pub motion_preview_frame_url: Option<String>,
    /// Motion artwork token from `boma 0x0135` binary plist.
    pub motion_artwork_token: Option<String>,
    /// Compilation album flag (`iama` offset 25).
    pub is_compilation: bool,
    /// Whether this album is referenced only by playlist-only tracks (`iama` offset 26).
    pub playlist_only: bool,
    /// Apple Music catalog album release / edition type (`iama` offset 136..140).
    pub catalog_album_type: CatalogAlbumType,
    /// Raw star rating (`0..=100` in steps of 20).
    pub star_rating: u8,
    /// Whether `star_rating` is computed from track ratings (`0x20`) rather than set directly (`0x01`).
    pub star_rating_is_computed: bool,
    /// User suggestion / favorite flag (`iama` offset 42..44).
    pub suggestion_flag: SuggestionFlag,
    /// Whether `suggestion_flag` has the `0x0100` locally-modified bit set.
    pub suggestion_locally_modified: bool,
    /// Timestamp when `suggestion_flag` was last modified (`iama` offset 96..100).
    pub date_suggestion_flag_modified: Option<Timestamp>,
    /// Most recent last-played timestamp across tracks on this album (`iama` offset 100..104).
    pub date_last_played: Option<Timestamp>,
    /// iCloud sync timestamp (`iama` offset 120..124).
    pub date_cloud_synced: Option<Timestamp>,
    /// Album release date (`iama` offset 124..128).
    pub date_released: Option<Timestamp>,
    /// 64-bit Apple Music Store Album ID (`plID`, `iama` offset 104..112).
    pub apple_music_store_album_id: Option<u64>,
}

impl Album {
    /// Returns the star rating on a `0..=5` star scale (`star_rating / 20`).
    #[must_use]
    pub const fn stars(&self) -> u8 {
        self.star_rating / 20
    }
}

/// Decoded Artist record (`iAma`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Artist {
    /// Artist Persistent ID (`iAma` offset 16..24).
    pub persistent_id: PersistentId,
    /// Secondary / iTunes Artist Persistent ID (`iAma` offset 80..88).
    pub itunes_persistent_id: Option<PersistentId>,
    /// Artist name (`boma 0x0190`).
    pub name: Option<String>,
    /// Sort artist name (`boma 0x0191`).
    pub sort_name: Option<String>,
    /// iCloud Music Library artist ID (`boma 0x0193`).
    pub cloud_artist_id: Option<String>,
    /// Whether this artist is referenced only by playlist-only tracks (`iAma` offset 25).
    pub playlist_only: bool,
    /// User suggestion / favorite flag (`iAma` offset 101..103).
    pub suggestion_flag: SuggestionFlag,
    /// Whether `suggestion_flag` has the `0x0100` locally-modified bit set.
    pub suggestion_locally_modified: bool,
    /// Timestamp when `suggestion_flag` was last modified (`iAma` offset 60..64).
    pub date_suggestion_flag_modified: Option<Timestamp>,
    /// iCloud sync timestamp (`iAma` offset 120..124).
    pub date_cloud_synced: Option<Timestamp>,
    /// 64-bit Apple Music Store Artist ID (`atID`, `iAma` offset 52..60).
    pub apple_music_store_artist_id: Option<u64>,
    /// Artwork UUID in `artwork.sqlite` (`iAma` offset 64..80).
    pub artwork_uuid: Option<String>,
    /// Decoded artist artwork URL from `boma 0x0192` XML or binary plist (`artwork-url` key).
    pub artwork_url: Option<String>,
}

/// Decoded Track record (`itma` + `boma` children).
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Apple Music tracks store 12 independent boolean metadata checkboxes in itma"
)]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Track {
    /// Track Persistent ID (`itma` offset 16..24).
    pub persistent_id: PersistentId,
    /// Track title (`boma 0x0002`).
    pub title: Option<String>,
    /// Album title (`boma 0x0003`).
    pub album: Option<String>,
    /// Track artist name (`boma 0x0004`).
    pub artist: Option<String>,
    /// Album artist name (`boma 0x001B`).
    pub album_artist: Option<String>,
    /// Composer name (`boma 0x000C`).
    pub composer: Option<String>,
    /// Genre name (`boma 0x0005`).
    pub genre: Option<String>,
    /// Apple Store Genre ID (`geID`, `TrackNumerics` offset 264..268).
    pub store_genre_id: Option<u32>,
    /// Localized file kind description (`boma 0x0006`).
    pub kind: Option<String>,
    /// User comments (`boma 0x0008`).
    pub comments: Option<String>,
    /// Grouping tag (`boma 0x000E`).
    pub grouping: Option<String>,
    /// Classical work name (`boma 0x003F`).
    pub work_name: Option<String>,
    /// Classical movement name (`boma 0x0040`).
    pub movement_name: Option<String>,
    /// Classical movement number (`itma` offset 88..90).
    pub movement_number: u16,
    /// Classical total movements (`itma` offset 86..88).
    pub movement_total: u16,
    /// Sort title (`boma 0x001E`).
    pub sort_title: Option<String>,
    /// Sort album (`boma 0x001F`).
    pub sort_album: Option<String>,
    /// Sort artist (`boma 0x0020`).
    pub sort_artist: Option<String>,
    /// Sort album artist (`boma 0x0021`).
    pub sort_album_artist: Option<String>,
    /// Sort composer (`boma 0x0022`).
    pub sort_composer: Option<String>,
    /// Sort show name (`boma 0x0053`).
    pub sort_show: Option<String>,
    /// Sort string generation modes (`itma` offset 66..72).
    pub sort_modes: TrackSortModes,
    /// Track number on disc (`itma` offset 160..162).
    pub track_number: u16,
    /// Total tracks on disc (`itma` offset 116..118).
    pub track_total: u16,
    /// Disc number (`itma` offset 84..86).
    pub disc_number: u16,
    /// Total discs (`itma` offset 90..92).
    pub disc_total: u16,
    /// Release year (`itma` offset 168..172).
    pub year: u32,
    /// Beats per minute (`itma` offset 82..84).
    pub bpm: u16,
    /// Duration in milliseconds (`TrackNumerics` offset 156..160).
    pub duration_ms: Option<u32>,
    /// File size in bytes (`TrackNumerics` offset 296..300).
    pub file_size: Option<u32>,
    /// Audio bit rate in kbps (`TrackNumerics` offset 88..92).
    pub bit_rate: Option<u32>,
    /// Whether the audio file uses Variable Bit Rate (`TrackNumerics` offset 27).
    pub is_vbr: Option<bool>,
    /// Audio sample rate in Hz (`TrackNumerics` offset 60..64).
    pub sample_rate: Option<f32>,
    /// 4-character file type code (e.g. `"M4A "`, `"MP3 "`).
    pub file_type_fourcc: Option<String>,
    /// 4-character audio codec code (e.g. `"mp4a"`, `"mpeg"`).
    pub codec_fourcc: Option<String>,
    /// Decoded video stream metadata (`boma 0x0024`).
    pub video_info: Option<TrackVideoInfo>,
    /// Volume adjustment in `-255..=255` (`itma` offset 92..96).
    pub volume_adjustment: i32,
    /// Sound Check volume normalization (`iTunNORM`, `TrackNumerics` offset 132..136).
    pub normalization: Option<u32>,
    /// Gapless playback encoder delay in samples (`TrackNumerics` offset 108..112).
    pub gapless_encoder_delay: Option<u32>,
    /// Gapless playback encoder padding in samples (`TrackNumerics` offset 112..116).
    pub gapless_encoder_padding: Option<u32>,
    /// Gapless playback total sample count (`TrackNumerics` offset 256..264).
    pub gapless_total_samples: Option<u64>,
    /// Gapless playback byte offset to the 8th-from-last MPEG frame (`TrackNumerics` offset 272..280).
    pub gapless_last_eight_frames_offset: Option<u64>,
    /// Decoded equalizer preset (`boma 0x0007`).
    pub equalizer_preset: Option<EqualizerPreset>,
    /// Raw star rating (`0..=100` in steps of 20, `itma` offset 65).
    pub star_rating: u8,
    /// User suggestion / favorite flag (`itma` offset 62..64).
    pub suggestion_flag: SuggestionFlag,
    /// Whether `suggestion_flag` has the `0x0100` locally-modified bit set.
    pub suggestion_locally_modified: bool,
    /// Timestamp when `suggestion_flag` was last modified (`itma` offset 336..340).
    pub date_suggestion_flag_modified: Option<Timestamp>,
    /// Whether the track is unchecked/disabled (`itma` offset 42).
    pub disabled: bool,
    /// "Skip when shuffling" checkbox (`itma` offset 30).
    pub skip_when_shuffling: bool,
    /// Whether the track has ever been played (`itma` offset 31; persists across play-count resets).
    pub has_ever_been_played: bool,
    /// "Remember playback position" checkbox (`itma` offset 50).
    pub remember_playback_position: bool,
    /// "Show composer in all views" checkbox (`itma` offset 51).
    pub show_composer_in_all_views: bool,
    /// "Use work and movement" checkbox (`itma` offset 52).
    pub use_work_and_movement: bool,
    /// "Album is a compilation of songs by various artists" checkbox (`itma` offset 38).
    pub album_is_compilation: bool,
    /// "Part of a gapless album" checkbox (`itma` offset 49).
    pub part_of_gapless_album: bool,
    /// Whether the track exists only inside a subscribed/shared playlist and not in the main library (`itma` offset 39).
    pub playlist_only: bool,
    /// Whether the track was purchased from the iTunes Store.
    pub purchased: bool,
    /// Whether custom lyrics exist for this track (`itma` offset 255).
    pub has_custom_lyrics: bool,
    /// Whether custom lyrics were set locally (`itma` offset 44).
    pub has_local_custom_lyrics: bool,
    /// Whether local metadata edits are pending iCloud sync (`itma` offset 46).
    pub has_unsynced_local_changes: bool,
    /// Checksum/hash of custom lyrics (`TrackNumerics` offset 124..128).
    pub custom_lyrics_hash: Option<u32>,
    /// Whether Apple Music catalog time-synced lyrics are available (`TrackNumerics` offset 59: `1` = `Some(true)`, `2` = `Some(false)`, `0` = `None`).
    pub has_catalog_synced_lyrics: Option<bool>,
    /// Whether the track is matched via iTunes Match (`TrackNumerics` offset 32..34).
    pub is_itunes_matched: Option<bool>,
    /// Whether the track is an Apple Music subscription catalog item (`TrackNumerics` offset 34).
    pub is_apple_music_catalog: Option<bool>,
    /// Whether the track is an Apple Digital Master (`TrackNumerics` offset 36).
    pub is_apple_digital_master: Option<bool>,
    /// Whether the track is currently unavailable in the Apple Music catalog (`TrackNumerics` offset 20).
    pub is_cloud_unavailable: Option<bool>,
    /// Whether the track is currently available in the Apple Music catalog (`TrackNumerics` offset 39).
    pub is_available_in_catalog: Option<bool>,
    /// Apple Music catalog `audioTraits` bitmask (`itma` offset 331).
    pub audio_traits: AudioTraits,
    /// Downloaded local asset `audioTraits` bitmask (`TrackNumerics` offset 336..340 on `subrecord_kind == 1`; empty when not downloaded or stereo-only).
    pub downloaded_audio_traits: AudioTraits,
    /// Purchaser Apple Store account ID (`dsid`, `TrackNumerics` offset 280..288).
    pub purchaser_account_id: Option<u64>,
    /// `FairPlay` DRM authorized Apple Store account ID (`dsid`) on downloaded tracks (`TrackNumerics` offset 248..256).
    pub drm_account_id: Option<u64>,
    /// iCloud Music Library `sagaId` (`itma` offset 236..244).
    pub cloud_saga_id: Option<u64>,
    /// 64-bit Apple Music / iTunes Store track `adamId` (`TrackNumerics` offset 304..312).
    pub store_track_adam_id: Option<u64>,
    /// 64-bit Apple Music Store Album ID (`plID`, `TrackNumerics` offset 160..168).
    pub apple_music_store_album_id: Option<u64>,
    /// 64-bit Apple Music Store Artist ID (`atID`, `TrackNumerics` offset 168..176).
    pub apple_music_store_artist_id: Option<u64>,
    /// 64-bit Apple Music Store Album Artist ID (`TrackNumerics` offset 208..216).
    pub apple_music_store_album_artist_id: Option<u64>,
    /// Apple iTunes Storefront / Country ID (`sfID`, `TrackNumerics` offset 312..316).
    pub storefront_id: Option<u32>,
    /// Apple ID email of the purchaser (`boma 0x003B`).
    pub purchaser_email: Option<String>,
    /// Full name of the purchaser (`boma 0x003C`).
    pub purchaser_name: Option<String>,
    /// Local vs. cloud download state (`itma` offset 57).
    pub downloaded_state: DownloadedStatus,
    /// iCloud Music Library status (`itma` offset 58).
    pub cloud_status: CloudStatus,
    /// Cloud album identifier from `boma 0x0036` plist.
    pub cloud_album_id: Option<String>,
    /// Cloud artist identifier from `boma 0x0036` plist.
    pub cloud_artist_id: Option<String>,
    /// Cloud artwork token from `boma 0x0036` plist.
    pub cloud_artwork_token: Option<String>,
    /// Cloud lyrics token from `boma 0x0036` plist.
    pub cloud_lyrics_token: Option<String>,
    /// Content advisory rating (`itma` offset 59).
    pub content_advisory: ContentRating,
    /// Media kind (`itma` offset 108..112).
    pub media_type: MediaKind,
    /// Custom playback start offset in milliseconds (`itma` offset 148..152).
    pub playback_start_ms: u32,
    /// Custom playback stop offset in milliseconds (`itma` offset 152..156).
    pub playback_stop_ms: u32,
    /// User-visible play count (`TrackPlaysSkips` offset 12..16).
    pub play_count: Option<u32>,
    /// Cumulative play count unaffected by GUI resets (`TrackPlaysSkips` offset 16..20).
    pub true_play_count: Option<u32>,
    /// User-visible skip count (`TrackPlaysSkips` offset 32..36).
    pub skip_count: Option<u32>,
    /// Cumulative skip count (`TrackPlaysSkips` offset 36..40).
    pub true_skip_count: Option<u32>,
    /// Date added to library (`TrackNumerics` offset 92..96).
    pub date_added: Option<Timestamp>,
    /// Date modified (`TrackNumerics` offset 128..132).
    pub date_modified: Option<Timestamp>,
    /// Date last played (`TrackPlaysSkips` offset 8..12).
    pub date_last_played: Option<Timestamp>,
    /// Date first played in UTC (`TrackPlaysSkips` offset 20..24).
    pub date_first_played_utc: Option<Timestamp>,
    /// Date last skipped (`TrackPlaysSkips` offset 28..32).
    pub date_last_skipped: Option<Timestamp>,
    /// Release date (`TrackNumerics` offset 140..144; unwraps pre-1904 `u32` underflows when matching [`Track::year`]).
    pub date_released: Option<Timestamp>,
    /// Purchase date (`TrackNumerics` offset 136..140).
    pub date_purchased: Option<Timestamp>,
    /// Cloud sync timestamp (`TrackNumerics` offset 356..360, falling back to `itma` offset 352..356).
    pub date_cloud_synced: Option<Timestamp>,
    /// Local relative or `file://` URL (`boma 0x000B`).
    pub file_url: Option<String>,
    /// Legacy colon/slash file path (`boma 0x0043`).
    pub legacy_file_path: Option<String>,
    /// Extracted path from macOS `book` bookmark (`boma 0x0042`).
    pub book_path: Option<String>,
    /// Foreign key to [`Album::persistent_id`] (`itma` offset 172..180).
    pub album_persistent_id: Option<PersistentId>,
    /// Foreign key to [`Artist::persistent_id`] (`itma` offset 180..188).
    pub artist_persistent_id: Option<PersistentId>,
    /// Artwork UUID in `artwork.sqlite` (`itma` offset 256..272).
    pub artwork_uuid: Option<String>,
    /// Number of attached artworks (`TrackNumerics` offset 76..78).
    pub artwork_count: Option<u16>,
    /// Embedded / scanned artwork state (`TrackNumerics` offset 80..82).
    pub embedded_artwork: Option<EmbeddedArtworkKind>,
    /// Total byte size of attached artworks (`TrackNumerics` offset 84..88).
    pub artwork_total_size_bytes: Option<u32>,
    /// File folder count (`TrackNumerics` offset 72..74).
    pub file_folder_count: Option<u16>,
    /// Library folder count (`TrackNumerics` offset 74..76).
    pub library_folder_count: Option<u16>,
    /// International Standard Recording Code (`boma 0x002B`).
    pub isrc: Option<String>,
    /// Copyright notice (`boma 0x002E`).
    pub copyright: Option<String>,
    /// Additional string `boma` subtypes preserved by key `"0xXXXX_name"`.
    pub extra_strings: BTreeMap<String, String>,
}

impl Track {
    /// Returns the star rating on a `0..=5` star scale (`star_rating / 20`).
    #[must_use]
    pub const fn stars(&self) -> u8 {
        self.star_rating / 20
    }

    /// Returns `true` if the local audio file has embedded artwork (`embedded_artwork == Some(EmbeddedArtworkKind::Embedded)`).
    #[must_use]
    pub const fn has_embedded_artwork(&self) -> bool {
        matches!(self.embedded_artwork, Some(EmbeddedArtworkKind::Embedded))
    }
}

/// Decoded Playlist record (`lpma` + `boma` children + sidecar `Playlist_<ID>.plist`).
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Playlists have 6 orthogonal boolean flags (is_master, is_folder, is_smart, is_subscribed, show_in_profile_and_search, suggestion_locally_modified)"
)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Playlist {
    /// Playlist Persistent ID (`lpma` offset 30..38).
    pub persistent_id: PersistentId,
    /// Parent Folder Playlist Persistent ID (`lpma` offset 50..58).
    pub parent_persistent_id: Option<PersistentId>,
    /// Playlist title (`boma 0x00C8`).
    pub name: Option<String>,
    /// Playlist description from `boma 0x00CD` plist.
    pub description: Option<String>,
    /// Semantic playlist kind (`Master`, `Distinguished`, `Folder`, `Smart`, `SubscribedCatalog`, `Standard`).
    pub kind: PlaylistKind,
    /// Whether this is the internal Master playlist (`####!####`, `lpma` offset 44).
    pub is_master: bool,
    /// Whether this playlist is a Folder (`lpma` offset 49).
    pub is_folder: bool,
    /// Whether this playlist is a user/system Smart Playlist (`boma 0x00C9` / `0x00CA`).
    pub is_smart: bool,
    /// Whether this playlist is a subscribed external Apple Music catalog playlist (`lpma` offset 260).
    pub is_subscribed: bool,
    /// "Show in My Profile and in Search" checkbox (`lpma` offset 262).
    pub show_in_profile_and_search: bool,
    /// Distinguished system playlist kind (`lpma` offset 79..81).
    pub system_kind: DistinguishedPlaylistKind,
    /// User suggestion / favorite flag (`lpma` offset 223..225).
    pub suggestion_flag: SuggestionFlag,
    /// Whether `suggestion_flag` has the `0x0100` locally-modified bit set.
    pub suggestion_locally_modified: bool,
    /// Suggestion flag modification timestamp (`lpma` offset 324..328, when `header_size >= 328`).
    pub date_suggestion_flag_modified: Option<Timestamp>,
    /// Creation timestamp (`lpma` offset 22..26).
    pub date_created: Option<Timestamp>,
    /// Modification timestamp (`lpma` offset 138..142).
    pub date_modified: Option<Timestamp>,
    /// Last played timestamp (`lpma` offset 182..186).
    pub date_last_played: Option<Timestamp>,
    /// Cloud publish / artwork generation timestamp (`lpma` offset 304..308).
    pub date_cloud_published: Option<Timestamp>,
    /// Cloud sync timestamp (`lpma` offset 356..360, when `header_size >= 368`).
    pub date_cloud_synced: Option<Timestamp>,
    /// User-visible play count (`lpma` offset 174..178).
    pub play_count: u32,
    /// Cumulative play count (`lpma` offset 178..182).
    pub true_play_count: u32,
    /// Cloud refresh interval in seconds (`lpma` offset 256..260).
    pub refresh_interval_seconds: u32,
    /// Cloud share / visibility mode (`lpma` offset 308..312).
    pub share_mode: PlaylistShareMode,
    /// Artwork cache state bitfield (`lpma` offset 300..304).
    pub artwork_cache_state: u32,
    /// iCloud Music Library playlist `sagaId` (`lpma` offset 70..78).
    pub cloud_saga_id: Option<u64>,
    /// Universal library playlist ID (`pl.u-...`) from `boma 0x00CD`.
    pub universal_library_id: Option<String>,
    /// Cloud playlist UUID from `boma 0x00CD`.
    pub cloud_uuid: Option<String>,
    /// Subscribed container URL from `boma 0x00CD`.
    pub subscribed_container_url: Option<String>,
    /// Curator / owner display name from `boma 0x00CD`.
    pub owner_name: Option<String>,
    /// Curator / owner identifier from `boma 0x00CD`.
    pub owner_id: Option<String>,
    /// Foreign key to [`SocialProfile::persistent_id`] (`lpma` offset 340..348).
    pub author_social_profile_persistent_id: Option<PersistentId>,
    /// Cloud artwork URL from `boma 0x00CD`.
    pub cloud_artwork_url: Option<String>,
    /// Cloud artwork token from `boma 0x00CD`.
    pub cloud_artwork_token: Option<String>,
    /// Artwork UUID in `artwork.sqlite` (`lpma` offset 263..279).
    pub artwork_uuid: Option<String>,
    /// Declared track count in `lpma` header (`lpma` offset 16..20).
    pub declared_track_count: u32,
    /// Ordered list of track [`PersistentId`]s (`ipfa` / `IPfa` items).
    pub track_persistent_ids: Vec<PersistentId>,
    /// Direct child playlist [`PersistentId`]s when this playlist is a folder.
    pub child_playlist_persistent_ids: Vec<PersistentId>,
    /// Decoded Smart Playlist configuration (`boma 0x00CA` + `0x00C9` `SLst`).
    pub smart_playlist: Option<SmartPlaylistConfig>,
    /// View and sorting preferences loaded from `preferences/Playlist_<ID>.plist`.
    pub view_preferences: Option<PlaylistViewPreferences>,
}

/// Discovers the default Apple Music `.musiclibrary` bundle path in the user's Music directory if present.
#[must_use]
pub fn discover_default_library_path() -> Option<PathBuf> {
    let music_dir = default_user_music_dir()?;

    let candidates = [
        music_dir
            .join("Apple Music")
            .join("Apple Music Library.musiclibrary"),
        music_dir.join("Music").join("Music Library.musiclibrary"),
        music_dir.join("Apple Music Library.musiclibrary"),
        music_dir.join("Music Library.musiclibrary"),
    ];

    candidates.into_iter().find(|p| p.exists())
}

fn default_user_music_dir() -> Option<PathBuf> {
    #[cfg(feature = "cli")]
    if let Some(user_dirs) = directories::UserDirs::new() {
        return Some(
            user_dirs
                .audio_dir()
                .map_or_else(|| user_dirs.home_dir().join("Music"), Path::to_path_buf),
        );
    }
    std::env::home_dir().map(|home| home.join("Music"))
}

impl Library {
    /// Opens the user's default Apple Music `.musiclibrary` bundle on the current system.
    ///
    /// # Errors
    ///
    /// Returns [`MusicDbError::LibraryNotFound`] if no default library is found, or [`MusicDbError`] if parsing fails.
    pub fn open_default() -> Result<Self> {
        let path = discover_default_library_path().ok_or(MusicDbError::LibraryNotFound)?;
        Self::open(path)
    }

    /// Opens a `.musicdb` file, `.musiclibrary` bundle directory, or parent folder containing a `.musiclibrary` bundle
    /// and converts it to a high-level [`Library`].
    ///
    /// # Errors
    ///
    /// Returns [`MusicDbError`] if reading, decrypting, or parsing the `.musicdb` file fails.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mdb_path = RawLibraryFile::resolve_musicdb_path(path);
        let raw = RawLibraryFile::open(&mdb_path)?;
        let mut lib = Self::from(raw);

        let bundle_dir = mdb_path.parent().filter(|parent| {
            parent.extension().and_then(|e| e.to_str()) == Some("musiclibrary")
                || parent.join("Preferences.plist").exists()
                || parent.join("preferences").is_dir()
                || parent.join("Preferences").is_dir()
        });

        if let Some(dir) = bundle_dir {
            lib.load_sidecar_preferences(dir);
        }

        Ok(lib)
    }

    /// Rebuilds the internal $O(1)$ [`PersistentId`] and track-relation lookup indexes.
    pub fn rebuild_indexes(&mut self) {
        self.track_index = self
            .tracks
            .iter()
            .enumerate()
            .map(|(idx, t)| (t.persistent_id, idx))
            .collect();
        self.album_index = self
            .albums
            .iter()
            .enumerate()
            .map(|(idx, a)| (a.persistent_id, idx))
            .collect();
        self.artist_index = self
            .artists
            .iter()
            .enumerate()
            .map(|(idx, a)| (a.persistent_id, idx))
            .collect();
        self.playlist_index = self
            .playlists
            .iter()
            .enumerate()
            .map(|(idx, p)| (p.persistent_id, idx))
            .collect();
        self.social_profile_index = self
            .social_profiles
            .iter()
            .enumerate()
            .map(|(idx, sp)| (sp.persistent_id, idx))
            .collect();

        self.tracks_by_album.clear();
        self.tracks_by_album.reserve(self.albums.len());
        self.tracks_by_artist.clear();
        self.tracks_by_artist.reserve(self.artists.len());
        for (idx, t) in self.tracks.iter().enumerate() {
            if let Some(apid) = t.album_persistent_id {
                self.tracks_by_album.entry(apid).or_default().push(idx);
            }
            if let Some(rpid) = t.artist_persistent_id {
                self.tracks_by_artist.entry(rpid).or_default().push(idx);
            }
        }
        for indices in self.tracks_by_album.values_mut() {
            indices.sort_by_key(|&idx| {
                self.tracks
                    .get(idx)
                    .map_or((u16::MAX, u16::MAX), |t| (t.disc_number, t.track_number))
            });
        }
    }

    /// Returns any non-fatal warnings encountered while reading optional sidecar `.plist` files.
    #[must_use]
    pub fn sidecar_warnings(&self) -> &[String] {
        &self.sidecar_warnings
    }

    /// Loads optional sidecar `.plist` files from a `.musiclibrary` bundle directory (`Preferences.plist` and `preferences/*.plist`).
    ///
    /// Non-fatal `.plist` parse errors are recorded in [`Library::sidecar_warnings`].
    pub fn load_sidecar_preferences<P: AsRef<Path>>(&mut self, bundle_dir: P) {
        let dir = bundle_dir.as_ref();
        self.sidecar_warnings.clear();

        let prefs_plist = dir.join("Preferences.plist");
        if prefs_plist.is_file() {
            match plist::Value::from_file(&prefs_plist) {
                Ok(plist::Value::Dictionary(root)) => {
                    if let Some(copy_flag) = root
                        .get("com.apple.Music")
                        .and_then(plist::Value::as_dictionary)
                        .and_then(|d| {
                            d.get(" CopyFilesToMediaFolder")
                                .or_else(|| d.get("CopyFilesToMediaFolder"))
                        })
                        .or_else(|| {
                            root.get("Copy files to Music Media folder when adding to library")
                        })
                        .and_then(plist::Value::as_boolean)
                    {
                        self.metadata.copy_files_to_media_folder = Some(copy_flag);
                    }
                }
                Ok(_) => self.sidecar_warnings.push(format!(
                    "sidecar {} root is not a dictionary",
                    prefs_plist.display()
                )),
                Err(err) => self.sidecar_warnings.push(format!(
                    "failed to parse sidecar {}: {err}",
                    prefs_plist.display()
                )),
            }
        }

        let prefs_subdir = if dir.join("preferences").is_dir() {
            dir.join("preferences")
        } else {
            dir.join("Preferences")
        };
        if !prefs_subdir.is_dir() {
            return;
        }

        let songs_plist = prefs_subdir.join("Songs.plist");
        if songs_plist.is_file() {
            self.metadata.songs_view_preferences =
                load_optional_view_plist(&songs_plist, &mut self.sidecar_warnings);
        }

        for pl in &mut self.playlists {
            let pid_str = pl.persistent_id.to_string();
            let candidate_upper = prefs_subdir.join(format!("Playlist_{pid_str}.plist"));
            let candidate_lower =
                prefs_subdir.join(format!("Playlist_{}.plist", pid_str.to_lowercase()));
            if candidate_upper.is_file() {
                pl.view_preferences =
                    load_optional_view_plist(&candidate_upper, &mut self.sidecar_warnings);
            } else if candidate_lower.is_file() {
                pl.view_preferences =
                    load_optional_view_plist(&candidate_lower, &mut self.sidecar_warnings);
            }
        }
    }

    /// Looks up a track by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn track_by_id(&self, id: impl IntoPersistentId) -> Option<&Track> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.track_index.get(&pid) {
            self.tracks.get(idx)
        } else if self.track_index.is_empty() {
            self.tracks.iter().find(|t| t.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up an album by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn album_by_id(&self, id: impl IntoPersistentId) -> Option<&Album> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.album_index.get(&pid) {
            self.albums.get(idx)
        } else if self.album_index.is_empty() {
            self.albums.iter().find(|a| a.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up an artist by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn artist_by_id(&self, id: impl IntoPersistentId) -> Option<&Artist> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.artist_index.get(&pid) {
            self.artists.get(idx)
        } else if self.artist_index.is_empty() {
            self.artists.iter().find(|a| a.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up a playlist by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn playlist_by_id(&self, id: impl IntoPersistentId) -> Option<&Playlist> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.playlist_index.get(&pid) {
            self.playlists.get(idx)
        } else if self.playlist_index.is_empty() {
            self.playlists.iter().find(|p| p.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up an Apple Music social profile by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn social_profile_by_id(&self, id: impl IntoPersistentId) -> Option<&SocialProfile> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.social_profile_index.get(&pid) {
            self.social_profiles.get(idx)
        } else if self.social_profile_index.is_empty() {
            self.social_profiles
                .iter()
                .find(|sp| sp.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up a playlist by exact name (with case-insensitive exact fallback).
    #[must_use]
    pub fn playlist_by_name(&self, name: &str) -> Option<&Playlist> {
        self.playlists
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .or_else(|| {
                let q = name.to_lowercase();
                self.playlists.iter().find(|p| {
                    p.name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(name) || n.to_lowercase() == q)
                })
            })
    }

    /// Returns an iterator over all resolved [`Track`] records in the given [`Playlist`] in playlist order.
    pub fn playlist_tracks<'a>(
        &'a self,
        playlist: &'a Playlist,
    ) -> impl Iterator<Item = &'a Track> + 'a {
        playlist
            .track_persistent_ids
            .iter()
            .filter_map(|id| self.track_by_id(*id))
    }

    /// Returns an iterator over the direct child [`Playlist`] records of a folder playlist.
    pub fn playlist_children<'a>(
        &'a self,
        folder: &'a Playlist,
    ) -> impl Iterator<Item = &'a Playlist> + 'a {
        folder
            .child_playlist_persistent_ids
            .iter()
            .filter_map(|id| self.playlist_by_id(*id))
    }

    /// Returns an iterator over all [`Track`] records belonging to an [`Album`] by [`PersistentId`], sorted by disc and track number.
    pub fn album_tracks(
        &self,
        album_id: impl IntoPersistentId,
    ) -> impl Iterator<Item = &Track> + '_ {
        let indices = album_id
            .into_persistent_id()
            .and_then(|pid| self.tracks_by_album.get(&pid))
            .map_or(&[][..], Vec::as_slice);
        indices.iter().filter_map(|&idx| self.tracks.get(idx))
    }

    /// Returns an iterator over all [`Track`] records belonging to an [`Artist`] by [`PersistentId`].
    pub fn artist_tracks(
        &self,
        artist_id: impl IntoPersistentId,
    ) -> impl Iterator<Item = &Track> + '_ {
        let indices = artist_id
            .into_persistent_id()
            .and_then(|pid| self.tracks_by_artist.get(&pid))
            .map_or(&[][..], Vec::as_slice);
        indices.iter().filter_map(|&idx| self.tracks.get(idx))
    }

    /// Serializes the library to a pretty-printed JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`MusicDbError::Json`] if JSON serialization fails.
    #[cfg(any(feature = "sqlite", feature = "cli"))]
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

fn library_metadata_from_header(h: &HfmaHeader) -> LibraryMetadata {
    LibraryMetadata {
        persistent_id: PersistentId::new(h.library_persistent_id),
        itunes_persistent_id: (h.itunes_library_persistent_id != 0)
            .then_some(PersistentId::new(h.itunes_library_persistent_id)),
        revision_hash: PersistentId::new(h.revision_hash),
        file_size_mirror: h.file_size_mirror,
        app_version: h.version_string(),
        format_major_version: h.major_version,
        format_minor_version: h.minor_version,
        hsma_count: h.hsma_count,
        declared_track_count: h.track_count,
        declared_album_count: h.album_count,
        declared_artist_count: h.artist_count,
        declared_playlist_count: h.playlist_count,
        timezone_offset_seconds: h.timezone_offset_seconds,
        apple_store_account_id: (h.apple_store_account_id != 0).then_some(h.apple_store_account_id),
        date_modified: h.date_modified.to_utc(),
        ..LibraryMetadata::default()
    }
}

fn finalize_library(
    metadata: LibraryMetadata,
    albums: Vec<Album>,
    artists: Vec<Artist>,
    tracks: Vec<Track>,
    mut playlists: Vec<Playlist>,
    social_profiles: Vec<SocialProfile>,
    pending_cloud_deletions: Vec<PendingCloudDeletion>,
) -> Library {
    link_playlist_hierarchy_and_refs(&mut playlists);

    let mut lib = Library {
        metadata,
        albums,
        artists,
        tracks,
        playlists,
        social_profiles,
        pending_cloud_deletions,
        ..Library::default()
    };
    lib.rebuild_indexes();
    lib
}

impl From<RawLibraryFile> for Library {
    fn from(raw: RawLibraryFile) -> Self {
        let tz = raw.outer_header.timezone_offset_seconds;
        let mut metadata = library_metadata_from_header(&raw.outer_header);

        let mut albums = Vec::new();
        let mut artists = Vec::new();
        let mut tracks = Vec::new();
        let mut playlists = Vec::new();
        let mut social_profiles = Vec::new();
        let mut pending_cloud_deletions = Vec::new();

        for hsma in raw.sections {
            match hsma.child {
                HsmaChild::LibraryMaster(plma) => {
                    populate_plma_metadata_owned(plma, &mut metadata);
                }
                HsmaChild::AlbumList(lama) => {
                    albums.extend(
                        lama.albums
                            .into_iter()
                            .map(|a| Album::from_owned_with_tz(a, tz)),
                    );
                }
                HsmaChild::ArtistList(l_ama) => {
                    artists.extend(l_ama.artists.into_iter().map(Artist::from_owned));
                }
                HsmaChild::TrackList(ltma) => {
                    tracks.extend(
                        ltma.tracks
                            .into_iter()
                            .map(|t| Track::from_owned_with_tz(t, tz)),
                    );
                }
                HsmaChild::PlaylistList(l_pma) => {
                    playlists.extend(l_pma.playlists.into_iter().map(Playlist::from_owned));
                }
                HsmaChild::PendingCloudDeletions(dcma) => {
                    pending_cloud_deletions
                        .extend(dcma.items.iter().map(PendingCloudDeletion::from));
                }
                HsmaChild::SocialProfileList(lsma) => {
                    social_profiles
                        .extend(lsma.profiles.into_iter().map(SocialProfile::from_owned));
                }
                HsmaChild::InnerHfma(_)
                | HsmaChild::Padding(_)
                | HsmaChild::Unknown { .. }
                | HsmaChild::Skipped { .. } => {}
            }
        }

        finalize_library(
            metadata,
            albums,
            artists,
            tracks,
            playlists,
            social_profiles,
            pending_cloud_deletions,
        )
    }
}

impl From<&RawLibraryFile> for Library {
    fn from(raw: &RawLibraryFile) -> Self {
        let tz = raw.outer_header.timezone_offset_seconds;
        let mut metadata = library_metadata_from_header(&raw.outer_header);

        let mut albums = Vec::new();
        let mut artists = Vec::new();
        let mut tracks = Vec::new();
        let mut playlists = Vec::new();
        let mut social_profiles = Vec::new();
        let mut pending_cloud_deletions = Vec::new();

        for hsma in &raw.sections {
            match &hsma.child {
                HsmaChild::LibraryMaster(plma) => {
                    populate_plma_metadata(plma, &mut metadata);
                }
                HsmaChild::AlbumList(lama) => {
                    albums.extend(lama.albums.iter().map(|a| Album::from_raw_with_tz(a, tz)));
                }
                HsmaChild::ArtistList(l_ama) => {
                    artists.extend(l_ama.artists.iter().map(Artist::from));
                }
                HsmaChild::TrackList(ltma) => {
                    tracks.extend(ltma.tracks.iter().map(|t| Track::from_raw_with_tz(t, tz)));
                }
                HsmaChild::PlaylistList(l_pma) => {
                    playlists.extend(l_pma.playlists.iter().map(Playlist::from));
                }
                HsmaChild::PendingCloudDeletions(dcma) => {
                    pending_cloud_deletions
                        .extend(dcma.items.iter().map(PendingCloudDeletion::from));
                }
                HsmaChild::SocialProfileList(lsma) => {
                    social_profiles.extend(lsma.profiles.iter().map(SocialProfile::from));
                }
                HsmaChild::InnerHfma(_)
                | HsmaChild::Padding(_)
                | HsmaChild::Unknown { .. }
                | HsmaChild::Skipped { .. } => {}
            }
        }

        finalize_library(
            metadata,
            albums,
            artists,
            tracks,
            playlists,
            social_profiles,
            pending_cloud_deletions,
        )
    }
}

fn unix_timestamp_to_utc(unix_secs: u64) -> Option<Timestamp> {
    if unix_secs == 0 {
        return None;
    }
    i64::try_from(unix_secs)
        .ok()
        .and_then(|secs| Timestamp::from_second(secs).ok())
}

fn apply_plma_header(header: &LibraryMasterHeader, meta: &mut LibraryMetadata) {
    meta.date_cloud_synced = header.date_cloud_synced.to_utc();
    meta.date_last_modified = header.date_last_modified.to_utc();
    meta.cloud_database_revision =
        (header.cloud_database_revision != 0).then_some(header.cloud_database_revision);
    meta.date_cloud_purchase_history_modified =
        unix_timestamp_to_utc(header.date_cloud_purchase_history_modified);
    meta.date_cloud_lease_expires = header.date_cloud_lease_expires.to_utc();
    meta.date_local_lease_expires = header
        .date_local_lease_expires
        .and_then(AppleTimestamp::to_utc);
    meta.show_song_list_checkboxes = Some(header.checkbox_show_song_list_checkboxes != 0);
    meta.keep_media_folder_organized = Some(header.checkbox_keep_media_folder_organized != 0);
}

fn apply_plma_string(subtype: u32, val: String, meta: &mut LibraryMetadata) {
    match subtype {
        0x01F8 => meta.media_folder_uri = Some(val),
        0x01FC => meta.imported_itl_path = Some(val),
        0x01FD | 0x0200 => meta.media_folder_path = Some(val),
        _ => {}
    }
}

fn populate_plma_metadata(plma: &LibraryMasterSection, meta: &mut LibraryMetadata) {
    apply_plma_header(&plma.header, meta);
    for boma in &plma.bomas {
        if let Some(s) = boma.as_str() {
            apply_plma_string(boma.subtype, s.to_owned(), meta);
        }
    }
}

fn populate_plma_metadata_owned(plma: LibraryMasterSection, meta: &mut LibraryMetadata) {
    apply_plma_header(&plma.header, meta);
    for boma in plma.bomas {
        let subtype = boma.subtype;
        if let Some(s) = boma.into_string() {
            apply_plma_string(subtype, s, meta);
        }
    }
}

fn link_playlist_hierarchy_and_refs(playlists: &mut [Playlist]) {
    let mut parent_to_children: HashMap<PersistentId, Vec<PersistentId>> = HashMap::new();
    let mut playlist_names: HashMap<PersistentId, String> = HashMap::new();
    for pl in playlists.iter() {
        if let Some(parent_id) = pl.parent_persistent_id {
            parent_to_children
                .entry(parent_id)
                .or_default()
                .push(pl.persistent_id);
        }
        if let Some(name) = &pl.name {
            playlist_names.insert(pl.persistent_id, name.clone());
        }
    }
    for pl in playlists.iter_mut() {
        if let Some(children) = parent_to_children.remove(&pl.persistent_id) {
            pl.child_playlist_persistent_ids = children;
        }
        if let Some(smart) = &mut pl.smart_playlist
            && let Some(group) = &mut smart.rule_group
        {
            resolve_playlist_refs_in_group(group, &playlist_names);
        }
    }
}

fn apply_social_boma_string(profile: &mut SocialProfile, subtype: u32, val: String) {
    match subtype {
        0x0320 => profile.profile_id = Some(val),
        0x0321 => profile.name = Some(val),
        0x0322 => profile.handle = Some(val),
        0x0323 => profile.avatar_id = Some(val),
        0x0324 => profile.avatar_url = Some(val),
        _ => {}
    }
}

impl SocialProfile {
    fn from_owned(isma: SocialProfileItem) -> Self {
        let mut profile = Self {
            persistent_id: PersistentId::new(isma.header.profile_persistent_id),
            artwork_uuid: format_artwork_uuid(&isma.header.artwork_uuid),
            ..Self::default()
        };
        for boma in isma.bomas {
            let subtype = boma.subtype;
            if let Some(s) = boma.into_string() {
                apply_social_boma_string(&mut profile, subtype, s);
            }
        }
        profile
    }
}

impl From<&SocialProfileItem> for SocialProfile {
    fn from(isma: &SocialProfileItem) -> Self {
        let mut profile = Self {
            persistent_id: PersistentId::new(isma.header.profile_persistent_id),
            artwork_uuid: format_artwork_uuid(&isma.header.artwork_uuid),
            ..Self::default()
        };
        for boma in &isma.bomas {
            if let Some(s) = boma.as_str() {
                apply_social_boma_string(&mut profile, boma.subtype, s.to_owned());
            }
        }
        profile
    }
}

/// Converts a standalone [`AlbumItem`] assuming `tz_offset_seconds = 0` (UTC) for local timestamps
/// such as `date_last_played`. Use [`Library::from`] to apply the library header's timezone offset.
impl From<&AlbumItem> for Album {
    fn from(iama: &AlbumItem) -> Self {
        Self::from_raw_with_tz(iama, 0)
    }
}

fn apply_album_string(album: &mut Album, subtype: u32, val: String) {
    match subtype {
        0x012C => album.title = Some(val),
        0x012D => album.artist = Some(val),
        0x012E => album.album_artist = Some(val),
        0x0133 => album.cloud_album_id = Some(val),
        0x0134 => album.editorial_notes = Some(val),
        0x0136 => album.catalog_genre = Some(val),
        _ => {}
    }
}

impl Album {
    fn from_header(h: &AlbumItemHeader, tz_offset_seconds: i32) -> Self {
        Self {
            persistent_id: PersistentId::new(h.album_persistent_id),
            itunes_persistent_id: (h.album_persistent_id_2 != 0)
                .then_some(PersistentId::new(h.album_persistent_id_2)),
            first_track_persistent_id: (h.first_track_persistent_id != 0)
                .then_some(PersistentId::new(h.first_track_persistent_id)),
            is_compilation: h.is_compilation != 0,
            playlist_only: h.playlist_only != 0,
            catalog_album_type: h
                .catalog_album_type
                .map_or(CatalogAlbumType::None, CatalogAlbumType::from),
            star_rating: h.star_rating,
            star_rating_is_computed: h.star_rating_inheritance == 0x20,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            suggestion_locally_modified: SuggestionFlag::is_locally_modified(h.suggestion_flag),
            date_suggestion_flag_modified: h.date_suggestion_flag_modified.to_utc(),
            date_last_played: h.date_last_played.to_utc_with_offset(tz_offset_seconds),
            date_cloud_synced: h.date_cloud_synced.and_then(AppleTimestamp::to_utc),
            date_released: h.date_released.and_then(AppleTimestamp::to_utc),
            apple_music_store_album_id: h.apple_music_store_album_id.filter(|&id| id != 0),
            ..Self::default()
        }
    }

    pub(crate) fn from_raw_with_tz(iama: &AlbumItem, tz_offset_seconds: i32) -> Self {
        let mut album = Self::from_header(&iama.header, tz_offset_seconds);
        for boma in &iama.bomas {
            if boma.subtype == 0x0135 {
                apply_album_motion_artwork_plist(
                    boma,
                    &mut album.motion_artwork_url,
                    &mut album.motion_preview_frame_url,
                    &mut album.motion_artwork_token,
                );
            } else if let Some(s) = boma.as_str() {
                apply_album_string(&mut album, boma.subtype, s.to_owned());
            }
        }
        album
    }

    fn from_owned_with_tz(iama: AlbumItem, tz_offset_seconds: i32) -> Self {
        let mut album = Self::from_header(&iama.header, tz_offset_seconds);
        for boma in iama.bomas {
            if boma.subtype == 0x0135 {
                apply_album_motion_artwork_plist(
                    &boma,
                    &mut album.motion_artwork_url,
                    &mut album.motion_preview_frame_url,
                    &mut album.motion_artwork_token,
                );
            } else {
                let subtype = boma.subtype;
                if let Some(s) = boma.into_string() {
                    apply_album_string(&mut album, subtype, s);
                }
            }
        }
        album
    }
}

#[cfg(feature = "serde")]
#[derive(Default)]
struct LossyPlistString(Option<String>);

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for LossyPlistString {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct LossyVisitor;

        impl<'de> serde::de::Visitor<'de> for LossyVisitor {
            type Value = LossyPlistString;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any plist value")
            }

            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(Some(v.to_owned())))
            }

            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(Some(v)))
            }

            fn visit_bool<E>(self, _: bool) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(None))
            }

            fn visit_i64<E>(self, _: i64) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(None))
            }

            fn visit_u64<E>(self, _: u64) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(None))
            }

            fn visit_f64<E>(self, _: f64) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(None))
            }

            fn visit_bytes<E>(self, _: &[u8]) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(None))
            }

            fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(None))
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(LossyPlistString(None))
            }

            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<Self::Value, D::Error> {
                let _ = <serde::de::IgnoredAny as serde::Deserialize>::deserialize(d)?;
                Ok(LossyPlistString(None))
            }

            fn visit_newtype_struct<D: serde::Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<Self::Value, D::Error> {
                let _ = <serde::de::IgnoredAny as serde::Deserialize>::deserialize(d)?;
                Ok(LossyPlistString(None))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(LossyPlistString(None))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while map
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {}
                Ok(LossyPlistString(None))
            }
        }

        deserializer.deserialize_any(LossyVisitor)
    }
}

#[cfg(feature = "serde")]
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct AlbumMotionArtworkPlist {
    #[serde(rename = "motion-artwork-url")]
    artwork_url: LossyPlistString,
    #[serde(rename = "motion-preview-frame-url")]
    preview_frame_url: LossyPlistString,
    #[serde(rename = "motion-artwork-token")]
    artwork_token: LossyPlistString,
}

fn apply_album_motion_artwork_plist(
    boma: &BomaSection,
    motion_artwork_url: &mut Option<String>,
    motion_preview_frame_url: &mut Option<String>,
    motion_artwork_token: &mut Option<String>,
) {
    let raw_bytes = match &boma.payload {
        BomaPayload::BinaryPlist { raw_bytes, .. }
        | BomaPayload::RawStringUtf8 { raw_bytes, .. }
        | BomaPayload::Unknown { raw_bytes, .. } => raw_bytes.as_slice(),
        _ => return,
    };

    #[cfg(feature = "serde")]
    {
        if let Ok(parsed) = plist::from_bytes::<AlbumMotionArtworkPlist>(raw_bytes) {
            if let Some(v) = parsed.artwork_url.0 {
                motion_artwork_url.get_or_insert(v);
            }
            if let Some(v) = parsed.preview_frame_url.0 {
                motion_preview_frame_url.get_or_insert(v);
            }
            if let Some(v) = parsed.artwork_token.0 {
                motion_artwork_token.get_or_insert(v);
            }
        }
    }

    #[cfg(not(feature = "serde"))]
    {
        let Ok(plist::Value::Dictionary(dict)) =
            plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
        else {
            return;
        };
        if let Some(v) = dict
            .get("motion-artwork-url")
            .and_then(plist::Value::as_string)
        {
            motion_artwork_url.get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("motion-preview-frame-url")
            .and_then(plist::Value::as_string)
        {
            motion_preview_frame_url.get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("motion-artwork-token")
            .and_then(plist::Value::as_string)
        {
            motion_artwork_token.get_or_insert_with(|| v.to_owned());
        }
    }
}

#[cfg(feature = "serde")]
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct ArtistArtworkPlist {
    #[serde(rename = "artwork-url")]
    artwork_url: LossyPlistString,
}

fn apply_artist_artwork_plist(artist: &mut Artist, raw_bytes: &[u8]) {
    #[cfg(feature = "serde")]
    if let Ok(parsed) = plist::from_bytes::<ArtistArtworkPlist>(raw_bytes)
        && let Some(v) = parsed.artwork_url.0
    {
        artist.artwork_url.get_or_insert(v);
    }

    #[cfg(not(feature = "serde"))]
    if let Ok(plist::Value::Dictionary(dict)) =
        plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
        && let Some(v) = dict.get("artwork-url").and_then(plist::Value::as_string)
    {
        artist.artwork_url.get_or_insert_with(|| v.to_owned());
    }
}

fn apply_artist_string(artist: &mut Artist, subtype: u32, raw_bytes: Option<&[u8]>, val: String) {
    match subtype {
        0x0190 | 0x0004 => artist.name = Some(val),
        0x0191 => artist.sort_name = Some(val),
        0x0192 => apply_artist_artwork_plist(artist, raw_bytes.unwrap_or(val.as_bytes())),
        0x0193 => artist.cloud_artist_id = Some(val),
        _ => {}
    }
}

impl Artist {
    fn from_header(h: &ArtistItemHeader) -> Self {
        let raw_store_artist_id = if h.apple_music_store_artist_id != 0 {
            h.apple_music_store_artist_id
        } else {
            h.apple_music_store_artist_id_2
        };
        Self {
            persistent_id: PersistentId::new(h.artist_persistent_id),
            itunes_persistent_id: (h.artist_persistent_id_2 != 0)
                .then_some(PersistentId::new(h.artist_persistent_id_2)),
            playlist_only: h.playlist_only != 0,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            suggestion_locally_modified: SuggestionFlag::is_locally_modified(h.suggestion_flag),
            date_suggestion_flag_modified: h.date_suggestion_flag_modified.to_utc(),
            date_cloud_synced: h.date_cloud_synced.and_then(AppleTimestamp::to_utc),
            apple_music_store_artist_id: (raw_store_artist_id != 0).then_some(raw_store_artist_id),
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            ..Self::default()
        }
    }

    fn from_owned(i_ama: ArtistItem) -> Self {
        let mut artist = Self::from_header(&i_ama.header);
        for boma in i_ama.bomas {
            let subtype = boma.subtype;
            match boma.payload {
                BomaPayload::BinaryPlist { raw_bytes, .. } if subtype == 0x0192 => {
                    apply_artist_artwork_plist(&mut artist, &raw_bytes);
                }
                BomaPayload::RawStringUtf8 { text, raw_bytes } => {
                    apply_artist_string(&mut artist, subtype, Some(&raw_bytes), text);
                }
                payload => {
                    let owned_boma = BomaSection { payload, ..boma };
                    if let Some(s) = owned_boma.into_string() {
                        apply_artist_string(&mut artist, subtype, None, s);
                    }
                }
            }
        }
        artist
    }
}

impl From<&ArtistItem> for Artist {
    fn from(i_ama: &ArtistItem) -> Self {
        let mut artist = Self::from_header(&i_ama.header);
        for boma in &i_ama.bomas {
            match &boma.payload {
                BomaPayload::BinaryPlist { raw_bytes, .. } if boma.subtype == 0x0192 => {
                    apply_artist_artwork_plist(&mut artist, raw_bytes);
                }
                BomaPayload::RawStringUtf8 { text, raw_bytes } => {
                    apply_artist_string(&mut artist, boma.subtype, Some(raw_bytes), text.clone());
                }
                _ => {
                    if let Some(s) = boma.as_str() {
                        apply_artist_string(&mut artist, boma.subtype, None, s.to_owned());
                    }
                }
            }
        }
        artist
    }
}

/// Converts a standalone [`TrackItem`] assuming `tz_offset_seconds = 0` (UTC) for local timestamps
/// such as `date_added`, `date_modified`, and `date_last_played`. Use [`Library::from`] to apply
/// the library header's timezone offset.
impl From<&TrackItem> for Track {
    fn from(itma: &TrackItem) -> Self {
        Self::from_raw_with_tz(itma, 0)
    }
}

impl Track {
    fn from_header(h: &TrackItemHeader) -> Self {
        let cloud_saga_id = (h.cloud_saga_id != 0 && h.cloud_saga_id != 0xFFFF_FFFF_FFFF_FFFD)
            .then_some(h.cloud_saga_id);

        Self {
            persistent_id: PersistentId::new(h.track_persistent_id),
            movement_number: h.movement_number,
            movement_total: h.movement_total,
            sort_modes: TrackSortModes::from(h.auto_sort_flags),
            track_number: h.track_number,
            track_total: h.track_total,
            disc_number: h.disc_number,
            disc_total: h.disc_total,
            year: h.year,
            bpm: h.bpm,
            volume_adjustment: h.volume_adjustment,
            star_rating: h.star_rating,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            suggestion_locally_modified: SuggestionFlag::is_locally_modified(h.suggestion_flag),
            date_suggestion_flag_modified: h.date_suggestion_flag_modified.to_utc(),
            disabled: h.checkbox_disabled != 0,
            skip_when_shuffling: h.checkbox_skip_when_shuffling != 0,
            has_ever_been_played: h.has_ever_been_played != 0,
            remember_playback_position: h.checkbox_remember_playback_position != 0,
            show_composer_in_all_views: h.checkbox_show_composer_in_all_views != 0,
            use_work_and_movement: h.checkbox_use_work_and_movement != 0,
            album_is_compilation: h.checkbox_album_is_compilation != 0,
            part_of_gapless_album: h.checkbox_part_of_gapless_album != 0,
            playlist_only: h.playlist_only != 0,
            purchased: h.cloud_sync_state == 3 || h.purchased_album_group != 0,
            has_custom_lyrics: h.has_custom_lyrics != 0 || h.has_local_custom_lyrics != 0,
            has_local_custom_lyrics: h.has_local_custom_lyrics != 0,
            has_unsynced_local_changes: h.has_unsynced_local_changes != 0,
            cloud_saga_id,
            audio_traits: AudioTraits::from_bits_retain(h.audio_traits),
            apple_music_store_artist_id: (h.apple_music_store_artist_id != 0)
                .then_some(u64::from(h.apple_music_store_artist_id)),
            downloaded_state: DownloadedStatus::from(h.downloaded_status),
            cloud_status: CloudStatus::from(h.cloud_status),
            content_advisory: ContentRating::from(h.content_rating),
            media_type: MediaKind::from(h.media_kind),
            playback_start_ms: h.playback_start_ms,
            playback_stop_ms: h.playback_stop_ms,
            album_persistent_id: (h.album_persistent_id != 0)
                .then_some(PersistentId::new(h.album_persistent_id)),
            artist_persistent_id: (h.artist_persistent_id != 0)
                .then_some(PersistentId::new(h.artist_persistent_id)),
            date_cloud_synced: h.date_cloud_synced.to_utc(),
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            ..Self::default()
        }
    }

    pub(crate) fn from_raw_with_tz(itma: &TrackItem, tz_offset_seconds: i32) -> Self {
        let mut track = Self::from_header(&itma.header);
        let mut local_num: Option<&TrackNumerics> = None;
        let mut cloud_num: Option<&TrackNumerics> = None;
        let mut other_nums: Vec<&TrackNumerics> = Vec::new();
        for boma in &itma.bomas {
            if let BomaPayload::TrackNumerics(num) = &boma.payload {
                collect_track_numerics(num, &mut local_num, &mut cloud_num, &mut other_nums);
            } else {
                apply_track_boma(&mut track, boma, tz_offset_seconds);
            }
        }
        apply_collected_track_numerics(
            &mut track,
            local_num,
            cloud_num,
            &other_nums,
            tz_offset_seconds,
        );
        track
    }

    fn from_owned_with_tz(itma: TrackItem, tz_offset_seconds: i32) -> Self {
        let mut track = Self::from_header(&itma.header);
        let mut local_num: Option<Box<TrackNumerics>> = None;
        let mut cloud_num: Option<Box<TrackNumerics>> = None;
        let mut other_nums: Vec<Box<TrackNumerics>> = Vec::new();
        for boma in itma.bomas {
            if let BomaPayload::TrackNumerics(num) = boma.payload {
                match num.subrecord_kind {
                    1 if local_num.is_none() => local_num = Some(num),
                    3 if cloud_num.is_none() => cloud_num = Some(num),
                    _ => other_nums.push(num),
                }
            } else {
                apply_track_boma_owned(&mut track, boma, tz_offset_seconds);
            }
        }
        let other_refs: Vec<&TrackNumerics> = other_nums.iter().map(Box::as_ref).collect();
        apply_collected_track_numerics(
            &mut track,
            local_num.as_deref(),
            cloud_num.as_deref(),
            &other_refs,
            tz_offset_seconds,
        );
        track
    }
}

fn collect_track_numerics<'a>(
    num: &'a TrackNumerics,
    local_num: &mut Option<&'a TrackNumerics>,
    cloud_num: &mut Option<&'a TrackNumerics>,
    other_nums: &mut Vec<&'a TrackNumerics>,
) {
    match num.subrecord_kind {
        1 if local_num.is_none() => *local_num = Some(num),
        3 if cloud_num.is_none() => *cloud_num = Some(num),
        _ => other_nums.push(num),
    }
}

fn apply_collected_track_numerics(
    track: &mut Track,
    local_num: Option<&TrackNumerics>,
    cloud_num: Option<&TrackNumerics>,
    other_nums: &[&TrackNumerics],
    tz_offset_seconds: i32,
) {
    let mut ordered = local_num
        .into_iter()
        .chain(cloud_num)
        .chain(other_nums.iter().copied());
    if let Some(primary) = ordered.next() {
        apply_primary_track_numerics(track, primary, tz_offset_seconds);
        for secondary in ordered {
            apply_secondary_track_numerics(track, secondary);
        }
    }
}

fn apply_track_plays_skips(track: &mut Track, ps: &TrackPlaysSkips, tz_offset_seconds: i32) {
    if track.play_count.is_none() {
        track.play_count = Some(ps.play_count);
        track.true_play_count = Some(ps.true_play_count);
        track.skip_count = Some(ps.skip_count);
        track.true_skip_count = Some(ps.true_skip_count);
        track.date_last_played = ps.date_last_played.to_utc_with_offset(tz_offset_seconds);
        track.date_first_played_utc = ps.date_first_played_utc.to_utc();
        track.date_last_skipped = ps.date_last_skipped.to_utc_with_offset(tz_offset_seconds);
    }
}

fn apply_track_boma(track: &mut Track, boma: &BomaSection, tz_offset_seconds: i32) {
    match &boma.payload {
        BomaPayload::TrackPlaysSkips(ps) => apply_track_plays_skips(track, ps, tz_offset_seconds),
        BomaPayload::VideoInfo(vi) => {
            if track.video_info.is_none() {
                track.video_info = Some(TrackVideoInfo::from(vi.as_ref()));
            }
        }
        BomaPayload::Book { extracted_path, .. } => {
            if track.book_path.is_none() {
                track.book_path.clone_from(extracted_path);
            }
        }
        BomaPayload::BinaryPlist { raw_bytes, .. } if boma.subtype == 0x0036 => {
            apply_track_cloud_plist(track, raw_bytes);
        }
        BomaPayload::RawStringUtf8 { text, raw_bytes } => {
            apply_track_string_boma(track, boma.subtype, Some(raw_bytes), text.clone());
        }
        BomaPayload::StringSection(s) => {
            apply_track_string_boma(track, boma.subtype, None, s.text.clone());
        }
        BomaPayload::RawStringUtf16Le { text, .. } => {
            apply_track_string_boma(track, boma.subtype, None, text.clone());
        }
        _ => {}
    }
}

fn apply_track_boma_owned(track: &mut Track, boma: BomaSection, tz_offset_seconds: i32) {
    let subtype = boma.subtype;
    match boma.payload {
        BomaPayload::TrackPlaysSkips(ps) => apply_track_plays_skips(track, &ps, tz_offset_seconds),
        BomaPayload::VideoInfo(vi) => {
            if track.video_info.is_none() {
                track.video_info = Some(TrackVideoInfo::from(vi.as_ref()));
            }
        }
        BomaPayload::Book { extracted_path, .. } => {
            if track.book_path.is_none() {
                track.book_path = extracted_path;
            }
        }
        BomaPayload::BinaryPlist { raw_bytes, .. } if subtype == 0x0036 => {
            apply_track_cloud_plist(track, &raw_bytes);
        }
        BomaPayload::RawStringUtf8 { text, raw_bytes } => {
            apply_track_string_boma(track, subtype, Some(&raw_bytes), text);
        }
        BomaPayload::StringSection(s) => {
            apply_track_string_boma(track, subtype, None, s.text);
        }
        BomaPayload::RawStringUtf16Le { text, .. } => {
            apply_track_string_boma(track, subtype, None, text);
        }
        _ => {}
    }
}

fn apply_primary_track_numerics(track: &mut Track, num: &TrackNumerics, tz_offset_seconds: i32) {
    track.duration_ms = Some(num.duration_ms);
    track.file_size = Some(num.file_size);
    track.bit_rate = Some(num.bit_rate);
    track.is_vbr = Some(num.is_vbr != 0);
    track.sample_rate = Some(num.sample_rate);
    track.file_type_fourcc = num.file_type_fourcc_str();
    track.codec_fourcc = num.codec_fourcc_str();
    track.normalization = (num.normalization != 0).then_some(num.normalization);
    track.gapless_encoder_delay =
        (num.gapless_encoder_delay != 0).then_some(num.gapless_encoder_delay);
    track.gapless_encoder_padding =
        (num.gapless_encoder_padding != 0).then_some(num.gapless_encoder_padding);
    track.gapless_total_samples =
        (num.gapless_total_samples != 0).then_some(num.gapless_total_samples);
    track.gapless_last_eight_frames_offset =
        (num.gapless_last_eight_frames_offset != 0).then_some(num.gapless_last_eight_frames_offset);
    track.store_genre_id = (num.store_genre_id != 0).then_some(num.store_genre_id);
    track.custom_lyrics_hash = (num.custom_lyrics_hash != 0).then_some(num.custom_lyrics_hash);
    track.date_added = num.date_added.to_utc_with_offset(tz_offset_seconds);
    track.date_modified = num.date_modified.to_utc_with_offset(tz_offset_seconds);
    track.date_purchased = num.date_purchased.to_utc();
    if !num.date_purchased.is_zero()
        || (num.subrecord_kind == 1
            && (num.store_track_adam_id != 0 || num.storefront_id != 0)
            && num.is_itunes_matched == [0, 0]
            && num.is_apple_music_catalog == 0)
    {
        track.purchased = true;
    }
    track.date_released = num.date_released.to_utc_release_for_year(track.year);
    if let Some(ts) = num.date_cloud_synced.to_utc() {
        track.date_cloud_synced = Some(ts);
    }
    apply_catalog_lyrics_state(track, num.catalog_lyrics_state);
    track.is_itunes_matched = Some(num.is_itunes_matched != [0, 0]);
    track.is_apple_music_catalog = Some(num.is_apple_music_catalog != 0);
    track.is_apple_digital_master = Some(num.is_apple_digital_master != 0);
    track.is_cloud_unavailable = Some(num.is_cloud_unavailable != 0);
    track.is_available_in_catalog = Some(num.is_available_in_catalog != 0);
    if num.purchaser_account_id != 0 {
        track.purchaser_account_id = Some(num.purchaser_account_id);
    }
    if num.drm_account_id != 0 {
        track.drm_account_id = Some(num.drm_account_id);
    }
    if num.subrecord_kind == 1
        && let Ok(bits) = u8::try_from(num.downloaded_audio_traits)
        && bits != 0
    {
        track.downloaded_audio_traits = AudioTraits::from_bits_retain(bits);
    }
    if num.store_track_adam_id != 0 {
        track.store_track_adam_id = Some(num.store_track_adam_id);
    }
    if num.apple_music_store_album_id != 0 {
        track.apple_music_store_album_id = Some(num.apple_music_store_album_id);
    }
    if num.apple_music_store_artist_id != 0 {
        track.apple_music_store_artist_id = Some(num.apple_music_store_artist_id);
    }
    if num.apple_music_store_album_artist_id != 0 {
        track.apple_music_store_album_artist_id = Some(num.apple_music_store_album_artist_id);
    }
    if num.storefront_id != 0 {
        track.storefront_id = Some(num.storefront_id);
    }
    track.artwork_count = Some(num.artwork_count);
    track.embedded_artwork = Some(EmbeddedArtworkKind::from(num.has_embedded_artwork));
    track.artwork_total_size_bytes = Some(num.artwork_total_size_bytes);
    track.file_folder_count = Some(num.file_folder_count);
    track.library_folder_count = Some(num.library_folder_count);
}

fn apply_catalog_lyrics_state(track: &mut Track, state: u8) {
    match state {
        1 => track.has_catalog_synced_lyrics = Some(true),
        2 if track.has_catalog_synced_lyrics.is_none() => {
            track.has_catalog_synced_lyrics = Some(false);
        }
        _ => {}
    }
}

fn merge_secondary_embedded_artwork(track: &mut Track, raw: i16) {
    let secondary_art = EmbeddedArtworkKind::from(raw);
    let current_is_local = track
        .embedded_artwork
        .is_some_and(EmbeddedArtworkKind::has_local_artwork);
    if (!current_is_local && secondary_art.has_local_artwork())
        || (matches!(
            track.embedded_artwork,
            None | Some(EmbeddedArtworkKind::None)
        ) && secondary_art != EmbeddedArtworkKind::None)
    {
        track.embedded_artwork = Some(secondary_art);
    }
}

fn apply_secondary_track_numerics(track: &mut Track, num: &TrackNumerics) {
    apply_catalog_lyrics_state(track, num.catalog_lyrics_state);
    if track.date_cloud_synced.is_none() {
        track.date_cloud_synced = num.date_cloud_synced.to_utc();
    }
    if track.date_released.is_none() {
        track.date_released = num.date_released.to_utc_release_for_year(track.year);
    }
    if track.date_purchased.is_none() && !num.date_purchased.is_zero() {
        track.date_purchased = num.date_purchased.to_utc();
        track.purchased = true;
    }
    if track.artwork_count.unwrap_or(0) == 0 && num.artwork_count != 0 {
        track.artwork_count = Some(num.artwork_count);
    }
    merge_secondary_embedded_artwork(track, num.has_embedded_artwork);
    if track.artwork_total_size_bytes.unwrap_or(0) == 0 && num.artwork_total_size_bytes != 0 {
        track.artwork_total_size_bytes = Some(num.artwork_total_size_bytes);
    }
    if track.store_track_adam_id.is_none() && num.store_track_adam_id != 0 {
        track.store_track_adam_id = Some(num.store_track_adam_id);
    }
    if track.apple_music_store_album_id.is_none() && num.apple_music_store_album_id != 0 {
        track.apple_music_store_album_id = Some(num.apple_music_store_album_id);
    }
    if (track.apple_music_store_artist_id.is_none()
        || track.apple_music_store_artist_id == Some(num.apple_music_store_artist_id & 0xFFFF_FFFF))
        && num.apple_music_store_artist_id != 0
    {
        track.apple_music_store_artist_id = Some(num.apple_music_store_artist_id);
    }
    if track.apple_music_store_album_artist_id.is_none()
        && num.apple_music_store_album_artist_id != 0
    {
        track.apple_music_store_album_artist_id = Some(num.apple_music_store_album_artist_id);
    }
    if track.storefront_id.is_none() && num.storefront_id != 0 {
        track.storefront_id = Some(num.storefront_id);
    }
    if track.store_genre_id.is_none() && num.store_genre_id != 0 {
        track.store_genre_id = Some(num.store_genre_id);
    }
    if num.is_itunes_matched != [0, 0] {
        track.is_itunes_matched = Some(true);
    }
    if num.is_apple_music_catalog != 0 {
        track.is_apple_music_catalog = Some(true);
    }
    if num.is_apple_digital_master != 0 {
        track.is_apple_digital_master = Some(true);
    }
    if num.is_cloud_unavailable != 0 {
        track.is_cloud_unavailable = Some(true);
    }
    if num.is_available_in_catalog != 0 {
        track.is_available_in_catalog = Some(true);
    }
    if track.purchaser_account_id.is_none() && num.purchaser_account_id != 0 {
        track.purchaser_account_id = Some(num.purchaser_account_id);
    }
    if track.drm_account_id.is_none() && num.drm_account_id != 0 {
        track.drm_account_id = Some(num.drm_account_id);
    }
    if track.downloaded_audio_traits.is_empty()
        && num.subrecord_kind == 1
        && let Ok(bits) = u8::try_from(num.downloaded_audio_traits)
        && bits != 0
    {
        track.downloaded_audio_traits = AudioTraits::from_bits_retain(bits);
    }
    if track.codec_fourcc.is_none() {
        track.codec_fourcc = num.codec_fourcc_str();
    }
}

fn extra_string_key(subtype: u32) -> String {
    match subtype {
        0x0036 => "0x0036_plist_artwork".to_owned(),
        0x0038 => "0x0038_plist_redownload_params".to_owned(),
        other => format!("0x{other:04X}_{}", boma_subtype_name(other)),
    }
}

fn apply_track_string_boma(track: &mut Track, subtype: u32, raw_bytes: Option<&[u8]>, val: String) {
    let target = match subtype {
        0x0002 => &mut track.title,
        0x0003 => &mut track.album,
        0x0004 => &mut track.artist,
        0x0005 => &mut track.genre,
        0x0006 => &mut track.kind,
        0x0007 => {
            if track.equalizer_preset.is_none() {
                track.equalizer_preset = Some(EqualizerPreset::from(val.as_str()));
            }
            return;
        }
        0x0008 => &mut track.comments,
        0x000B => &mut track.file_url,
        0x000C => &mut track.composer,
        0x000E => &mut track.grouping,
        0x001B => &mut track.album_artist,
        0x001E => &mut track.sort_title,
        0x001F => &mut track.sort_album,
        0x0020 => &mut track.sort_artist,
        0x0021 => &mut track.sort_album_artist,
        0x0022 => &mut track.sort_composer,
        0x002B => &mut track.isrc,
        0x002E => &mut track.copyright,
        0x003B => &mut track.purchaser_email,
        0x003C => &mut track.purchaser_name,
        0x003F => &mut track.work_name,
        0x0040 => &mut track.movement_name,
        0x0043 => &mut track.legacy_file_path,
        0x0053 => &mut track.sort_show,
        0x0036 => {
            apply_track_cloud_plist(track, raw_bytes.unwrap_or(val.as_bytes()));
            return;
        }
        other => {
            track
                .extra_strings
                .entry(extra_string_key(other))
                .or_insert(val);
            return;
        }
    };
    target.get_or_insert(val);
}

#[cfg(feature = "serde")]
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct TrackCloudPlist {
    #[serde(rename = "cloud-album-id")]
    album_id: LossyPlistString,
    #[serde(rename = "cloud-artist-id")]
    artist_id: LossyPlistString,
    #[serde(rename = "cloud-artwork-token")]
    artwork_token: LossyPlistString,
    #[serde(rename = "cloud-lyrics")]
    lyrics: LossyPlistString,
    #[serde(rename = "cloud-lyrics-token")]
    lyrics_token: LossyPlistString,
}

fn apply_track_cloud_plist(track: &mut Track, raw_bytes: &[u8]) {
    #[cfg(feature = "serde")]
    if let Ok(parsed) = plist::from_bytes::<TrackCloudPlist>(raw_bytes) {
        if let Some(v) = parsed.album_id.0 {
            track.cloud_album_id.get_or_insert(v);
        }
        if let Some(v) = parsed.artist_id.0 {
            track.cloud_artist_id.get_or_insert(v);
        }
        if let Some(v) = parsed.artwork_token.0 {
            track.cloud_artwork_token.get_or_insert(v);
        }
        if let Some(v) = parsed.lyrics.0.or(parsed.lyrics_token.0) {
            track.cloud_lyrics_token.get_or_insert(v);
        }
    }

    #[cfg(not(feature = "serde"))]
    if let Ok(plist::Value::Dictionary(dict)) =
        plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
    {
        if let Some(v) = dict.get("cloud-album-id").and_then(plist::Value::as_string) {
            track.cloud_album_id.get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("cloud-artist-id")
            .and_then(plist::Value::as_string)
        {
            track.cloud_artist_id.get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("cloud-artwork-token")
            .and_then(plist::Value::as_string)
        {
            track
                .cloud_artwork_token
                .get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("cloud-lyrics")
            .or_else(|| dict.get("cloud-lyrics-token"))
            .and_then(plist::Value::as_string)
        {
            track.cloud_lyrics_token.get_or_insert_with(|| v.to_owned());
        }
    }
}

impl Playlist {
    fn from_header(h: &PlaylistItemHeader) -> Self {
        Self {
            persistent_id: PersistentId::new(h.playlist_persistent_id),
            parent_persistent_id: (h.parent_persistent_id != 0)
                .then_some(PersistentId::new(h.parent_persistent_id)),
            is_master: h.is_master_playlist != 0,
            is_folder: h.is_folder != 0,
            is_subscribed: h.is_subscribed_playlist != 0,
            show_in_profile_and_search: h.checkbox_show_in_profile_and_search != 0,
            system_kind: DistinguishedPlaylistKind::from(h.distinguished_kind),
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            suggestion_locally_modified: SuggestionFlag::is_locally_modified(h.suggestion_flag),
            date_suggestion_flag_modified: h
                .date_suggestion_flag_modified
                .and_then(AppleTimestamp::to_utc),
            date_created: h.date_created.to_utc(),
            date_modified: h.date_modified.to_utc(),
            date_last_played: h.date_last_played.to_utc(),
            date_cloud_published: h.date_cloud_published.to_utc(),
            date_cloud_synced: h.date_cloud_synced_2.and_then(AppleTimestamp::to_utc),
            play_count: h.play_count,
            true_play_count: h.true_play_count,
            refresh_interval_seconds: h.refresh_interval_seconds,
            share_mode: PlaylistShareMode::from(h.cloud_share_mode),
            artwork_cache_state: h.artwork_cache_state,
            cloud_saga_id: (h.cloud_playlist_saga_id_1 != 0).then_some(h.cloud_playlist_saga_id_1),
            author_social_profile_persistent_id: h
                .author_social_profile_persistent_id
                .filter(|&id| id != 0)
                .map(PersistentId::new),
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            declared_track_count: h.track_count,
            ..Self::default()
        }
    }

    fn finalize_playlist(
        &mut self,
        h: &PlaylistItemHeader,
        track_persistent_ids: Vec<PersistentId>,
        raw_smart_rules: Option<&SlstSection>,
        raw_smart_options: Option<&SmartPlaylistOptions>,
    ) {
        self.track_persistent_ids = track_persistent_ids;
        let has_smart_blocks = raw_smart_rules.is_some() || raw_smart_options.is_some();
        self.is_smart = !self.is_folder && has_smart_blocks;
        self.smart_playlist = self
            .is_smart
            .then(|| decode_smart_playlist(raw_smart_options, raw_smart_rules));

        self.kind = if self.is_master {
            PlaylistKind::Master
        } else if self.is_folder {
            PlaylistKind::Folder
        } else if h.distinguished_kind != 0 {
            PlaylistKind::Distinguished
        } else if self.is_smart {
            PlaylistKind::Smart
        } else if self.share_mode == PlaylistShareMode::SubscribedCatalog
            || self.share_mode == PlaylistShareMode::Personalized
            || self.subscribed_container_url.is_some()
        {
            PlaylistKind::SubscribedCatalog
        } else {
            PlaylistKind::Standard
        };
    }

    fn from_owned(lpma: PlaylistItem) -> Self {
        let h = &lpma.header;
        let mut raw_smart_rules: Option<Box<SlstSection>> = None;
        let mut raw_smart_options: Option<Box<SmartPlaylistOptions>> = None;
        let initial_cap = usize::try_from(h.track_count)
            .unwrap_or(0)
            .min(lpma.bomas.len());
        let mut track_persistent_ids = Vec::with_capacity(initial_cap);
        let mut pl = Self::from_header(h);

        for boma in lpma.bomas {
            apply_playlist_boma_owned(
                &mut pl,
                boma,
                &mut track_persistent_ids,
                &mut raw_smart_rules,
                &mut raw_smart_options,
            );
        }

        pl.finalize_playlist(
            h,
            track_persistent_ids,
            raw_smart_rules.as_deref(),
            raw_smart_options.as_deref(),
        );
        pl
    }
}

impl From<&PlaylistItem> for Playlist {
    fn from(lpma: &PlaylistItem) -> Self {
        let h = &lpma.header;
        let mut raw_smart_rules: Option<&SlstSection> = None;
        let mut raw_smart_options: Option<&SmartPlaylistOptions> = None;
        let initial_cap = usize::try_from(h.track_count)
            .unwrap_or(0)
            .min(lpma.bomas.len());
        let mut track_persistent_ids = Vec::with_capacity(initial_cap);
        let mut pl = Self::from_header(h);

        for boma in &lpma.bomas {
            apply_playlist_boma(
                &mut pl,
                boma,
                &mut track_persistent_ids,
                &mut raw_smart_rules,
                &mut raw_smart_options,
            );
        }

        pl.finalize_playlist(h, track_persistent_ids, raw_smart_rules, raw_smart_options);
        pl
    }
}

fn apply_playlist_boma<'a>(
    pl: &mut Playlist,
    boma: &'a BomaSection,
    track_persistent_ids: &mut Vec<PersistentId>,
    raw_smart_rules: &mut Option<&'a SlstSection>,
    raw_smart_options: &mut Option<&'a SmartPlaylistOptions>,
) {
    match &boma.payload {
        BomaPayload::PlaylistItem(ipfa) => {
            track_persistent_ids.push(PersistentId::new(ipfa.track_persistent_id));
        }
        BomaPayload::PlaylistItemCompact(ipfa) => {
            track_persistent_ids.push(PersistentId::new(ipfa.track_persistent_id));
        }
        BomaPayload::SmartPlaylistRules(slst) => {
            *raw_smart_rules = Some(slst.as_ref());
        }
        BomaPayload::SmartPlaylistOptions(opts) => {
            *raw_smart_options = Some(opts.as_ref());
        }
        _ => {
            if boma.subtype == 0x00C8
                && let Some(s) = boma.as_str()
            {
                pl.name = Some(s.to_owned());
            } else if boma.subtype == 0x00CD
                && let BomaPayload::RawStringUtf8 { raw_bytes, .. }
                | BomaPayload::BinaryPlist { raw_bytes, .. } = &boma.payload
            {
                apply_playlist_cloud_plist(pl, raw_bytes);
            }
        }
    }
}

fn apply_playlist_boma_owned(
    pl: &mut Playlist,
    boma: BomaSection,
    track_persistent_ids: &mut Vec<PersistentId>,
    raw_smart_rules: &mut Option<Box<SlstSection>>,
    raw_smart_options: &mut Option<Box<SmartPlaylistOptions>>,
) {
    let subtype = boma.subtype;
    match boma.payload {
        BomaPayload::PlaylistItem(ipfa) => {
            track_persistent_ids.push(PersistentId::new(ipfa.track_persistent_id));
        }
        BomaPayload::PlaylistItemCompact(ipfa) => {
            track_persistent_ids.push(PersistentId::new(ipfa.track_persistent_id));
        }
        BomaPayload::SmartPlaylistRules(slst) => {
            *raw_smart_rules = Some(slst);
        }
        BomaPayload::SmartPlaylistOptions(opts) => {
            *raw_smart_options = Some(opts);
        }
        BomaPayload::RawStringUtf8 { raw_bytes, .. }
        | BomaPayload::BinaryPlist { raw_bytes, .. }
            if subtype == 0x00CD =>
        {
            apply_playlist_cloud_plist(pl, &raw_bytes);
        }
        payload if subtype == 0x00C8 => {
            let owned_boma = BomaSection { payload, ..boma };
            if let Some(s) = owned_boma.into_string() {
                pl.name = Some(s);
            }
        }
        _ => {}
    }
}

#[cfg(feature = "serde")]
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct PlaylistCloudPlist {
    #[serde(rename = "universal-library-id")]
    universal_library_id: LossyPlistString,
    #[serde(rename = "uuid")]
    cloud_uuid: LossyPlistString,
    #[serde(rename = "subscribed-container-url")]
    subscribed_container_url: LossyPlistString,
    #[serde(rename = "ownerName")]
    owner_name: LossyPlistString,
    #[serde(rename = "ownerID")]
    owner_id: LossyPlistString,
    #[serde(rename = "description")]
    description: LossyPlistString,
    #[serde(rename = "cloud-artwork-url")]
    cloud_artwork_url: LossyPlistString,
    #[serde(rename = "cloud-artwork-token")]
    cloud_artwork_token: LossyPlistString,
}

fn apply_playlist_cloud_plist(pl: &mut Playlist, raw_bytes: &[u8]) {
    #[cfg(feature = "serde")]
    {
        if let Ok(parsed) = plist::from_bytes::<PlaylistCloudPlist>(raw_bytes) {
            pl.universal_library_id = parsed.universal_library_id.0;
            pl.cloud_uuid = parsed.cloud_uuid.0;
            pl.subscribed_container_url = parsed.subscribed_container_url.0;
            pl.owner_name = parsed.owner_name.0;
            pl.owner_id = parsed.owner_id.0;
            pl.description = parsed.description.0;
            pl.cloud_artwork_url = parsed.cloud_artwork_url.0;
            pl.cloud_artwork_token = parsed.cloud_artwork_token.0;
        }
    }

    #[cfg(not(feature = "serde"))]
    {
        let Ok(plist::Value::Dictionary(dict)) =
            plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
        else {
            return;
        };
        pl.universal_library_id = dict
            .get("universal-library-id")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
        pl.cloud_uuid = dict
            .get("uuid")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
        pl.subscribed_container_url = dict
            .get("subscribed-container-url")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
        pl.owner_name = dict
            .get("ownerName")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
        pl.owner_id = dict
            .get("ownerID")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
        pl.description = dict
            .get("description")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
        pl.cloud_artwork_url = dict
            .get("cloud-artwork-url")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
        pl.cloud_artwork_token = dict
            .get("cloud-artwork-token")
            .and_then(plist::Value::as_string)
            .map(str::to_owned);
    }
}

fn decode_smart_playlist(
    opts: Option<&SmartPlaylistOptions>,
    slst: Option<&SlstSection>,
) -> SmartPlaylistConfig {
    let (live_updating, match_rules, match_only_checked_items, limit) = if let Some(o) = opts {
        let lim = (o.checkbox_enable_limit != 0).then(|| SmartPlaylistLimit {
            count: o.limit_count,
            unit: SmartLimitUnit::from(o.limit_unit),
            selection: SmartSelectionMethod::from((
                o.limit_selection_method,
                o.negate_limit_selection_method,
            )),
        });
        (
            o.checkbox_live_updating != 0,
            o.checkbox_enable_matching_rules != 0,
            o.checkbox_match_only_checked_items != 0,
            lim,
        )
    } else {
        (true, true, false, None)
    };

    let rule_group = slst.map(|s| decode_slst_group(s, false));

    SmartPlaylistConfig {
        live_updating,
        match_rules,
        match_only_checked_items,
        limit,
        rule_group,
    }
}

fn decode_slst_group(slst: &SlstSection, negated: bool) -> SmartRuleGroup {
    let conjunction = if slst.conjunction == 0 {
        SmartConjunction::All
    } else {
        SmartConjunction::Any
    };
    let rules = slst.rules.iter().map(decode_smart_rule).collect();
    SmartRuleGroup {
        conjunction,
        negated,
        rules,
    }
}

fn decode_smart_rule(rule: &SmartPlaylistRule) -> SmartRuleNode {
    if let SmartRuleChild::Nested(nested) = &rule.child {
        let negated = SmartOperatorFlags::from_bits_retain(rule.comparison_method)
            .contains(SmartOperatorFlags::NEGATE);
        return SmartRuleNode::Group(decode_slst_group(nested, negated));
    }

    let field = SmartField::from(rule.field_id);
    let operator = SmartOperator::from(rule.comparison_method);
    let value = match &rule.child {
        SmartRuleChild::StringUtf16Be { text, .. } => SmartRuleValue::Text { text: text.clone() },
        SmartRuleChild::Arguments(args) => decode_smart_rule_arguments(field, operator, args),
        SmartRuleChild::Raw(bytes) => SmartRuleValue::Raw {
            hex: bytes_to_lower_hex(bytes),
        },
        SmartRuleChild::Nested(_) => SmartRuleValue::Raw { hex: String::new() },
    };

    let description = format_smart_condition_description(rule, &value);

    SmartRuleNode::Condition(SmartCondition {
        field_id: rule.field_id,
        field,
        operator,
        comparison_raw: rule.comparison_method,
        value,
        description,
    })
}

fn decode_smart_rule_arguments(
    field: SmartField,
    operator: SmartOperator,
    args: &crate::sections::SmartPlaylistRuleArguments,
) -> SmartRuleValue {
    if matches!(
        operator,
        SmartOperator::InTheLast | SmartOperator::NotInTheLast
    ) {
        let amount = if args.arg_1 < 0 {
            args.arg_1.unsigned_abs()
        } else {
            0
        };
        let unit = match args.arg_2 {
            86_400 | 1 => RelativeTimeUnit::Days,
            604_800 | 2 => RelativeTimeUnit::Weeks,
            2_628_000 | 3 => RelativeTimeUnit::Months,
            other => RelativeTimeUnit::Unknown(other),
        };
        return SmartRuleValue::RelativeTime {
            amount,
            unit,
            seconds_per_unit: args.arg_2,
        };
    }

    let is_range = matches!(operator, SmartOperator::InRange | SmartOperator::NotInRange);
    match field {
        SmartField::Compilation
        | SmartField::Checked
        | SmartField::AlbumArtwork
        | SmartField::Purchased
        | SmartField::HasVideo => SmartRuleValue::Boolean {
            value: args.arg_0 != 0,
        },
        SmartField::Rating | SmartField::AlbumRating => decode_smart_rating_arg(args, is_range),
        SmartField::Playlist => SmartRuleValue::PlaylistRef {
            playlist_persistent_id: PersistentId::new(args.arg_0.cast_unsigned()),
            playlist_name: None,
        },
        SmartField::CloudStatus => SmartRuleValue::CloudStatus {
            status: CloudStatus::from(u8::try_from(args.arg_0).unwrap_or(u8::MAX)),
        },
        SmartField::Favorite | SmartField::AlbumFavorite => SmartRuleValue::SuggestionFlag {
            flag: SuggestionFlag::from(u16::try_from(args.arg_0).unwrap_or(u16::MAX)),
        },
        SmartField::MediaKind => SmartRuleValue::MediaKind {
            kind: MediaKind::from(u32::try_from(args.arg_0).unwrap_or(u32::MAX)),
        },
        SmartField::Location => {
            let name = match args.arg_0 {
                1 => "Computer",
                2 => "iCloud",
                16 => "On This Computer",
                _ => "Unknown Location",
            };
            SmartRuleValue::LocationKind {
                code: args.arg_0,
                name: name.to_owned(),
            }
        }
        SmartField::DateAdded
        | SmartField::DateModified
        | SmartField::LastPlayed
        | SmartField::LastSkipped => decode_smart_date_arg(args, is_range),
        _ if is_range => SmartRuleValue::IntegerRange {
            min: args.arg_0,
            max: args.arg_3,
        },
        _ => SmartRuleValue::Integer { value: args.arg_0 },
    }
}

fn decode_smart_rating_arg(
    args: &crate::sections::SmartPlaylistRuleArguments,
    is_range: bool,
) -> SmartRuleValue {
    if is_range {
        SmartRuleValue::StarRatingRange {
            min_stars: u8::try_from(args.arg_0.clamp(0, 100) / 20).unwrap_or(0),
            max_stars: u8::try_from(args.arg_3.clamp(0, 100) / 20).unwrap_or(0),
            min_raw: args.arg_0,
            max_raw: args.arg_3,
        }
    } else {
        SmartRuleValue::StarRating {
            stars: u8::try_from(args.arg_0.clamp(0, 100) / 20).unwrap_or(0),
            raw_0_100: args.arg_0,
        }
    }
}

fn decode_smart_date_arg(
    args: &crate::sections::SmartPlaylistRuleArguments,
    is_range: bool,
) -> SmartRuleValue {
    if is_range {
        SmartRuleValue::DateRange {
            start_apple: args.arg_0,
            end_apple: args.arg_3,
            start_utc: u32::try_from(args.arg_0)
                .ok()
                .map(AppleTimestamp::new)
                .and_then(AppleTimestamp::to_utc),
            end_utc: u32::try_from(args.arg_3)
                .ok()
                .map(AppleTimestamp::new)
                .and_then(AppleTimestamp::to_utc),
        }
    } else {
        SmartRuleValue::Date {
            apple_timestamp: args.arg_0,
            utc: u32::try_from(args.arg_0)
                .ok()
                .map(AppleTimestamp::new)
                .and_then(AppleTimestamp::to_utc),
        }
    }
}

fn format_smart_condition_description(rule: &SmartPlaylistRule, value: &SmartRuleValue) -> String {
    let fname = rule.field_name();
    let cmp = rule.comparison_description();
    let val_str = match value {
        SmartRuleValue::Text { text } => format!("{text:?}"),
        SmartRuleValue::Boolean { value: v } => v.to_string(),
        SmartRuleValue::Integer { value: v } => v.to_string(),
        SmartRuleValue::IntegerRange { min, max } => format!("{min} to {max}"),
        SmartRuleValue::StarRating { stars, .. } => format!("{stars} stars"),
        SmartRuleValue::StarRatingRange {
            min_stars,
            max_stars,
            ..
        } => format!("{min_stars} to {max_stars} stars"),
        SmartRuleValue::RelativeTime { amount, unit, .. } => format!("{amount} {unit}"),
        SmartRuleValue::Date {
            utc,
            apple_timestamp,
        } => utc.map_or_else(|| apple_timestamp.to_string(), |d| d.to_string()),
        SmartRuleValue::DateRange {
            start_utc,
            end_utc,
            start_apple,
            end_apple,
        } => format!(
            "{} to {}",
            start_utc.map_or_else(|| start_apple.to_string(), |d| d.to_string()),
            end_utc.map_or_else(|| end_apple.to_string(), |d| d.to_string())
        ),
        SmartRuleValue::PlaylistRef {
            playlist_persistent_id,
            playlist_name,
        } => match playlist_name {
            Some(n) => format!("{n:?} ({playlist_persistent_id})"),
            None => playlist_persistent_id.to_string(),
        },
        SmartRuleValue::CloudStatus { status } => status.to_string(),
        SmartRuleValue::SuggestionFlag { flag } => flag.to_string(),
        SmartRuleValue::MediaKind { kind } => kind.to_string(),
        SmartRuleValue::LocationKind { name, .. } => name.clone(),
        SmartRuleValue::Raw { hex } => format!("0x{hex}"),
    };
    format!("{fname} {cmp} {val_str}")
}

fn resolve_playlist_refs_in_group(
    group: &mut SmartRuleGroup,
    playlist_names: &HashMap<PersistentId, String>,
) {
    for node in &mut group.rules {
        match node {
            SmartRuleNode::Group(sub) => resolve_playlist_refs_in_group(sub, playlist_names),
            SmartRuleNode::Condition(cond) => {
                if let SmartRuleValue::PlaylistRef {
                    playlist_persistent_id,
                    playlist_name,
                } = &mut cond.value
                    && let Some(resolved) = playlist_names.get(playlist_persistent_id)
                {
                    *playlist_name = Some(resolved.clone());
                    cond.description = format!(
                        "Playlist {} {resolved:?} ({playlist_persistent_id})",
                        if cond.operator == SmartOperator::IsNot {
                            "is not"
                        } else {
                            "is"
                        },
                    );
                }
            }
        }
    }
}

fn load_optional_view_plist(
    path: &Path,
    warnings: &mut Vec<String>,
) -> Option<PlaylistViewPreferences> {
    match parse_playlist_view_plist(path) {
        Ok(vp) => Some(vp),
        Err(msg) => {
            warnings.push(msg);
            None
        }
    }
}

fn parse_playlist_view_plist(path: &Path) -> std::result::Result<PlaylistViewPreferences, String> {
    let val = plist::Value::from_file(path)
        .map_err(|err| format!("failed to parse sidecar {}: {err}", path.display()))?;
    let dict = val
        .as_dictionary()
        .ok_or_else(|| format!("sidecar {} root is not a dictionary", path.display()))?;

    let mut view_as = dict
        .get("LibraryPage_ViewAs")
        .and_then(decode_view_as_value);

    let view_dict = if let Some(views) = dict
        .get("LibraryPage_Views")
        .and_then(plist::Value::as_dictionary)
    {
        if view_as.is_none() {
            view_as = views.keys().next().cloned();
        }
        view_as
            .as_deref()
            .and_then(|k| views.get(k))
            .or_else(|| views.values().next())
            .and_then(plist::Value::as_dictionary)
    } else {
        Some(dict)
    };

    let mut sort_field = None;
    let mut sort_ascending = None;
    let mut visible_columns = Vec::new();

    if let Some(vd) = view_dict {
        sort_field = vd
            .get("LibraryPage_View_SortingPrimary")
            .and_then(decode_sort_field_value);
        sort_ascending = vd
            .get("LibraryPage_View_SortingReverse")
            .and_then(plist::Value::as_boolean)
            .map(|rev| !rev);
        extract_visible_columns(vd, &mut sort_ascending, &mut visible_columns);
    }

    Ok(PlaylistViewPreferences {
        view_as,
        sort_field,
        sort_ascending,
        visible_columns,
    })
}

fn decode_view_as_value(v: &plist::Value) -> Option<String> {
    if let Some(s) = v.as_string() {
        Some(s.to_owned())
    } else {
        v.as_signed_integer().map(|code| match code {
            1 => "Songs".to_owned(),
            7 => "Albums".to_owned(),
            8 => "Artists".to_owned(),
            11 => "Playlist".to_owned(),
            other => format!("ViewAs({other})"),
        })
    }
}

fn decode_sort_field_value(v: &plist::Value) -> Option<String> {
    if let Some(s) = v.as_string() {
        Some(s.to_owned())
    } else {
        v.as_signed_integer().map(|code| match code {
            0 => "PlaylistOrder".to_owned(),
            102 => "AlbumByArtist".to_owned(),
            104 => "Artist".to_owned(),
            106 => "Title".to_owned(),
            117 => "Album".to_owned(),
            1027 => "CloudOrArtist".to_owned(),
            25_001 => "Custom".to_owned(),
            other => format!("SortField({other})"),
        })
    }
}

fn extract_visible_columns(
    vd: &plist::Dictionary,
    sort_ascending: &mut Option<bool>,
    visible_columns: &mut Vec<String>,
) {
    if let Some(cols_arr) = vd
        .get("LibraryPage_View_ListColumns")
        .and_then(plist::Value::as_array)
    {
        for col in cols_arr {
            if let Some(cd) = col.as_dictionary() {
                if sort_ascending.is_none()
                    && let Some(asc) = cd
                        .get("LibraryPage_View_ListColumn_Ascending")
                        .and_then(plist::Value::as_boolean)
                {
                    *sort_ascending = Some(asc);
                }
                if let Some(kind) = cd
                    .get("LibraryPage_View_ListColumn_Kind")
                    .and_then(plist::Value::as_signed_integer)
                {
                    let col_name = match kind {
                        0 => "CheckBox".to_owned(),
                        1 => "Position".to_owned(),
                        3 => "Title".to_owned(),
                        4 => "Artist".to_owned(),
                        7 => "Album".to_owned(),
                        10 => "Genre".to_owned(),
                        11 => "Rating".to_owned(),
                        22 => "Plays".to_owned(),
                        27 => "CloudStatus".to_owned(),
                        30 => "Duration".to_owned(),
                        46 => "Favorite".to_owned(),
                        other => format!("Column({other})"),
                    };
                    visible_columns.push(col_name);
                }
            }
        }
    } else if let Some(cols_dict) = vd
        .get("LibraryPage_View_ListColumns")
        .and_then(plist::Value::as_dictionary)
    {
        for k in cols_dict.keys() {
            visible_columns.push(k.clone());
        }
    }
}

fn bytes_to_lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_track_cloud_plist_handles_xml_nested_entities_and_malformed() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>ignored-int</key><integer>2112</integer>
    <key>ignored-flag</key><true/>
    <key>nested</key>
    <dict>
        <key>cloud-album-id</key><string>SHOULD_BE_IGNORED</string>
        <key>items</key><array><string>Limelight</string></array>
    </dict>
    <key>cloud-lyrics-token</key><string>fallback-token</string>
    <key>cloud-album-id</key><string>l.rush-moving-pictures</string>
    <key>cloud-artist-id</key><string>r.geddy&amp;alex&amp;neil</string>
    <key>cloud-artwork-token</key><string>tok-yyz</string>
    <key>cloud-lyrics</key><string>preferred-lyrics</string>
</dict>
</plist>"#;

        let mut track = Track::default();
        apply_track_cloud_plist(&mut track, xml);
        assert_eq!(
            track.cloud_album_id.as_deref(),
            Some("l.rush-moving-pictures")
        );
        assert_eq!(track.cloud_artist_id.as_deref(), Some("r.geddy&alex&neil"));
        assert_eq!(track.cloud_artwork_token.as_deref(), Some("tok-yyz"));
        assert_eq!(
            track.cloud_lyrics_token.as_deref(),
            Some("preferred-lyrics")
        );
        assert!(!track.extra_strings.contains_key("0x0036_plist_artwork"));

        // Truncated plist must not commit partially-parsed fields.
        let truncated =
            b"<plist version=\"1.0\"><dict><key>cloud-album-id</key><string>l.partial</string>";
        let mut partial_track = Track::default();
        apply_track_cloud_plist(&mut partial_track, truncated);
        assert_eq!(partial_track.cloud_album_id, None);
    }

    #[test]
    fn streaming_album_motion_plist_handles_binary_plist() {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "motion-artwork-url".to_owned(),
            plist::Value::String("https://example.invalid/rush/signals.m3u8".to_owned()),
        );
        dict.insert(
            "motion-preview-frame-url".to_owned(),
            plist::Value::String("https://example.invalid/rush/signals.jpg".to_owned()),
        );
        dict.insert(
            "motion-artwork-token".to_owned(),
            plist::Value::String("tok-subdivisions".to_owned()),
        );
        dict.insert("ignored-count".to_owned(), plist::Value::Integer(8.into()));

        let mut bplist_bytes = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut bplist_bytes)
            .expect("binary plist");

        let boma = BomaSection {
            header_size: 20,
            total_size: 20,
            subtype: 0x0135,
            unk_16_end: crate::sections::Unknown::new(crate::sections::TailBytes::from([0u8; 4])),
            payload: BomaPayload::BinaryPlist {
                subtype: 0x0135,
                raw_bytes: bplist_bytes,
            },
        };

        let mut url = None;
        let mut preview = None;
        let mut token = None;
        apply_album_motion_artwork_plist(&boma, &mut url, &mut preview, &mut token);
        assert_eq!(
            url.as_deref(),
            Some("https://example.invalid/rush/signals.m3u8")
        );
        assert_eq!(
            preview.as_deref(),
            Some("https://example.invalid/rush/signals.jpg")
        );
        assert_eq!(token.as_deref(), Some("tok-subdivisions"));
    }

    #[test]
    fn extra_string_key_matches_formatted_subtype_name() {
        for subtype in [0x0036, 0x0038, 0x0034, 0x9999] {
            assert_eq!(
                extra_string_key(subtype),
                format!("0x{subtype:04X}_{}", boma_subtype_name(subtype))
            );
        }
    }

    fn make_binary_plist_boma(subtype: u32, dict: &plist::Dictionary) -> BomaSection {
        let mut raw_bytes = Vec::new();
        plist::Value::Dictionary(dict.clone())
            .to_writer_binary(&mut raw_bytes)
            .expect("encode binary plist");
        BomaSection {
            header_size: 20,
            total_size: 20,
            subtype,
            unk_16_end: crate::sections::Unknown::new(crate::sections::TailBytes::from([0u8; 4])),
            payload: BomaPayload::BinaryPlist { subtype, raw_bytes },
        }
    }

    #[test]
    fn binary_plist_boma_populates_track_cloud_fields() {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "cloud-album-id".to_owned(),
            plist::Value::String("l.rush-permanent-waves".to_owned()),
        );
        dict.insert(
            "cloud-artist-id".to_owned(),
            plist::Value::String("r.rush".to_owned()),
        );
        dict.insert(
            "cloud-artwork-token".to_owned(),
            plist::Value::String("tok-spirit-of-radio".to_owned()),
        );
        dict.insert(
            "cloud-lyrics-token".to_owned(),
            plist::Value::String("lyr-freewill".to_owned()),
        );

        let track_item = TrackItem {
            header: TrackItemHeader::default(),
            bomas: vec![
                make_binary_plist_boma(0x0036, &dict),
                make_binary_plist_boma(0x0038, &dict),
            ],
        };
        for track in [
            Track::from_raw_with_tz(&track_item, 0),
            Track::from_owned_with_tz(track_item.clone(), 0),
        ] {
            assert_eq!(
                track.cloud_album_id.as_deref(),
                Some("l.rush-permanent-waves")
            );
            assert_eq!(track.cloud_artist_id.as_deref(), Some("r.rush"));
            assert_eq!(
                track.cloud_artwork_token.as_deref(),
                Some("tok-spirit-of-radio")
            );
            assert_eq!(track.cloud_lyrics_token.as_deref(), Some("lyr-freewill"));
            assert!(!track.extra_strings.contains_key("0x0036_plist_artwork"));
            assert!(
                !track
                    .extra_strings
                    .contains_key("0x0038_plist_redownload_params")
            );
        }
    }

    #[test]
    fn binary_plist_boma_populates_playlist_cloud_fields() {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "universal-library-id".to_owned(),
            plist::Value::String("pl.u-rush2112".to_owned()),
        );
        dict.insert(
            "cloud-artwork-url".to_owned(),
            plist::Value::String("https://example.invalid/rush/2112.jpg".to_owned()),
        );
        let pl_item = PlaylistItem {
            header: PlaylistItemHeader::default(),
            bomas: vec![make_binary_plist_boma(0x00CD, &dict)],
        };
        for pl in [
            Playlist::from(&pl_item),
            Playlist::from_owned(pl_item.clone()),
        ] {
            assert_eq!(pl.universal_library_id.as_deref(), Some("pl.u-rush2112"));
            assert_eq!(
                pl.cloud_artwork_url.as_deref(),
                Some("https://example.invalid/rush/2112.jpg")
            );
        }
    }

    #[test]
    fn artist_0x0192_populates_artwork_url_from_binary_and_xml_plists() {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "artwork-url".to_owned(),
            plist::Value::String("https://example.invalid/rush/artist.jpg".to_owned()),
        );
        let art_item_bin = ArtistItem {
            header: ArtistItemHeader::default(),
            bomas: vec![make_binary_plist_boma(0x0192, &dict)],
        };
        for artist in [
            Artist::from(&art_item_bin),
            Artist::from_owned(art_item_bin.clone()),
        ] {
            assert_eq!(
                artist.artwork_url.as_deref(),
                Some("https://example.invalid/rush/artist.jpg")
            );
        }

        let mut xml_bytes = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_xml(&mut xml_bytes)
            .expect("encode xml plist");
        let xml_text = String::from_utf8(xml_bytes.clone()).expect("utf8 xml");
        let art_item_xml = ArtistItem {
            header: ArtistItemHeader::default(),
            bomas: vec![BomaSection {
                header_size: 20,
                total_size: 20,
                subtype: 0x0192,
                unk_16_end: crate::sections::Unknown::new(crate::sections::TailBytes::from(
                    [0u8; 4],
                )),
                payload: BomaPayload::RawStringUtf8 {
                    text: xml_text,
                    raw_bytes: xml_bytes,
                },
            }],
        };
        for artist in [
            Artist::from(&art_item_xml),
            Artist::from_owned(art_item_xml.clone()),
        ] {
            assert_eq!(
                artist.artwork_url.as_deref(),
                Some("https://example.invalid/rush/artist.jpg")
            );
        }
    }

    #[test]
    fn tier1_discovered_enum_values_do_not_fall_through() {
        let ds = DownloadedStatus::from(0);
        assert_eq!(ds, DownloadedStatus::Unspecified);
        assert_eq!(ds.raw(), 0);
        assert_eq!(ds.to_string(), "Unspecified");

        let sm = PlaylistShareMode::from(1);
        assert_eq!(sm, PlaylistShareMode::Personalized);
        assert_eq!(sm.raw(), 1);
        assert_eq!(sm.to_string(), "Personalized");

        let fav_most = SmartSelectionMethod::from((0x7B, 0));
        assert_eq!(fav_most, SmartSelectionMethod::MostRecentlyFavorited);
        assert_eq!(fav_most.to_string(), "MostRecentlyFavorited");

        let fav_least = SmartSelectionMethod::from((0x7B, 1));
        assert_eq!(fav_least, SmartSelectionMethod::LeastRecentlyFavorited);
        assert_eq!(fav_least.to_string(), "LeastRecentlyFavorited");
    }

    #[test]
    fn track_exposes_date_suggestion_flag_modified() {
        let mut header = TrackItemHeader::default();
        let unset = Track::from_header(&header);
        assert_eq!(unset.date_suggestion_flag_modified, None);

        // 2024-01-01T00:00:00Z in Apple HFS+ epoch = 1_704_067_200 + 2_082_844_800 = 3_786_912_000
        header.date_suggestion_flag_modified = AppleTimestamp::new(3_786_912_000);
        let set = Track::from_header(&header);
        assert_eq!(
            set.date_suggestion_flag_modified.map(|t| t.to_string()),
            Some("2024-01-01T00:00:00Z".to_owned())
        );
    }

    fn make_arg_rule(
        field_id: u32,
        comparison_method: u32,
        args: crate::sections::SmartPlaylistRuleArguments,
    ) -> SmartPlaylistRule {
        SmartPlaylistRule {
            field_id,
            comparison_method,
            is_nested_rule_group: 0,
            unk_12_44: crate::sections::Unknown::new([0; 32]),
            unk_44: crate::sections::Unknown::new(0),
            unk_48: crate::sections::Unknown::new(0),
            child_size: 68,
            child: SmartRuleChild::Arguments(args),
        }
    }

    #[test]
    fn smart_rule_in_the_last_i64_min_does_not_overflow() {
        let rule = make_arg_rule(
            0x17,        // Last Played
            0x0000_0200, // InTheLast
            crate::sections::SmartPlaylistRuleArguments {
                arg_1: i64::MIN,
                arg_2: 86_400,
                ..Default::default()
            },
        );
        let node = decode_smart_rule(&rule);
        let SmartRuleNode::Condition(cond) = node else {
            panic!("expected SmartRuleNode::Condition");
        };
        assert_eq!(
            cond.value,
            SmartRuleValue::RelativeTime {
                amount: i64::MIN.unsigned_abs(),
                unit: RelativeTimeUnit::Days,
                seconds_per_unit: 86_400,
            }
        );
    }

    #[test]
    fn smart_rule_boolean_is_not_does_not_double_negate() {
        let is_not_true = make_arg_rule(
            0x1D,        // Checked
            0x0200_0001, // IsNot
            crate::sections::SmartPlaylistRuleArguments {
                arg_0: 1,
                ..Default::default()
            },
        );
        let SmartRuleNode::Condition(cond) = decode_smart_rule(&is_not_true) else {
            panic!("expected SmartRuleNode::Condition");
        };
        assert_eq!(cond.operator, SmartOperator::IsNot);
        assert_eq!(cond.value, SmartRuleValue::Boolean { value: true });
        assert_eq!(cond.description, "Checked is not true");

        let bitwise_true = make_arg_rule(
            0x1F,        // Compilation
            0x0000_0400, // BITWISE_SET
            crate::sections::SmartPlaylistRuleArguments {
                arg_0: 1,
                ..Default::default()
            },
        );
        let SmartRuleNode::Condition(bw_cond) = decode_smart_rule(&bitwise_true) else {
            panic!("expected SmartRuleNode::Condition");
        };
        assert_eq!(bw_cond.operator, SmartOperator::Is);
        assert_eq!(bw_cond.value, SmartRuleValue::Boolean { value: true });

        let is_false = make_arg_rule(
            0x1D,
            0x0000_0001,
            crate::sections::SmartPlaylistRuleArguments {
                arg_0: 0,
                ..Default::default()
            },
        );
        let SmartRuleNode::Condition(false_cond) = decode_smart_rule(&is_false) else {
            panic!("expected SmartRuleNode::Condition");
        };
        assert_eq!(false_cond.value, SmartRuleValue::Boolean { value: false });
    }

    #[test]
    fn playlist_cloud_synced_uses_offset_356_when_offset_324_is_zero() {
        let header = PlaylistItemHeader {
            header_size: 368,
            date_suggestion_flag_modified: Some(AppleTimestamp::new(0)),
            date_cloud_synced_2: Some(AppleTimestamp::new(3_786_912_000)),
            ..Default::default()
        };
        let pl = Playlist::from_header(&header);
        assert_eq!(pl.date_suggestion_flag_modified, None);
        assert_eq!(
            pl.date_cloud_synced.map(|t| t.to_string()),
            Some("2024-01-01T00:00:00Z".to_owned())
        );

        let distinct = PlaylistItemHeader {
            header_size: 368,
            date_suggestion_flag_modified: Some(AppleTimestamp::new(3_786_912_000)),
            date_cloud_synced_2: Some(AppleTimestamp::new(3_786_998_400)),
            ..Default::default()
        };
        let pl2 = Playlist::from_header(&distinct);
        assert_eq!(
            pl2.date_suggestion_flag_modified.map(|t| t.to_string()),
            Some("2024-01-01T00:00:00Z".to_owned())
        );
        assert_eq!(
            pl2.date_cloud_synced.map(|t| t.to_string()),
            Some("2024-01-02T00:00:00Z".to_owned())
        );
    }

    #[test]
    fn smart_rule_group_negation_is_preserved() {
        let nested_leaf = make_arg_rule(
            0x19,        // Rating
            0x0000_0010, // GreaterThan
            crate::sections::SmartPlaylistRuleArguments {
                arg_0: 60,
                ..Default::default()
            },
        );
        let nested_slst = SlstSection {
            rule_count: 1,
            conjunction: 1, // Any
            rules: vec![nested_leaf],
            ..Default::default()
        };
        let outer = SlstSection {
            rule_count: 1,
            conjunction: 0, // All
            rules: vec![SmartPlaylistRule {
                field_id: 0,
                comparison_method: SmartOperatorFlags::NEGATE.bits(),
                is_nested_rule_group: 1,
                unk_12_44: crate::sections::Unknown::new([0; 32]),
                unk_44: crate::sections::Unknown::new(0),
                unk_48: crate::sections::Unknown::new(0),
                child_size: 0,
                child: SmartRuleChild::Nested(Box::new(nested_slst)),
            }],
            ..Default::default()
        };

        let root = decode_slst_group(&outer, false);
        assert!(!root.negated);
        assert_eq!(root.conjunction, SmartConjunction::All);
        assert_eq!(root.rules.len(), 1);
        let SmartRuleNode::Group(sub) = &root.rules[0] else {
            panic!("expected nested SmartRuleNode::Group");
        };
        assert!(sub.negated);
        assert_eq!(sub.conjunction, SmartConjunction::Any);
    }

    #[test]
    fn dual_track_numerics_merge_is_order_independent_and_gates_downloaded_traits() {
        let local_num = TrackNumerics {
            subrecord_kind: 1,
            duration_ms: 273_000,
            file_size: 9_500_000,
            bit_rate: 256,
            has_embedded_artwork: -1,
            downloaded_audio_traits: u32::from(AudioTraits::LOSSLESS.bits()),
            ..Default::default()
        };
        let cloud_num = TrackNumerics {
            subrecord_kind: 3,
            duration_ms: 273_000,
            file_size: 0,
            bit_rate: 0,
            has_embedded_artwork: 1,
            is_apple_music_catalog: 1,
            catalog_lyrics_state: 1,
            codec_fourcc: 0x6D70_3461, // "mp4a"
            store_track_adam_id: 1_440_821_120,
            // Non-zero value on sub3 must NOT leak into downloaded_audio_traits
            downloaded_audio_traits: u32::from(AudioTraits::DOLBY_ATMOS.bits()),
            ..Default::default()
        };
        let make_boma = |n: TrackNumerics| BomaSection {
            header_size: 20,
            total_size: 20 + 364,
            subtype: 0x0001,
            unk_16_end: crate::sections::Unknown::new(crate::sections::TailBytes::from([0u8; 4])),
            payload: BomaPayload::TrackNumerics(Box::new(n)),
        };

        let item_1_3 = TrackItem {
            header: TrackItemHeader::default(),
            bomas: vec![make_boma(local_num.clone()), make_boma(cloud_num.clone())],
        };
        let item_3_1 = TrackItem {
            header: TrackItemHeader::default(),
            bomas: vec![make_boma(cloud_num.clone()), make_boma(local_num)],
        };

        let t_1_3 = Track::from_raw_with_tz(&item_1_3, 0);
        let t_3_1 = Track::from_raw_with_tz(&item_3_1, 0);
        let t_owned_3_1 = Track::from_owned_with_tz(item_3_1, 0);
        assert_eq!(t_1_3, t_3_1);
        assert_eq!(t_1_3, t_owned_3_1);
        assert_eq!(t_1_3.file_size, Some(9_500_000));
        assert_eq!(t_1_3.bit_rate, Some(256));
        assert_eq!(t_1_3.store_track_adam_id, Some(1_440_821_120));
        assert_eq!(t_1_3.codec_fourcc.as_deref(), Some("mp4a"));
        assert_eq!(t_1_3.embedded_artwork, Some(EmbeddedArtworkKind::Embedded));
        assert!(t_1_3.has_embedded_artwork());
        assert_eq!(t_1_3.downloaded_audio_traits, AudioTraits::LOSSLESS);

        // Cloud-only track (sub3 only) must NOT populate downloaded_audio_traits
        let item_cloud_only = TrackItem {
            header: TrackItemHeader::default(),
            bomas: vec![make_boma(cloud_num)],
        };
        let t_cloud = Track::from_raw_with_tz(&item_cloud_only, 0);
        assert_eq!(t_cloud.downloaded_audio_traits, AudioTraits::empty());
        assert_eq!(t_cloud.embedded_artwork, Some(EmbeddedArtworkKind::Cloud));
        assert!(!t_cloud.has_embedded_artwork());
    }

    #[test]
    fn embedded_artwork_and_media_kind_conversions() {
        assert_eq!(EmbeddedArtworkKind::from(0), EmbeddedArtworkKind::None);
        assert_eq!(EmbeddedArtworkKind::from(-1), EmbeddedArtworkKind::Embedded);
        assert_eq!(EmbeddedArtworkKind::from(1), EmbeddedArtworkKind::Cloud);
        assert_eq!(
            EmbeddedArtworkKind::from(128),
            EmbeddedArtworkKind::VideoPosterFrame
        );
        assert_eq!(
            EmbeddedArtworkKind::from(42),
            EmbeddedArtworkKind::Unknown(42)
        );
        assert_eq!(EmbeddedArtworkKind::VideoPosterFrame.raw(), 128);
        assert!(EmbeddedArtworkKind::VideoPosterFrame.has_local_artwork());
        assert!(EmbeddedArtworkKind::Embedded.has_local_artwork());
        assert!(!EmbeddedArtworkKind::Cloud.has_local_artwork());
        assert_eq!(
            EmbeddedArtworkKind::VideoPosterFrame.to_string(),
            "VideoPosterFrame"
        );

        assert_eq!(MediaKind::from(0x05), MediaKind::MusicOrPodcast);
        assert_eq!(MediaKind::from(0x05).raw(), 0x05);
        assert_eq!(MediaKind::MusicOrPodcast.to_string(), "MusicOrPodcast");
        assert_eq!(MediaKind::from(0x9999), MediaKind::Unknown(0x9999));
        assert_eq!(MediaKind::Unknown(0x9999).to_string(), "Unknown(39321)");
    }

    #[test]
    fn video_sort_show_social_artwork_and_playlist_lookup() {
        let vi = VideoInfo {
            width: 1280,
            height: 720,
            codec_fourcc: 0x6176_6331, // "avc1"
            video_bit_rate_kbps: 2_400,
            avc_profile_idc: 100,
            avc_level_idc: 31,
            frame_rate_milli: 29_970,
            color_bit_depth: 24,
            ..Default::default()
        };
        let track_item = TrackItem {
            header: TrackItemHeader::default(),
            bomas: vec![
                BomaSection {
                    header_size: 20,
                    total_size: 20 + 84,
                    subtype: 0x0024,
                    unk_16_end: crate::sections::Unknown::new(crate::sections::TailBytes::from(
                        [0u8; 4],
                    )),
                    payload: BomaPayload::VideoInfo(Box::new(vi)),
                },
                BomaSection {
                    header_size: 20,
                    total_size: 20,
                    subtype: 0x0053,
                    unk_16_end: crate::sections::Unknown::new(crate::sections::TailBytes::from(
                        [0u8; 4],
                    )),
                    payload: BomaPayload::RawStringUtf8 {
                        text: "Rush In Rio".to_owned(),
                        raw_bytes: b"Rush In Rio".to_vec(),
                    },
                },
            ],
        };
        let track = Track::from_raw_with_tz(&track_item, 0);
        assert_eq!(track.sort_show.as_deref(), Some("Rush In Rio"));
        let decoded_vi = track.video_info.as_ref().expect("video_info");
        assert_eq!((decoded_vi.width, decoded_vi.height), (1280, 720));
        assert_eq!(decoded_vi.codec_fourcc.as_deref(), Some("avc1"));
        assert!((decoded_vi.frame_rate_fps() - 29.97).abs() < 1e-6);

        // Exact + case-insensitive exact only (no substring match)
        let mut lib = Library::default();
        lib.playlists.push(Playlist {
            persistent_id: PersistentId::new(1),
            name: Some("Rush: Studio".to_owned()),
            ..Playlist::default()
        });
        assert!(lib.playlist_by_name("Rush: Studio").is_some());
        assert!(lib.playlist_by_name("rush: studio").is_some());
        assert!(lib.playlist_by_name("Studio").is_none());

        // SocialProfile::artwork_uuid
        let sp_item = SocialProfileItem {
            header: crate::sections::SocialProfileItemHeader {
                profile_persistent_id: 0xCAFE_BABE,
                artwork_uuid: [0xAB; 16],
                ..Default::default()
            },
            bomas: Vec::new(),
        };
        let sp_ref = SocialProfile::from(&sp_item);
        let sp_own = SocialProfile::from_owned(sp_item);
        assert_eq!(sp_ref, sp_own);
        assert_eq!(
            sp_ref.artwork_uuid.as_deref(),
            Some("ABABABAB-ABAB-ABAB-ABAB-ABABABABABAB")
        );
    }

    #[test]
    fn pre_1904_wrapped_date_released_unwraps_when_year_matches() {
        let make_track = |year: u32, raw_date_released: u32| -> Track {
            let num = TrackNumerics {
                subrecord_kind: 3,
                date_released: AppleTimestamp::new(raw_date_released),
                ..Default::default()
            };
            let item = TrackItem {
                header: TrackItemHeader {
                    year,
                    ..Default::default()
                },
                bomas: vec![BomaSection {
                    header_size: 20,
                    total_size: 20 + 364,
                    subtype: 0x0001,
                    unk_16_end: crate::sections::Unknown::new(crate::sections::TailBytes::from(
                        [0u8; 4],
                    )),
                    payload: BomaPayload::TrackNumerics(Box::new(num)),
                }],
            };
            Track::from_raw_with_tz(&item, 0)
        };

        // Wrapped 1900-01-01T12:00:00Z (raw u32 = 4_168_866_496 -> unsigned 2036-02-07T18:28:16Z).
        let wrapped_1900 = make_track(1900, 4_168_866_496);
        assert_eq!(
            wrapped_1900.date_released.map(|ts| ts.to_string()),
            Some("1900-01-01T12:00:00Z".to_owned())
        );

        // Genuine 2036 track with the exact same raw timestamp must NOT be unwrapped.
        let future_2036 = make_track(2036, 4_168_866_496);
        assert_eq!(
            future_2036.date_released.map(|ts| ts.to_string()),
            Some("2036-02-07T18:28:16Z".to_owned())
        );

        // Pre-1904 year whose raw timestamp does NOT unwrap to that year must stay unsigned.
        let mismatch = make_track(1900, 3_000_000_000);
        assert_eq!(
            mismatch.date_released,
            AppleTimestamp::new(3_000_000_000).to_utc()
        );
    }
}
