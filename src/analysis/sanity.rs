//! Section-first sanity audit for parsed [`RawLibraryFile`] structures.
//!
//! Audits raw `musicdb::sections` fields before [`musicdb::Library`] normalizes,
//! collapses, or silently drops anomalies, using `musicdb::model` enums and
//! bitflags purely as read-only validation oracles.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use jiff::Timestamp;
use serde::Serialize;

use musicdb::sections::{
    AlbumItem, AppleTimestamp, ArtistItem, BomaPayload, BomaSection, CloudDeletionItem, HfmaHeader,
    HsmaChild, LibraryMasterSection, PlaylistItem, RawLibraryFile, SectionKind, SlstSection,
    SmartPlaylistOptions, SmartPlaylistRule, SmartRuleChild, SocialProfileItem, TrackItem,
    TrackNumerics, TrackPlaysSkips, VideoInfo,
};
use musicdb::{
    AudioTraits, CatalogAlbumType, CloudDeletionKind, CloudStatus, ContentRating,
    DistinguishedPlaylistKind, DownloadedStatus, EqualizerPreset, MediaKind, MediaKindFlags,
    PersistentId, PlaylistShareMode, SmartField, SmartLimitUnit, SmartOperator, SmartOperatorFlags,
    SmartSelectionMethod, SortMode, SortModeFlags, SuggestionFlag, SuggestionFlags,
};

const APPLE_SENTINEL_CUTOFF: u32 = 86_400;
const APPLE_UNIX_EPOCH_RAW: u32 = 2_082_844_800;
const ITUNES_ERA_START_UNIX: i64 = 946_684_800; // 2000-01-01T00:00:00Z
const ONE_DAY_SECS: i64 = 86_400;
const TEN_YEARS_SECS: i64 = 10 * 365 * 86_400;
const DEFAULT_MAX_EXAMPLES: usize = 5;

/// Broad category for a sanity-check finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SanityCategory {
    #[value(name = "enum", alias = "enum_fallthrough")]
    EnumFallthrough,
    #[value(name = "bitflag")]
    Bitflag,
    #[value(name = "timestamp")]
    Timestamp,
    #[value(name = "string", alias = "string_data")]
    StringData,
    #[value(name = "bounds")]
    Bounds,
}

impl fmt::Display for SanityCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnumFallthrough => f.write_str("enum_fallthrough"),
            Self::Bitflag => f.write_str("bitflag"),
            Self::Timestamp => f.write_str("timestamp"),
            Self::StringData => f.write_str("string_data"),
            Self::Bounds => f.write_str("bounds"),
        }
    }
}

/// Severity of a sanity-check finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SanitySeverity {
    Warning,
    Info,
}

impl fmt::Display for SanitySeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Warning => f.write_str("WARN"),
            Self::Info => f.write_str("INFO"),
        }
    }
}

/// Configuration for [`analyze_sanity`].
#[derive(Debug, Clone)]
pub(crate) struct SanityCheckOptions {
    pub(crate) include_info: bool,
    pub(crate) reference_now: Timestamp,
    pub(crate) max_examples: usize,
    pub(crate) categories: Vec<SanityCategory>,
}

impl Default for SanityCheckOptions {
    fn default() -> Self {
        Self {
            include_info: false,
            reference_now: current_utc_timestamp(),
            max_examples: DEFAULT_MAX_EXAMPLES,
            categories: Vec::new(),
        }
    }
}

/// Complete report produced by [`analyze_sanity`].
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SanityReport {
    pub(crate) reference_now: String,
    pub(crate) hfma_date_modified: Option<String>,
    pub(crate) timezone_offset_seconds: i32,
    pub(crate) total_warnings: usize,
    pub(crate) total_info: usize,
    pub(crate) findings: Vec<SanityFindingGroup>,
}

/// Aggregated findings for one `(section, offset, field_name, rule_id)` rule.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SanityFindingGroup {
    pub(crate) category: SanityCategory,
    pub(crate) severity: SanitySeverity,
    pub(crate) rule_id: &'static str,
    pub(crate) section: String,
    pub(crate) field_name: String,
    pub(crate) offset_info: String,
    pub(crate) message: String,
    pub(crate) total_occurrences: usize,
    pub(crate) distinct_values: usize,
    pub(crate) buckets: Vec<SanityValueBucket>,
}

/// Distinct offending value bucket within a [`SanityFindingGroup`].
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SanityValueBucket {
    pub(crate) value: String,
    pub(crate) count: usize,
    pub(crate) shown: usize,
    pub(crate) examples: Vec<SanitySectionExample>,
}

/// Single section item occurrence illustrating an offending value.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SanitySectionExample {
    pub(crate) item_context: String,
    pub(crate) subcontext: Option<String>,
}

impl fmt::Display for SanityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== MusicDB Sanity Check ===")?;
        writeln!(f, "Reference UTC   : {}", self.reference_now)?;
        if let Some(modified) = &self.hfma_date_modified {
            writeln!(f, "Library Modified: {modified}")?;
        }
        writeln!(f, "Timezone Offset : {}s", self.timezone_offset_seconds)?;
        writeln!(
            f,
            "Summary         : {} warnings, {} info observations",
            self.total_warnings, self.total_info
        )?;
        if self.findings.is_empty() {
            writeln!(f, "\nNo findings reported.")?;
            return Ok(());
        }
        for group in &self.findings {
            writeln!(
                f,
                "\n[{}] {} ({}) :: {}+{} ({}) — {} ({} occurrences, {} distinct)",
                group.severity,
                group.rule_id,
                group.category,
                group.section,
                group.offset_info,
                group.field_name,
                group.message,
                group.total_occurrences,
                group.distinct_values
            )?;
            for bucket in &group.buckets {
                writeln!(f, "  value={} (count={})", bucket.value, bucket.count)?;
                for ex in &bucket.examples {
                    if let Some(sub) = &ex.subcontext {
                        writeln!(f, "    - {} [{sub}]", ex.item_context)?;
                    } else {
                        writeln!(f, "    - {}", ex.item_context)?;
                    }
                }
                if bucket.count > bucket.shown {
                    writeln!(f, "    ... +{} more", bucket.count - bucket.shown)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum TimestampEpoch {
    Utc,
    Local,
}

#[derive(Debug, Clone, Copy)]
struct TimestampSpec {
    epoch: TimestampEpoch,
    allow_known_sentinel_2: bool,
    allow_pre_1970: bool,
    allow_future: bool,
}

impl TimestampSpec {
    const UTC: Self = Self {
        epoch: TimestampEpoch::Utc,
        allow_known_sentinel_2: false,
        allow_pre_1970: false,
        allow_future: false,
    };
    const UTC_CLOUD_SYNC: Self = Self {
        allow_known_sentinel_2: true,
        ..Self::UTC
    };
    const UTC_LEASE: Self = Self {
        allow_future: true,
        ..Self::UTC
    };
    const UTC_RELEASE: Self = Self {
        allow_pre_1970: true,
        allow_future: true,
        ..Self::UTC
    };
    const LOCAL: Self = Self {
        epoch: TimestampEpoch::Local,
        ..Self::UTC
    };
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    severity: SanitySeverity,
    category: SanityCategory,
    section: String,
    offset_info: String,
    field_name: String,
    rule_id: &'static str,
}

struct GroupAccum {
    message: String,
    total_occurrences: usize,
    buckets: BTreeMap<String, BucketAccum>,
}

struct BucketAccum {
    count: usize,
    examples: Vec<SanitySectionExample>,
}

struct Finding<'a> {
    category: SanityCategory,
    severity: SanitySeverity,
    rule_id: &'static str,
    section: &'a str,
    offset: &'a str,
    field: &'a str,
    message: &'a str,
    value: String,
    item_context: &'a str,
    subcontext: Option<String>,
}

struct SanityCollector {
    include_info: bool,
    max_examples: usize,
    categories: Vec<SanityCategory>,
    now_secs: i64,
    tz_offset_secs: i32,
    total_warnings: usize,
    total_info: usize,
    groups: BTreeMap<GroupKey, GroupAccum>,
}

impl SanityCollector {
    fn new(opts: &SanityCheckOptions, tz_offset_secs: i32) -> Self {
        Self {
            include_info: opts.include_info,
            max_examples: opts.max_examples,
            categories: opts.categories.clone(),
            now_secs: opts.reference_now.as_second(),
            tz_offset_secs,
            total_warnings: 0,
            total_info: 0,
            groups: BTreeMap::new(),
        }
    }

    fn record(&mut self, finding: Finding<'_>) {
        if !self.categories.is_empty() && !self.categories.contains(&finding.category) {
            return;
        }
        match finding.severity {
            SanitySeverity::Warning => self.total_warnings += 1,
            SanitySeverity::Info => {
                self.total_info += 1;
                if !self.include_info {
                    return;
                }
            }
        }
        let key = GroupKey {
            severity: finding.severity,
            category: finding.category,
            section: finding.section.to_string(),
            offset_info: finding.offset.to_string(),
            field_name: finding.field.to_string(),
            rule_id: finding.rule_id,
        };
        let group = self.groups.entry(key).or_insert_with(|| GroupAccum {
            message: finding.message.to_string(),
            total_occurrences: 0,
            buckets: BTreeMap::new(),
        });
        group.total_occurrences += 1;
        let bucket = group
            .buckets
            .entry(finding.value)
            .or_insert_with(|| BucketAccum {
                count: 0,
                examples: Vec::new(),
            });
        bucket.count += 1;
        if bucket.examples.len() < self.max_examples {
            bucket.examples.push(SanitySectionExample {
                item_context: finding.item_context.to_string(),
                subcontext: finding.subcontext,
            });
        }
    }

    fn check_bool(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        raw: u32,
        ctx: &str,
        sub: Option<String>,
    ) {
        if raw > 1 {
            self.record(Finding {
                category: SanityCategory::Bitflag,
                severity: SanitySeverity::Warning,
                rule_id: "bool.non_binary",
                section,
                offset,
                field,
                message: "raw boolean field holds value outside {0, 1}",
                value: raw.to_string(),
                item_context: ctx,
                subcontext: sub,
            });
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared bitflag predicate needs section, offset, field, raw/known masks, and item context"
    )]
    fn check_unknown_bits(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        raw: u32,
        known_mask: u32,
        ctx: &str,
        sub: Option<String>,
    ) {
        let unknown = raw & !known_mask;
        if unknown != 0 {
            self.record(Finding {
                category: SanityCategory::Bitflag,
                severity: SanitySeverity::Warning,
                rule_id: "bitflag.unknown_bits",
                section,
                offset,
                field,
                message: "bitflag field has undocumented bits set",
                value: format!("{raw:#x} (unknown={unknown:#x})"),
                item_context: ctx,
                subcontext: sub,
            });
        }
    }

    fn check_enum_fallthrough(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        value: String,
        ctx: &str,
        sub: Option<String>,
    ) {
        self.record(Finding {
            category: SanityCategory::EnumFallthrough,
            severity: SanitySeverity::Warning,
            rule_id: "enum.fallthrough",
            section,
            offset,
            field,
            message: "raw value fell through to Unknown/Other variant",
            value,
            item_context: ctx,
            subcontext: sub,
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared timestamp sentinel predicate needs section, offset, field, raw value, policy spec, and item context"
    )]
    fn check_timestamp_sentinel(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        raw: u32,
        spec: TimestampSpec,
        ctx: &str,
        sub: Option<String>,
    ) -> bool {
        if raw < APPLE_SENTINEL_CUTOFF {
            let (severity, rule_id, message) = if raw == 2 && spec.allow_known_sentinel_2 {
                (
                    SanitySeverity::Info,
                    "timestamp.known_sentinel",
                    "known local-only sentinel timestamp value 2",
                )
            } else {
                (
                    SanitySeverity::Warning,
                    "timestamp.unknown_sentinel",
                    "non-zero AppleTimestamp below 86_400 sentinel cutoff",
                )
            };
            self.record(Finding {
                category: SanityCategory::Timestamp,
                severity,
                rule_id,
                section,
                offset,
                field,
                message,
                value: raw.to_string(),
                item_context: ctx,
                subcontext: sub,
            });
            return true;
        }
        if raw > u32::MAX - APPLE_SENTINEL_CUTOFF {
            let signed = i64::from(raw) - (1i64 << 32);
            self.record(Finding {
                category: SanityCategory::Timestamp,
                severity: SanitySeverity::Warning,
                rule_id: "timestamp.underflow_sentinel",
                section,
                offset,
                field,
                message: "AppleTimestamp wrapped within 24h below 0 (e.g. DST underflow or u32::MAX)",
                value: format!("{raw} (signed={signed})"),
                item_context: ctx,
                subcontext: sub,
            });
            return true;
        }
        false
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared timestamp predicate needs section, offset, field, value, policy spec, and item context"
    )]
    fn check_timestamp(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        ts: AppleTimestamp,
        spec: TimestampSpec,
        ctx: &str,
        sub: Option<String>,
    ) {
        let raw = ts.raw();
        if raw == 0 {
            return;
        }
        if self.check_timestamp_sentinel(section, offset, field, raw, spec, ctx, sub.clone()) {
            return;
        }
        if raw < APPLE_UNIX_EPOCH_RAW {
            if !spec.allow_pre_1970 {
                self.record(Finding {
                    category: SanityCategory::Timestamp,
                    severity: SanitySeverity::Warning,
                    rule_id: "timestamp.before_unix_epoch",
                    section,
                    offset,
                    field,
                    message: "AppleTimestamp precedes 1970-01-01 Unix epoch",
                    value: format!("{ts} (raw={raw})"),
                    item_context: ctx,
                    subcontext: sub,
                });
            }
            return;
        }
        let converted = match spec.epoch {
            TimestampEpoch::Utc => ts.to_utc(),
            TimestampEpoch::Local => ts.to_utc_with_offset(self.tz_offset_secs),
        };
        let Some(utc) = converted else {
            return;
        };
        self.check_timestamp_utc_bounds(section, offset, field, raw, utc, spec, ctx, sub);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared timestamp bounds helper needs section, offset, field, raw/utc values, policy spec, and item context"
    )]
    fn check_timestamp_utc_bounds(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        raw: u32,
        utc: Timestamp,
        spec: TimestampSpec,
        ctx: &str,
        sub: Option<String>,
    ) {
        let secs = utc.as_second();
        if !spec.allow_pre_1970 && secs < ITUNES_ERA_START_UNIX {
            self.record(Finding {
                category: SanityCategory::Timestamp,
                severity: SanitySeverity::Warning,
                rule_id: "timestamp.before_itunes_era",
                section,
                offset,
                field,
                message: "timestamp precedes 2000-01-01 iTunes era",
                value: format!("{utc} (raw={raw})"),
                item_context: ctx,
                subcontext: sub,
            });
            return;
        }
        let (severity, rule_id, message) = if spec.allow_future {
            if raw == u32::MAX || secs > self.now_secs.saturating_add(TEN_YEARS_SECS) {
                (
                    SanitySeverity::Warning,
                    "timestamp.far_future",
                    "timestamp is more than 10 years in the future",
                )
            } else if secs > self.now_secs {
                (
                    SanitySeverity::Info,
                    "timestamp.future_expected",
                    "future timestamp on field where future dates are expected",
                )
            } else {
                return;
            }
        } else if raw == u32::MAX || secs > self.now_secs.saturating_add(ONE_DAY_SECS) {
            (
                SanitySeverity::Warning,
                "timestamp.future_unexpected",
                "timestamp is unexpectedly in the future",
            )
        } else {
            return;
        };
        self.record(Finding {
            category: SanityCategory::Timestamp,
            severity,
            rule_id,
            section,
            offset,
            field,
            message,
            value: format!("{utc} (raw={raw})"),
            item_context: ctx,
            subcontext: sub,
        });
    }

    fn check_fourcc(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        raw: u32,
        ctx: &str,
        sub: Option<String>,
    ) {
        if raw == 0 {
            return;
        }
        let bytes = raw.to_be_bytes();
        if !bytes.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
            self.record(Finding {
                category: SanityCategory::StringData,
                severity: SanitySeverity::Warning,
                rule_id: "string.invalid_fourcc",
                section,
                offset,
                field,
                message: "non-zero FourCC contains non-printable ASCII bytes",
                value: format!("{raw:#010x}"),
                item_context: ctx,
                subcontext: sub,
            });
        }
    }

    fn check_star_rating(&mut self, section: &str, offset: &str, raw: u8, ctx: &str) {
        if raw > 100 || !raw.is_multiple_of(20) {
            self.record(Finding {
                category: SanityCategory::Bounds,
                severity: SanitySeverity::Warning,
                rule_id: "bounds.star_rating",
                section,
                offset,
                field: "star_rating",
                message: "star_rating must be <= 100 and a multiple of 20",
                value: raw.to_string(),
                item_context: ctx,
                subcontext: None,
            });
        }
    }

    fn check_count_vs_true(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        count: u32,
        true_count: u32,
        ctx: &str,
    ) {
        if count > true_count {
            self.record(Finding {
                category: SanityCategory::Bounds,
                severity: SanitySeverity::Info,
                rule_id: "bounds.count_exceeds_true",
                section,
                offset,
                field,
                message: "displayed count exceeds local/true count (common in iCloud multi-device libraries)",
                value: format!("{count} > {true_count}"),
                item_context: ctx,
                subcontext: None,
            });
        }
    }

    fn check_section_count(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        declared: u32,
        actual: usize,
        ctx: &str,
    ) {
        if usize::try_from(declared).ok() != Some(actual) {
            self.record(Finding {
                category: SanityCategory::Bounds,
                severity: SanitySeverity::Warning,
                rule_id: "bounds.section_count_mismatch",
                section,
                offset,
                field,
                message: "header count does not match parsed child count",
                value: format!("header={declared}, actual={actual}"),
                item_context: ctx,
                subcontext: None,
            });
        }
    }

    fn finish(self, opts: &SanityCheckOptions, hfma: &HfmaHeader) -> SanityReport {
        let findings = self
            .groups
            .into_iter()
            .map(|(key, acc)| {
                let distinct_values = acc.buckets.len();
                let mut buckets: Vec<SanityValueBucket> = acc
                    .buckets
                    .into_iter()
                    .map(|(value, b)| SanityValueBucket {
                        value,
                        count: b.count,
                        shown: b.examples.len(),
                        examples: b.examples,
                    })
                    .collect();
                buckets.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
                SanityFindingGroup {
                    category: key.category,
                    severity: key.severity,
                    rule_id: key.rule_id,
                    section: key.section,
                    field_name: key.field_name,
                    offset_info: key.offset_info,
                    message: acc.message,
                    total_occurrences: acc.total_occurrences,
                    distinct_values,
                    buckets,
                }
            })
            .collect();
        SanityReport {
            reference_now: opts.reference_now.to_string(),
            hfma_date_modified: hfma.date_modified.to_utc().map(|t| t.to_string()),
            timezone_offset_seconds: hfma.timezone_offset_seconds,
            total_warnings: self.total_warnings,
            total_info: self.total_info,
            findings,
        }
    }
}

/// Runs Tier 1 and Tier 2 sanity checks directly on a parsed [`RawLibraryFile`].
pub(crate) fn analyze_sanity(raw: &RawLibraryFile, opts: &SanityCheckOptions) -> SanityReport {
    let mut c = SanityCollector::new(opts, raw.outer_header.timezone_offset_seconds);
    check_hfma(&mut c, "hfma", &raw.outer_header);
    check_hfma_section_counts(&mut c, raw);

    for (idx, sec) in raw.sections.iter().enumerate() {
        let hsma_ctx = format!("hsma[{idx}]");
        if matches!(sec.header.kind(), SectionKind::Unknown(_)) {
            c.check_enum_fallthrough(
                "hsma",
                "12..16",
                "subtype",
                sec.header.subtype.to_string(),
                &hsma_ctx,
                None,
            );
        }
        match &sec.child {
            HsmaChild::InnerHfma(h) => check_hfma(&mut c, "inner_hfma", h),
            HsmaChild::LibraryMaster(plma) => check_plma(&mut c, plma),
            HsmaChild::AlbumList(list) => {
                for album in &list.albums {
                    check_album(&mut c, album);
                }
            }
            HsmaChild::ArtistList(list) => {
                for artist in &list.artists {
                    check_artist(&mut c, artist);
                }
            }
            HsmaChild::TrackList(list) => {
                for track in &list.tracks {
                    check_track(&mut c, track);
                }
            }
            HsmaChild::PlaylistList(list) => {
                for playlist in &list.playlists {
                    check_playlist(&mut c, playlist);
                }
            }
            HsmaChild::PendingCloudDeletions(list) => {
                for item in &list.items {
                    check_cloud_deletion(&mut c, item);
                }
            }
            HsmaChild::SocialProfileList(list) => {
                for profile in &list.profiles {
                    check_social_profile(&mut c, profile);
                }
            }
            HsmaChild::Unknown {
                kind, signature, ..
            } => {
                if !matches!(kind, SectionKind::Unknown(_) | SectionKind::Trailer) {
                    let sig = String::from_utf8_lossy(signature);
                    c.check_enum_fallthrough(
                        "hsma",
                        "12..16",
                        "child",
                        format!("{kind} (sig={sig:?})"),
                        &hsma_ctx,
                        None,
                    );
                }
            }
            HsmaChild::Padding(_) | HsmaChild::Skipped { .. } | _ => {}
        }
    }

    c.finish(opts, &raw.outer_header)
}

fn check_hfma_section_counts(c: &mut SanityCollector, raw: &RawLibraryFile) {
    let h = &raw.outer_header;
    let ctx = format!("hfma[{}]", PersistentId(h.library_persistent_id));
    c.check_section_count(
        "hfma",
        "56..60",
        "hsma_count",
        h.hsma_count,
        raw.sections.len(),
        &ctx,
    );

    let mut list_tracks = 0usize;
    let mut list_albums = 0usize;
    let mut list_artists = 0usize;
    let mut list_playlists = 0usize;
    for sec in &raw.sections {
        match &sec.child {
            HsmaChild::TrackList(l) => {
                list_tracks += usize::try_from(l.header.track_count).unwrap_or(l.tracks.len());
            }
            HsmaChild::AlbumList(l) => {
                list_albums += usize::try_from(l.header.album_count).unwrap_or(l.albums.len());
            }
            HsmaChild::ArtistList(l) => {
                list_artists += usize::try_from(l.header.artist_count).unwrap_or(l.artists.len());
            }
            HsmaChild::PlaylistList(l) => {
                list_playlists +=
                    usize::try_from(l.header.playlist_count).unwrap_or(l.playlists.len());
            }
            _ => {}
        }
    }
    c.check_section_count(
        "hfma",
        "68..72",
        "track_count",
        h.track_count,
        list_tracks,
        &ctx,
    );
    c.check_section_count(
        "hfma",
        "76..80",
        "album_count",
        h.album_count,
        list_albums,
        &ctx,
    );
    c.check_section_count(
        "hfma",
        "80..84",
        "artist_count",
        h.artist_count,
        list_artists,
        &ctx,
    );
    c.check_section_count(
        "hfma",
        "72..76",
        "playlist_count",
        h.playlist_count,
        list_playlists,
        &ctx,
    );
}

fn check_hfma(c: &mut SanityCollector, section: &str, h: &HfmaHeader) {
    let ctx = format!("{section}[{}]", PersistentId(h.library_persistent_id));
    c.check_timestamp(
        section,
        "100..104",
        "date_modified",
        h.date_modified,
        TimestampSpec::UTC,
        &ctx,
        None,
    );
    if let Some(nul_pos) = h.version_bytes.iter().position(|&b| b == 0)
        && h.version_bytes
            .get(nul_pos + 1..)
            .is_some_and(|tail| tail.iter().any(|&b| b != 0))
    {
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.embedded_nul",
            section,
            offset: "16..48",
            field: "version_bytes",
            message: "non-zero trailing bytes after NUL terminator in version_bytes",
            value: format!("{:?}", h.version_string()),
            item_context: &ctx,
            subcontext: None,
        });
    }
}

fn check_plma(c: &mut SanityCollector, plma: &LibraryMasterSection) {
    let h = &plma.header;
    let ctx = format!("plma[{}]", PersistentId(h.library_persistent_id));
    c.check_bool(
        "plma",
        "24",
        "checkbox_show_song_list_checkboxes",
        u32::from(h.checkbox_show_song_list_checkboxes),
        &ctx,
        None,
    );
    c.check_bool(
        "plma",
        "148",
        "checkbox_keep_media_folder_organized",
        u32::from(h.checkbox_keep_media_folder_organized),
        &ctx,
        None,
    );
    c.check_timestamp(
        "plma",
        "16..20",
        "date_cloud_synced",
        h.date_cloud_synced,
        TimestampSpec::UTC,
        &ctx,
        None,
    );
    c.check_timestamp(
        "plma",
        "20..24",
        "date_last_modified",
        h.date_last_modified,
        TimestampSpec::UTC,
        &ctx,
        None,
    );
    c.check_timestamp(
        "plma",
        "120..124",
        "date_cloud_lease_expires",
        h.date_cloud_lease_expires,
        TimestampSpec::UTC_LEASE,
        &ctx,
        None,
    );
    if let Some(ts) = h.date_local_lease_expires {
        c.check_timestamp(
            "plma",
            "182..186",
            "date_local_lease_expires",
            ts,
            TimestampSpec::UTC_LEASE,
            &ctx,
            None,
        );
    }
    check_bomas(c, "plma", &plma.bomas, &ctx);
}

fn check_album(c: &mut SanityCollector, album: &AlbumItem) {
    let h = &album.header;
    let ctx = format_item_context(
        "iama",
        h.album_persistent_id,
        find_boma_str(&album.bomas, 0x012C),
        find_boma_str(&album.bomas, 0x012D),
    );
    c.check_bool(
        "iama",
        "25",
        "is_compilation",
        u32::from(h.is_compilation),
        &ctx,
        None,
    );
    c.check_bool(
        "iama",
        "26",
        "playlist_only",
        u32::from(h.playlist_only),
        &ctx,
        None,
    );
    c.check_star_rating("iama", "40", h.star_rating, &ctx);
    check_suggestion_flag(c, "iama", "42..44", h.suggestion_flag, &ctx);
    if let Some(raw) = h.catalog_album_type
        && matches!(CatalogAlbumType::from(raw), CatalogAlbumType::Unknown(_))
    {
        c.check_enum_fallthrough(
            "iama",
            "136..140",
            "catalog_album_type",
            raw.to_string(),
            &ctx,
            None,
        );
    }
    c.check_timestamp(
        "iama",
        "96..100",
        "date_suggestion_flag_modified",
        h.date_suggestion_flag_modified,
        TimestampSpec::UTC,
        &ctx,
        None,
    );
    c.check_timestamp(
        "iama",
        "100..104",
        "date_last_played",
        h.date_last_played,
        TimestampSpec::LOCAL,
        &ctx,
        None,
    );
    if let Some(ts) = h.date_cloud_synced {
        c.check_timestamp(
            "iama",
            "120..124",
            "date_cloud_synced",
            ts,
            TimestampSpec::UTC_CLOUD_SYNC,
            &ctx,
            None,
        );
    }
    if let Some(ts) = h.date_released {
        c.check_timestamp(
            "iama",
            "124..128",
            "date_released",
            ts,
            TimestampSpec::UTC_RELEASE,
            &ctx,
            None,
        );
    }
    check_bomas(c, "iama", &album.bomas, &ctx);
}

fn check_artist(c: &mut SanityCollector, artist: &ArtistItem) {
    let h = &artist.header;
    let ctx = format_item_context(
        "iAma",
        h.artist_persistent_id,
        find_boma_str(&artist.bomas, 0x0190),
        None,
    );
    c.check_bool(
        "iAma",
        "25",
        "playlist_only",
        u32::from(h.playlist_only),
        &ctx,
        None,
    );
    check_suggestion_flag(c, "iAma", "101..103", h.suggestion_flag, &ctx);
    c.check_timestamp(
        "iAma",
        "60..64",
        "date_suggestion_flag_modified",
        h.date_suggestion_flag_modified,
        TimestampSpec::UTC,
        &ctx,
        None,
    );
    if let Some(ts) = h.date_cloud_synced {
        c.check_timestamp(
            "iAma",
            "120..124",
            "date_cloud_synced",
            ts,
            TimestampSpec::UTC_CLOUD_SYNC,
            &ctx,
            None,
        );
    }
    check_bomas(c, "iAma", &artist.bomas, &ctx);
}

fn check_track(c: &mut SanityCollector, track: &TrackItem) {
    let h = &track.header;
    let ctx = format_item_context(
        "itma",
        h.track_persistent_id,
        find_boma_str(&track.bomas, 0x0002),
        find_boma_str(&track.bomas, 0x0004),
    );
    check_track_booleans(c, track, &ctx);
    check_track_enums_and_flags(c, track, &ctx);
    check_track_bounds(c, track, &ctx);
    c.check_timestamp(
        "itma",
        "336..340",
        "date_suggestion_flag_modified",
        h.date_suggestion_flag_modified,
        TimestampSpec::UTC,
        &ctx,
        None,
    );
    c.check_timestamp(
        "itma",
        "352..356",
        "date_cloud_synced",
        h.date_cloud_synced,
        TimestampSpec::UTC_CLOUD_SYNC,
        &ctx,
        None,
    );
    check_bomas(c, "itma", &track.bomas, &ctx);
}

fn check_track_bounds(c: &mut SanityCollector, track: &TrackItem, ctx: &str) {
    let h = &track.header;
    c.check_star_rating("itma", "65", h.star_rating, ctx);
    if !(-255..=255).contains(&h.volume_adjustment) {
        c.record(Finding {
            category: SanityCategory::Bounds,
            severity: SanitySeverity::Warning,
            rule_id: "bounds.volume_adjustment",
            section: "itma",
            offset: "92..96",
            field: "volume_adjustment",
            message: "volume_adjustment is outside valid -255..=255 range",
            value: h.volume_adjustment.to_string(),
            item_context: ctx,
            subcontext: None,
        });
    }
    let pairs: [(&str, &str, u16, u16); 3] = [
        (
            "160..162,116..118",
            "track_number/track_total",
            h.track_number,
            h.track_total,
        ),
        (
            "84..86,90..92",
            "disc_number/disc_total",
            h.disc_number,
            h.disc_total,
        ),
        (
            "88..90,86..88",
            "movement_number/movement_total",
            h.movement_number,
            h.movement_total,
        ),
    ];
    for (offset, field, num, total) in pairs {
        if total > 0 && num > total {
            c.record(Finding {
                category: SanityCategory::Bounds,
                severity: SanitySeverity::Warning,
                rule_id: "bounds.number_exceeds_total",
                section: "itma",
                offset,
                field,
                message: "index exceeds declared total",
                value: format!("{num}/{total}"),
                item_context: ctx,
                subcontext: None,
            });
        }
    }
    if h.playback_stop_ms > 0 && h.playback_start_ms > h.playback_stop_ms {
        c.record(Finding {
            category: SanityCategory::Bounds,
            severity: SanitySeverity::Warning,
            rule_id: "bounds.playback_range",
            section: "itma",
            offset: "148..156",
            field: "playback_start_ms/playback_stop_ms",
            message: "playback_start_ms exceeds non-zero playback_stop_ms",
            value: format!("{} > {}", h.playback_start_ms, h.playback_stop_ms),
            item_context: ctx,
            subcontext: None,
        });
    }
}

fn check_track_booleans(c: &mut SanityCollector, track: &TrackItem, ctx: &str) {
    let h = &track.header;
    let fields: [(&str, &str, u8); 15] = [
        (
            "30",
            "checkbox_skip_when_shuffling",
            h.checkbox_skip_when_shuffling,
        ),
        ("31", "has_ever_been_played", h.has_ever_been_played),
        ("33", "is_cloud_synced", h.is_cloud_synced),
        (
            "38",
            "checkbox_album_is_compilation",
            h.checkbox_album_is_compilation,
        ),
        ("39", "playlist_only", h.playlist_only),
        ("42", "checkbox_disabled", h.checkbox_disabled),
        ("44", "has_local_custom_lyrics", h.has_local_custom_lyrics),
        (
            "46",
            "has_unsynced_local_changes",
            h.has_unsynced_local_changes,
        ),
        (
            "49",
            "checkbox_part_of_gapless_album",
            h.checkbox_part_of_gapless_album,
        ),
        (
            "50",
            "checkbox_remember_playback_position",
            h.checkbox_remember_playback_position,
        ),
        (
            "51",
            "checkbox_show_composer_in_all_views",
            h.checkbox_show_composer_in_all_views,
        ),
        (
            "52",
            "checkbox_use_work_and_movement",
            h.checkbox_use_work_and_movement,
        ),
        ("54", "has_asset_info_plist", h.has_asset_info_plist),
        (
            "55",
            "has_playback_or_bounds_activity",
            h.has_playback_or_bounds_activity,
        ),
        ("255", "has_custom_lyrics", h.has_custom_lyrics),
    ];
    for (offset, field, raw) in fields {
        c.check_bool("itma", offset, field, u32::from(raw), ctx, None);
    }
}

fn check_track_enums_and_flags(c: &mut SanityCollector, track: &TrackItem, ctx: &str) {
    let h = &track.header;
    if matches!(
        DownloadedStatus::from(h.downloaded_status),
        DownloadedStatus::Unknown(_)
    ) {
        c.check_enum_fallthrough(
            "itma",
            "57",
            "downloaded_status",
            h.downloaded_status.to_string(),
            ctx,
            None,
        );
    }
    if matches!(CloudStatus::from(h.cloud_status), CloudStatus::Unknown(_)) {
        c.check_enum_fallthrough(
            "itma",
            "58",
            "cloud_status",
            h.cloud_status.to_string(),
            ctx,
            None,
        );
    }
    if matches!(
        ContentRating::from(h.content_rating),
        ContentRating::Unknown(_)
    ) {
        c.check_enum_fallthrough(
            "itma",
            "59",
            "content_rating",
            h.content_rating.to_string(),
            ctx,
            None,
        );
    }
    check_suggestion_flag(c, "itma", "62..64", h.suggestion_flag, ctx);
    check_track_sort_and_media(c, track, ctx);
}

fn check_track_sort_and_media(c: &mut SanityCollector, track: &TrackItem, ctx: &str) {
    let h = &track.header;
    for (i, &raw) in h.auto_sort_flags.iter().enumerate() {
        let sub = Some(format!("auto_sort_flags[{i}]"));
        if matches!(SortMode::from(raw), SortMode::Unknown(_)) {
            c.check_enum_fallthrough(
                "itma",
                "66..72",
                "auto_sort_flags",
                format!("{raw:#04x}"),
                ctx,
                sub.clone(),
            );
        }
        c.check_unknown_bits(
            "itma",
            "66..72",
            "auto_sort_flags",
            u32::from(raw),
            u32::from(SortModeFlags::all().bits()),
            ctx,
            sub,
        );
    }
    if matches!(MediaKind::from(h.media_kind), MediaKind::Other(_)) {
        c.check_enum_fallthrough(
            "itma",
            "108..112",
            "media_kind",
            format!("{:#x}", h.media_kind),
            ctx,
            None,
        );
    }
    c.check_unknown_bits(
        "itma",
        "108..112",
        "media_kind",
        h.media_kind,
        MediaKindFlags::all().bits(),
        ctx,
        None,
    );
    c.check_unknown_bits(
        "itma",
        "331",
        "audio_traits",
        u32::from(h.audio_traits),
        u32::from(AudioTraits::all().bits()),
        ctx,
        None,
    );
}

fn check_track_numerics(
    c: &mut SanityCollector,
    n: &TrackNumerics,
    ctx: &str,
    sub: Option<String>,
) {
    let bool_fields: [(&str, &str, u32); 8] = [
        (
            "20",
            "is_cloud_unavailable",
            u32::from(n.is_cloud_unavailable),
        ),
        ("27", "is_vbr", u32::from(n.is_vbr)),
        ("28..32", "is_local_subrecord", n.is_local_subrecord),
        (
            "34",
            "is_apple_music_catalog",
            u32::from(n.is_apple_music_catalog),
        ),
        (
            "36",
            "is_apple_digital_master",
            u32::from(n.is_apple_digital_master),
        ),
        (
            "39",
            "is_available_in_catalog",
            u32::from(n.is_available_in_catalog),
        ),
        ("40", "is_local_mp3", u32::from(n.is_local_mp3)),
        ("41", "is_local_video", u32::from(n.is_local_video)),
    ];
    for (offset, field, raw) in bool_fields {
        c.check_bool("TrackNumerics", offset, field, raw, ctx, sub.clone());
    }
    if n.is_itunes_matched != [0, 0] && n.is_itunes_matched != [1, 1] {
        c.record(Finding {
            category: SanityCategory::Bitflag,
            severity: SanitySeverity::Warning,
            rule_id: "bool.non_binary",
            section: "TrackNumerics",
            offset: "32..34",
            field: "is_itunes_matched",
            message: "is_itunes_matched is neither [0, 0] nor [1, 1]",
            value: format!("{:?}", n.is_itunes_matched),
            item_context: ctx,
            subcontext: sub.clone(),
        });
    }
    c.check_unknown_bits(
        "TrackNumerics",
        "336..340",
        "downloaded_audio_traits",
        n.downloaded_audio_traits,
        u32::from(AudioTraits::all().bits()),
        ctx,
        sub.clone(),
    );
    c.check_fourcc(
        "TrackNumerics",
        "64..68",
        "file_type_fourcc",
        n.file_type_fourcc,
        ctx,
        sub.clone(),
    );
    c.check_fourcc(
        "TrackNumerics",
        "148..152",
        "codec_fourcc",
        n.codec_fourcc,
        ctx,
        sub.clone(),
    );
    check_track_numerics_sample_rate(c, n.sample_rate, ctx, sub.clone());
    check_track_numerics_timestamps(c, n, ctx, sub);
}

fn check_track_numerics_sample_rate(
    c: &mut SanityCollector,
    sample_rate: f32,
    ctx: &str,
    sub: Option<String>,
) {
    if !sample_rate.is_finite()
        || (sample_rate != 0.0 && !(1_000.0..=768_000.0).contains(&sample_rate))
    {
        c.record(Finding {
            category: SanityCategory::Bounds,
            severity: SanitySeverity::Warning,
            rule_id: "bounds.sample_rate",
            section: "TrackNumerics",
            offset: "60..64",
            field: "sample_rate",
            message: "non-zero sample_rate is non-finite or outside 1_000..=768_000 Hz",
            value: format!("{sample_rate}"),
            item_context: ctx,
            subcontext: sub,
        });
    }
}

fn check_track_numerics_timestamps(
    c: &mut SanityCollector,
    n: &TrackNumerics,
    ctx: &str,
    sub: Option<String>,
) {
    c.check_timestamp(
        "TrackNumerics",
        "92..96",
        "date_added",
        n.date_added,
        TimestampSpec::LOCAL,
        ctx,
        sub.clone(),
    );
    c.check_timestamp(
        "TrackNumerics",
        "128..132",
        "date_modified",
        n.date_modified,
        TimestampSpec::LOCAL,
        ctx,
        sub.clone(),
    );
    c.check_timestamp(
        "TrackNumerics",
        "136..140",
        "date_purchased",
        n.date_purchased,
        TimestampSpec::UTC,
        ctx,
        sub.clone(),
    );
    c.check_timestamp(
        "TrackNumerics",
        "140..144",
        "date_released",
        n.date_released,
        TimestampSpec::UTC_RELEASE,
        ctx,
        sub.clone(),
    );
    c.check_timestamp(
        "TrackNumerics",
        "356..360",
        "date_cloud_synced",
        n.date_cloud_synced,
        TimestampSpec::UTC_CLOUD_SYNC,
        ctx,
        sub,
    );
}

fn check_track_plays_skips(c: &mut SanityCollector, p: &TrackPlaysSkips, ctx: &str) {
    c.check_count_vs_true(
        "TrackPlaysSkips",
        "12..20",
        "play_count/true_play_count",
        p.play_count,
        p.true_play_count,
        ctx,
    );
    c.check_count_vs_true(
        "TrackPlaysSkips",
        "32..40",
        "skip_count/true_skip_count",
        p.skip_count,
        p.true_skip_count,
        ctx,
    );
    c.check_timestamp(
        "TrackPlaysSkips",
        "8..12",
        "date_last_played",
        p.date_last_played,
        TimestampSpec::LOCAL,
        ctx,
        None,
    );
    c.check_timestamp(
        "TrackPlaysSkips",
        "20..24",
        "date_first_played_utc",
        p.date_first_played_utc,
        TimestampSpec::UTC,
        ctx,
        None,
    );
    c.check_timestamp(
        "TrackPlaysSkips",
        "28..32",
        "date_last_skipped",
        p.date_last_skipped,
        TimestampSpec::LOCAL,
        ctx,
        None,
    );
}

fn check_video_info(c: &mut SanityCollector, v: &VideoInfo, ctx: &str) {
    c.check_fourcc(
        "VideoInfo",
        "12..16",
        "codec_fourcc",
        v.codec_fourcc,
        ctx,
        None,
    );
}

fn check_playlist(c: &mut SanityCollector, playlist: &PlaylistItem) {
    let h = &playlist.header;
    let ctx = format_item_context(
        "lpma",
        h.playlist_persistent_id,
        find_boma_str(&playlist.bomas, 0x00C8),
        None,
    );
    check_playlist_header_fields(c, playlist, &ctx);
    check_playlist_timestamps(c, playlist, &ctx);
    check_bomas(c, "lpma", &playlist.bomas, &ctx);
}

fn check_playlist_header_fields(c: &mut SanityCollector, playlist: &PlaylistItem, ctx: &str) {
    let h = &playlist.header;
    let bools: [(&str, &str, u8); 7] = [
        ("44", "is_master_playlist", h.is_master_playlist),
        ("49", "is_folder", h.is_folder),
        ("190", "ownership_flags[0]", h.ownership_flags[0]),
        ("191", "ownership_flags[1]", h.ownership_flags[1]),
        ("222", "is_cloud_shared", h.is_cloud_shared),
        ("260", "is_subscribed_playlist", h.is_subscribed_playlist),
        (
            "262",
            "checkbox_show_in_profile_and_search",
            h.checkbox_show_in_profile_and_search,
        ),
    ];
    for (offset, field, raw) in bools {
        c.check_bool("lpma", offset, field, u32::from(raw), ctx, None);
    }
    if matches!(
        DistinguishedPlaylistKind::from(h.distinguished_kind),
        DistinguishedPlaylistKind::Other(_)
    ) {
        c.check_enum_fallthrough(
            "lpma",
            "79..81",
            "distinguished_kind",
            h.distinguished_kind.to_string(),
            ctx,
            None,
        );
    }
    check_suggestion_flag(c, "lpma", "223..225", h.suggestion_flag, ctx);
    let item_count = playlist
        .bomas
        .iter()
        .filter(|b| {
            matches!(
                b.payload,
                BomaPayload::PlaylistItem(_) | BomaPayload::PlaylistItemCompact(_)
            )
        })
        .count();
    c.check_section_count(
        "lpma",
        "16..20",
        "track_count",
        h.track_count,
        item_count,
        ctx,
    );
    c.check_count_vs_true(
        "lpma",
        "174..182",
        "play_count/true_play_count",
        h.play_count,
        h.true_play_count,
        ctx,
    );
    if matches!(
        PlaylistShareMode::from(h.cloud_share_mode),
        PlaylistShareMode::Unknown(_)
    ) {
        c.check_enum_fallthrough(
            "lpma",
            "308..312",
            "cloud_share_mode",
            h.cloud_share_mode.to_string(),
            ctx,
            None,
        );
    }
}

fn check_playlist_timestamps(c: &mut SanityCollector, playlist: &PlaylistItem, ctx: &str) {
    let h = &playlist.header;
    c.check_timestamp(
        "lpma",
        "22..26",
        "date_created",
        h.date_created,
        TimestampSpec::UTC,
        ctx,
        None,
    );
    c.check_timestamp(
        "lpma",
        "138..142",
        "date_modified",
        h.date_modified,
        TimestampSpec::UTC,
        ctx,
        None,
    );
    c.check_timestamp(
        "lpma",
        "182..186",
        "date_last_played",
        h.date_last_played,
        TimestampSpec::UTC,
        ctx,
        None,
    );
    c.check_timestamp(
        "lpma",
        "304..308",
        "date_cloud_published",
        h.date_cloud_published,
        TimestampSpec::UTC,
        ctx,
        None,
    );
    if let Some(ts) = h.date_suggestion_flag_modified {
        c.check_timestamp(
            "lpma",
            "324..328",
            "date_suggestion_flag_modified",
            ts,
            TimestampSpec::UTC,
            ctx,
            None,
        );
    }
    if let Some(ts) = h.date_cloud_synced_2 {
        c.check_timestamp(
            "lpma",
            "356..360",
            "date_cloud_synced_2",
            ts,
            TimestampSpec::UTC_CLOUD_SYNC,
            ctx,
            None,
        );
    }
}

fn check_smart_options(c: &mut SanityCollector, opts: &SmartPlaylistOptions, ctx: &str) {
    let bools: [(&str, &str, u8); 5] = [
        ("0", "checkbox_live_updating", opts.checkbox_live_updating),
        (
            "1",
            "checkbox_enable_matching_rules",
            opts.checkbox_enable_matching_rules,
        ),
        ("2", "checkbox_enable_limit", opts.checkbox_enable_limit),
        (
            "12",
            "checkbox_match_only_checked_items",
            opts.checkbox_match_only_checked_items,
        ),
        (
            "13",
            "negate_limit_selection_method",
            opts.negate_limit_selection_method,
        ),
    ];
    for (offset, field, raw) in bools {
        c.check_bool(
            "SmartPlaylistOptions",
            offset,
            field,
            u32::from(raw),
            ctx,
            None,
        );
    }
    if (opts.checkbox_enable_limit != 0 || opts.limit_unit != 3)
        && matches!(
            SmartLimitUnit::from(opts.limit_unit),
            SmartLimitUnit::Unknown(_)
        )
    {
        c.check_enum_fallthrough(
            "SmartPlaylistOptions",
            "3",
            "limit_unit",
            opts.limit_unit.to_string(),
            ctx,
            None,
        );
    }
    if opts.checkbox_enable_limit != 0
        && matches!(
            SmartSelectionMethod::from((
                opts.limit_selection_method,
                opts.negate_limit_selection_method,
            )),
            SmartSelectionMethod::Unknown { .. }
        )
    {
        c.check_enum_fallthrough(
            "SmartPlaylistOptions",
            "4,13",
            "limit_selection_method",
            format!(
                "({:#04x}, negate={})",
                opts.limit_selection_method, opts.negate_limit_selection_method
            ),
            ctx,
            None,
        );
    }
}

fn check_slst(c: &mut SanityCollector, slst: &SlstSection, ctx: &str, prefix: &str) {
    for (idx, rule) in slst.rules.iter().enumerate() {
        let sub = if prefix.is_empty() {
            format!("SLst::rule[{idx}]")
        } else {
            format!("{prefix}.rule[{idx}]")
        };
        check_smart_rule(c, rule, ctx, &sub);
    }
}

fn check_smart_rule(c: &mut SanityCollector, rule: &SmartPlaylistRule, ctx: &str, sub: &str) {
    if rule.field_id == 0
        && let SmartRuleChild::Nested(nested) = &rule.child
    {
        check_slst(c, nested, ctx, sub);
        return;
    }
    if matches!(SmartField::from(rule.field_id), SmartField::Unknown(_)) {
        c.check_enum_fallthrough(
            "SmartPlaylistRule",
            "0..4",
            "field_id",
            format!("{:#x}", rule.field_id),
            ctx,
            Some(sub.to_string()),
        );
    }
    if matches!(
        SmartOperator::from(rule.comparison_method),
        SmartOperator::Unknown(_)
    ) {
        c.check_enum_fallthrough(
            "SmartPlaylistRule",
            "4..8",
            "comparison_method",
            format!("{:#x}", rule.comparison_method),
            ctx,
            Some(sub.to_string()),
        );
    }
    c.check_unknown_bits(
        "SmartPlaylistRule",
        "4..8",
        "comparison_method",
        rule.comparison_method,
        SmartOperatorFlags::all().bits(),
        ctx,
        Some(sub.to_string()),
    );
    match &rule.child {
        SmartRuleChild::Nested(nested) => check_slst(c, nested, ctx, sub),
        SmartRuleChild::Raw(bytes) => {
            c.check_enum_fallthrough(
                "SmartPlaylistRule",
                "56..",
                "child",
                format!("Raw({} bytes)", bytes.len()),
                ctx,
                Some(sub.to_string()),
            );
        }
        SmartRuleChild::Arguments(_) | SmartRuleChild::StringUtf16Be { .. } | _ => {}
    }
}

fn check_cloud_deletion(c: &mut SanityCollector, item: &CloudDeletionItem) {
    if matches!(
        CloudDeletionKind::from(item.deletion_kind),
        CloudDeletionKind::Unknown(_)
    ) {
        let ctx = format!("dCma[saga_id={}]", item.cloud_saga_id);
        c.check_enum_fallthrough(
            "dCma",
            "16",
            "deletion_kind",
            item.deletion_kind.to_string(),
            &ctx,
            None,
        );
    }
}

fn check_social_profile(c: &mut SanityCollector, profile: &SocialProfileItem) {
    let ctx = format_item_context(
        "isma",
        profile.header.profile_persistent_id,
        find_boma_str(&profile.bomas, 0x0321),
        find_boma_str(&profile.bomas, 0x0322),
    );
    check_bomas(c, "isma", &profile.bomas, &ctx);
}

fn check_suggestion_flag(
    c: &mut SanityCollector,
    section: &str,
    offset: &str,
    raw: u16,
    ctx: &str,
) {
    if matches!(SuggestionFlag::from(raw), SuggestionFlag::Unknown(_)) {
        c.check_enum_fallthrough(
            section,
            offset,
            "suggestion_flag",
            format!("{raw:#06x}"),
            ctx,
            None,
        );
    }
    c.check_unknown_bits(
        section,
        offset,
        "suggestion_flag",
        u32::from(raw),
        u32::from(SuggestionFlags::all().bits()),
        ctx,
        None,
    );
}

fn check_bomas(c: &mut SanityCollector, parent: &str, bomas: &[BomaSection], ctx: &str) {
    let mut numerics_idx = 0usize;
    for boma in bomas {
        match &boma.payload {
            BomaPayload::TrackNumerics(n) => {
                let sub = Some(format!(
                    "TrackNumerics[{numerics_idx}] (subrecord_kind={})",
                    n.subrecord_kind
                ));
                numerics_idx += 1;
                check_track_numerics(c, n, ctx, sub);
            }
            BomaPayload::TrackPlaysSkips(p) => check_track_plays_skips(c, p, ctx),
            BomaPayload::VideoInfo(v) => check_video_info(c, v, ctx),
            BomaPayload::SmartPlaylistOptions(opts) => check_smart_options(c, opts, ctx),
            BomaPayload::SmartPlaylistRules(slst) => check_slst(c, slst, ctx, ""),
            BomaPayload::StringSection(s) => {
                check_boma_string(
                    c,
                    parent,
                    boma.subtype,
                    &s.text,
                    &s.raw_string_bytes,
                    s.encoding == 1,
                    ctx,
                );
            }
            BomaPayload::RawStringUtf8 { text, raw_bytes } => {
                check_boma_string(c, parent, boma.subtype, text, raw_bytes, false, ctx);
                if matches!(boma.subtype, 0x001D | 0x0036 | 0x0038 | 0x00CD | 0x0192) {
                    check_xml_plist(c, parent, boma.subtype, raw_bytes, ctx);
                }
            }
            BomaPayload::RawStringUtf16Le { text, raw_bytes } => {
                check_boma_string(c, parent, boma.subtype, text, raw_bytes, true, ctx);
            }
            BomaPayload::BinaryPlist { subtype, raw_bytes } => {
                check_binary_plist(c, parent, *subtype, raw_bytes, ctx);
            }
            BomaPayload::Book {
                extracted_path,
                components,
                raw_bytes,
            } => {
                if extracted_path.as_deref().is_none_or(str::is_empty) || components.is_empty() {
                    let section = format!("{parent}::boma({:#06x})", boma.subtype);
                    let field = musicdb::sections::boma::boma_subtype_name(boma.subtype);
                    c.record(Finding {
                        category: SanityCategory::StringData,
                        severity: SanitySeverity::Warning,
                        rule_id: "string.empty_book_path",
                        section: &section,
                        offset: "payload",
                        field,
                        message: "macOS book bookmark payload yielded no extracted path or components",
                        value: format!(
                            "len={}, path={:?}, components={}",
                            raw_bytes.len(),
                            extracted_path,
                            components.len()
                        ),
                        item_context: ctx,
                        subcontext: None,
                    });
                }
            }
            BomaPayload::PlaylistItem(_)
            | BomaPayload::PlaylistItemCompact(_)
            | BomaPayload::Unknown { .. }
            | _ => {}
        }
    }
}

fn check_boma_string(
    c: &mut SanityCollector,
    parent: &str,
    subtype: u32,
    text: &str,
    raw_bytes: &[u8],
    is_utf16: bool,
    ctx: &str,
) {
    let section = format!("{parent}::boma({subtype:#06x})");
    let field = musicdb::sections::boma::boma_subtype_name(subtype);
    let sensitive = matches!(subtype, 0x003B | 0x003C);
    let preview = || {
        if sensitive {
            "<redacted>".to_string()
        } else {
            truncate_preview(text, 60)
        }
    };

    if is_utf16 && !raw_bytes.len().is_multiple_of(2) {
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.odd_utf16_byte_len",
            section: &section,
            offset: "payload",
            field,
            message: "UTF-16LE string payload has an odd byte length",
            value: format!("len={}", raw_bytes.len()),
            item_context: ctx,
            subcontext: None,
        });
    }
    if text.is_empty() {
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Info,
            rule_id: "string.empty_boma",
            section: &section,
            offset: "payload",
            field,
            message: "string boma payload is empty",
            value: "\"\"".to_string(),
            item_context: ctx,
            subcontext: None,
        });
        return;
    }
    check_boma_string_chars(c, &section, subtype, field, text, &preview, ctx);
}

fn check_boma_string_chars(
    c: &mut SanityCollector,
    section: &str,
    subtype: u32,
    field: &str,
    text: &str,
    preview: &impl Fn() -> String,
    ctx: &str,
) {
    if text.contains('\u{FFFD}') {
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.replacement_char",
            section,
            offset: "payload",
            field,
            message: "decoded string contains Unicode replacement character U+FFFD",
            value: preview(),
            item_context: ctx,
            subcontext: None,
        });
    }
    if text.contains('\0') {
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.embedded_nul",
            section,
            offset: "payload",
            field,
            message: "decoded string contains embedded NUL character",
            value: preview(),
            item_context: ctx,
            subcontext: None,
        });
    }
    if text.chars().any(is_disallowed_control_char) {
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.control_chars",
            section,
            offset: "payload",
            field,
            message: "decoded string contains unexpected C0/DEL/C1 control characters",
            value: preview(),
            item_context: ctx,
            subcontext: None,
        });
    }
    let total_chars = text.chars().count();
    let weird_chars = text.chars().filter(|&ch| is_weird_char(ch)).count();
    if total_chars >= 4 && weird_chars.saturating_mul(5) > total_chars {
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.weird_character_density",
            section,
            offset: "payload",
            field,
            message: "decoded string has >20% PUA, non-character, or replacement code points",
            value: format!("{weird_chars}/{total_chars} {}", preview()),
            item_context: ctx,
            subcontext: None,
        });
    }
    if subtype == 0x0007 {
        let eq = EqualizerPreset::from(text);
        if let Some(id) = eq.preset_id
            && !(100..=121).contains(&id)
        {
            c.check_enum_fallthrough(section, "payload", field, text.to_string(), ctx, None);
        }
    }
}

fn check_xml_plist(
    c: &mut SanityCollector,
    parent: &str,
    subtype: u32,
    raw_bytes: &[u8],
    ctx: &str,
) {
    if let Err(err) = plist::Value::from_reader_xml(Cursor::new(raw_bytes)) {
        let section = format!("{parent}::boma({subtype:#06x})");
        let field = musicdb::sections::boma::boma_subtype_name(subtype);
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.invalid_plist_boma",
            section: &section,
            offset: "payload",
            field,
            message: "XML plist boma failed to parse",
            value: err.to_string(),
            item_context: ctx,
            subcontext: None,
        });
    }
}

fn check_binary_plist(
    c: &mut SanityCollector,
    parent: &str,
    subtype: u32,
    raw_bytes: &[u8],
    ctx: &str,
) {
    if let Err(err) = plist::Value::from_reader(Cursor::new(raw_bytes)) {
        let section = format!("{parent}::boma({subtype:#06x})");
        let field = musicdb::sections::boma::boma_subtype_name(subtype);
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.invalid_plist_boma",
            section: &section,
            offset: "payload",
            field,
            message: "binary plist boma failed to parse",
            value: err.to_string(),
            item_context: ctx,
            subcontext: None,
        });
    }
}

fn is_disallowed_control_char(ch: char) -> bool {
    let code = u32::from(ch);
    (0x01..=0x1F).contains(&code) && !matches!(ch, '\t' | '\n' | '\r')
        || code == 0x7F
        || (0x80..=0x9F).contains(&code)
}

fn is_weird_char(ch: char) -> bool {
    let code = u32::from(ch);
    ch == '\u{FFFD}'
        || (0xE000..=0xF8FF).contains(&code)
        || (0xF_0000..=0xF_FFFD).contains(&code)
        || (0x10_0000..=0x10_FFFD).contains(&code)
        || (0xFDD0..=0xFDEF).contains(&code)
        || (code & 0xFFFE) == 0xFFFE
}

fn find_boma_str(bomas: &[BomaSection], subtype: u32) -> Option<&str> {
    if matches!(subtype, 0x003B | 0x003C) {
        return None;
    }
    bomas
        .iter()
        .find(|b| b.subtype == subtype)
        .and_then(BomaSection::as_str)
}

fn format_item_context(
    section: &str,
    persistent_id: u64,
    primary: Option<&str>,
    secondary: Option<&str>,
) -> String {
    let id = PersistentId(persistent_id);
    match (primary, secondary) {
        (Some(p), Some(s)) if !p.is_empty() && !s.is_empty() => {
            format!("{section}[{id}] ({p:?} — {s})")
        }
        (Some(p), _) if !p.is_empty() => format!("{section}[{id}] ({p:?})"),
        _ => format!("{section}[{id}]"),
    }
}

fn truncate_preview(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    format!("{out:?}")
}

fn current_utc_timestamp() -> Timestamp {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    Timestamp::from_second(secs).unwrap_or(Timestamp::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_opts(include_info: bool) -> SanityCheckOptions {
        SanityCheckOptions {
            include_info,
            // 2026-11-22T00:00:00Z
            reference_now: Timestamp::from_second(1_795_305_600).unwrap(),
            max_examples: 5,
            categories: Vec::new(),
        }
    }

    #[test]
    fn timestamp_predicate_covers_sentinels_epochs_and_future_rules() {
        let opts = fixed_opts(true);
        let mut c = SanityCollector::new(&opts, -10_800);

        // Known sentinel 2 on cloud sync -> Info; unknown sentinel 7 -> Warning.
        c.check_timestamp(
            "TrackNumerics",
            "356..360",
            "date_cloud_synced",
            AppleTimestamp::new(2),
            TimestampSpec::UTC_CLOUD_SYNC,
            "itma[0]",
            None,
        );
        c.check_timestamp(
            "itma",
            "352..356",
            "date_cloud_synced",
            AppleTimestamp::new(7),
            TimestampSpec::UTC,
            "itma[0]",
            None,
        );
        // Pre-1970 allowed on release date, rejected on date_added.
        c.check_timestamp(
            "TrackNumerics",
            "140..144",
            "date_released",
            AppleTimestamp::new(2_000_000_000),
            TimestampSpec::UTC_RELEASE,
            "itma[0]",
            None,
        );
        c.check_timestamp(
            "TrackNumerics",
            "92..96",
            "date_added",
            AppleTimestamp::new(2_000_000_000),
            TimestampSpec::LOCAL,
            "itma[0]",
            None,
        );
        // Expected future (+3 days on lease) -> Info; unexpected future (+3 days on date_modified) -> Warning.
        let plus_3d =
            u32::try_from(opts.reference_now.as_second() + 2_082_844_800 + 3 * 86_400).unwrap();
        c.check_timestamp(
            "plma",
            "120..124",
            "date_cloud_lease_expires",
            AppleTimestamp::new(plus_3d),
            TimestampSpec::UTC_LEASE,
            "plma[0]",
            None,
        );
        c.check_timestamp(
            "hfma",
            "100..104",
            "date_modified",
            AppleTimestamp::new(plus_3d),
            TimestampSpec::UTC,
            "hfma[0]",
            None,
        );
        // DST underflow (-3600 wrapped as u32) -> Warning (timestamp.underflow_sentinel).
        c.check_timestamp(
            "TrackPlaysSkips",
            "8..12",
            "date_last_played",
            AppleTimestamp::new(0u32.wrapping_sub(3600)),
            TimestampSpec::LOCAL,
            "itma[0]",
            None,
        );

        let report = c.finish(&opts, &HfmaHeader::default());
        assert_eq!(report.total_warnings, 4);
        assert_eq!(report.total_info, 2);
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"timestamp.known_sentinel"));
        assert!(rules.contains(&"timestamp.unknown_sentinel"));
        assert!(rules.contains(&"timestamp.underflow_sentinel"));
        assert!(rules.contains(&"timestamp.before_unix_epoch"));
        assert!(rules.contains(&"timestamp.future_expected"));
        assert!(rules.contains(&"timestamp.future_unexpected"));
    }

    #[test]
    fn boma_string_fourcc_and_plist_predicates_flag_corruption() {
        let opts = fixed_opts(false);
        let mut c = SanityCollector::new(&opts, 0);

        check_boma_string(
            &mut c,
            "itma",
            0x0002,
            "Tom\u{FFFD}Sawyer\0\x07",
            &[0u8; 3],
            true,
            "itma[1]",
        );
        check_boma_string(
            &mut c,
            "itma",
            0x0007,
            "#!#999#!#",
            b"#!#999#!#",
            false,
            "itma[1]",
        );
        check_xml_plist(&mut c, "itma", 0x0036, b"<not valid xml", "itma[1]");
        c.check_fourcc(
            "TrackNumerics",
            "64..68",
            "file_type_fourcc",
            0x00FF_0001,
            "itma[1]",
            None,
        );

        let report = c.finish(&opts, &HfmaHeader::default());
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"string.odd_utf16_byte_len"));
        assert!(rules.contains(&"string.replacement_char"));
        assert!(rules.contains(&"string.embedded_nul"));
        assert!(rules.contains(&"string.control_chars"));
        assert!(rules.contains(&"enum.fallthrough"));
        assert!(rules.contains(&"string.invalid_plist_boma"));
        assert!(rules.contains(&"string.invalid_fourcc"));
    }

    fn build_minimal_rush_raw() -> RawLibraryFile {
        // Minimal boma 0x0002 ("Tom Sawyer" UTF-16LE).
        let text_bytes: Vec<u8> = "Tom Sawyer"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let boma_total = u32::try_from(20 + 16 + text_bytes.len()).unwrap();
        let mut boma = vec![0u8; 20 + 16];
        boma[0..4].copy_from_slice(b"boma");
        boma[4..8].copy_from_slice(&20u32.to_le_bytes());
        boma[8..12].copy_from_slice(&boma_total.to_le_bytes());
        boma[12..16].copy_from_slice(&0x0002u32.to_le_bytes());
        boma[20..24].copy_from_slice(&1u32.to_le_bytes()); // UTF-16LE
        boma[24..28].copy_from_slice(&u32::try_from(text_bytes.len()).unwrap().to_le_bytes());
        boma.extend_from_slice(&text_bytes);

        // Minimal itma (376-byte header) + boma.
        let track_total = u32::try_from(376 + boma.len()).unwrap();
        let mut track_buf = vec![0u8; 376];
        track_buf[0..4].copy_from_slice(b"itma");
        track_buf[4..8].copy_from_slice(&376u32.to_le_bytes());
        track_buf[8..12].copy_from_slice(&track_total.to_le_bytes());
        track_buf[12..16].copy_from_slice(&1u32.to_le_bytes()); // boma_count
        track_buf[16..24].copy_from_slice(&0x2112u64.to_le_bytes());
        track_buf[57] = 1; // DownloadedStatus::Downloaded
        track_buf[58] = 2; // CloudStatus::Matched
        track_buf[108..112].copy_from_slice(&1u32.to_le_bytes()); // MediaKind::Music
        track_buf.extend_from_slice(&boma);

        // Minimal ltma (92-byte header) + itma, wrapped in hsma (56-byte header).
        let mut list_buf = vec![0u8; 92];
        list_buf[0..4].copy_from_slice(b"ltma");
        list_buf[4..8].copy_from_slice(&92u32.to_le_bytes());
        list_buf[8..12].copy_from_slice(&1u32.to_le_bytes()); // track_count
        list_buf.extend_from_slice(&track_buf);

        let hsma_total = u32::try_from(56 + list_buf.len()).unwrap();
        let mut hsma = vec![0u8; 56];
        hsma[0..4].copy_from_slice(b"hsma");
        hsma[4..8].copy_from_slice(&56u32.to_le_bytes());
        hsma[8..12].copy_from_slice(&hsma_total.to_le_bytes());
        hsma[12..16].copy_from_slice(&1u32.to_le_bytes()); // SectionKind::TrackList
        hsma.extend_from_slice(&list_buf);

        // Minimal hfma (160-byte header) + hsma.
        let mut raw = vec![0u8; 160];
        raw[0..4].copy_from_slice(b"hfma");
        raw[4..8].copy_from_slice(&160u32.to_le_bytes());
        raw[56..60].copy_from_slice(&1u32.to_le_bytes()); // hsma_count
        raw[68..72].copy_from_slice(&1u32.to_le_bytes()); // track_count
        raw.extend_from_slice(&hsma);

        RawLibraryFile::from_raw_bytes(&raw).expect("parse minimal synthetic Rush library")
    }

    #[test]
    fn minimal_rush_smoke_test_clean_and_mutated() {
        let mut raw = build_minimal_rush_raw();
        let opts = fixed_opts(false);
        let clean = analyze_sanity(&raw, &opts);
        assert_eq!(clean.total_warnings, 0);

        let HsmaChild::TrackList(list) = &mut raw.sections[0].child else {
            panic!("expected TrackList");
        };
        list.tracks[0].header.cloud_status = 99;
        list.tracks[0].header.audio_traits = 0x80;
        list.tracks[0].header.checkbox_disabled = 2;

        let mutated = analyze_sanity(&raw, &opts);
        assert_eq!(mutated.total_warnings, 3);
        let rendered = mutated.to_string();
        assert!(rendered.contains("Tom Sawyer"));
        assert!(rendered.contains("enum.fallthrough"));
        assert!(rendered.contains("bitflag.unknown_bits"));
        assert!(rendered.contains("bool.non_binary"));
        assert!(serde_json::to_string(&mutated).is_ok());
    }

    #[test]
    fn tier2_bounds_counts_strings_and_category_filtering() {
        let mut raw = build_minimal_rush_raw();
        raw.outer_header.hsma_count = 9;
        raw.outer_header.track_count = 3;

        let HsmaChild::TrackList(list) = &mut raw.sections[0].child else {
            panic!("expected TrackList");
        };
        let track = &mut list.tracks[0];
        track.header.star_rating = 55;
        track.header.volume_adjustment = 300;
        track.header.track_number = 8;
        track.header.track_total = 7;
        track.header.disc_number = 2;
        track.header.disc_total = 1;
        track.header.movement_number = 3;
        track.header.movement_total = 2;
        track.header.playback_start_ms = 50_000;
        track.header.playback_stop_ms = 10_000;

        let opts = fixed_opts(false);
        let full = analyze_sanity(&raw, &opts);
        let rules: Vec<&str> = full.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"bounds.section_count_mismatch"));
        assert!(rules.contains(&"bounds.star_rating"));
        assert!(rules.contains(&"bounds.volume_adjustment"));
        assert!(rules.contains(&"bounds.number_exceeds_total"));
        assert!(rules.contains(&"bounds.playback_range"));

        // Category filter restricts findings cleanly.
        let mut only_strings = fixed_opts(false);
        only_strings.categories = vec![SanityCategory::StringData];
        let filtered = analyze_sanity(&raw, &only_strings);
        assert_eq!(filtered.total_warnings, 0);
        assert!(filtered.findings.is_empty());

        // TrackNumerics sample_rate, TrackPlaysSkips counts, weird chars, and empty Book payload.
        let sub_opts = fixed_opts(true);
        let mut c = SanityCollector::new(&sub_opts, 0);
        let mut num = TrackNumerics::default();
        num.sample_rate = 999_999.0;
        check_track_numerics(&mut c, &num, "itma[2112]", None);
        let mut ps = TrackPlaysSkips::default();
        ps.play_count = 5;
        ps.true_play_count = 2;
        ps.skip_count = 4;
        ps.true_skip_count = 1;
        check_track_plays_skips(&mut c, &ps, "itma[2112]");
        check_boma_string(
            &mut c,
            "itma",
            0x0002,
            "\u{E000}\u{E001}Rush",
            b"Rush",
            false,
            "itma[2112]",
        );
        let mut raw_book_boma = vec![0u8; 28];
        raw_book_boma[0..4].copy_from_slice(b"boma");
        raw_book_boma[4..8].copy_from_slice(&20u32.to_le_bytes());
        raw_book_boma[8..12].copy_from_slice(&28u32.to_le_bytes());
        raw_book_boma[12..16].copy_from_slice(&0x0042u32.to_le_bytes());
        raw_book_boma[20..24].copy_from_slice(b"book");
        let empty_book: BomaSection =
            binrw::BinRead::read_le(&mut Cursor::new(&raw_book_boma)).unwrap();
        check_bomas(&mut c, "itma", &[empty_book], "itma[2112]");

        let sub_report = c.finish(&sub_opts, &HfmaHeader::default());
        let sub_rules: Vec<&str> = sub_report.findings.iter().map(|g| g.rule_id).collect();
        assert!(sub_rules.contains(&"bounds.sample_rate"));
        assert!(sub_rules.contains(&"bounds.count_exceeds_true"));
        assert!(sub_rules.contains(&"string.weird_character_density"));
        assert!(sub_rules.contains(&"string.empty_book_path"));
    }
}
