//! Section-first sanity audit for parsed [`RawLibraryFile`] structures.
//!
//! Audits raw `musicdb::sections` fields before [`musicdb::Library`] normalizes,
//! collapses, or silently drops anomalies, using `musicdb::model` enums and
//! bitflags purely as read-only validation oracles.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use jiff::Timestamp;
use serde::Serialize;

use musicdb::sections::{
    AlbumItem, AppleTimestamp, ArtistItem, BomaPayload, BomaSection, CloudDeletionItem, HfmaHeader,
    HsmaChild, LibraryMasterSection, PlaylistItem, RawLibraryFile, SectionKind, SlstSection,
    SmartPlaylistOptions, SmartPlaylistRule, SmartPlaylistRuleArguments, SmartRuleChild,
    SocialProfileItem, TrackItem, TrackNumerics, TrackPlaysSkips, VideoInfo,
};
use musicdb::{
    AudioTraits, CatalogAlbumType, CloudDeletionKind, CloudStatus, ContentRating,
    DistinguishedPlaylistKind, DownloadedStatus, EmbeddedArtworkKind, EqualizerPreset, MediaKind,
    MediaKindFlags, PersistentId, PlaylistShareMode, RelativeTimeUnit, SmartField, SmartLimitUnit,
    SmartOperator, SmartOperatorFlags, SmartSelectionMethod, SortMode, SortModeFlags,
    SuggestionFlag, SuggestionFlags,
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
    #[value(name = "url")]
    Url,
}

impl fmt::Display for SanityCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnumFallthrough => f.write_str("enum_fallthrough"),
            Self::Bitflag => f.write_str("bitflag"),
            Self::Timestamp => f.write_str("timestamp"),
            Self::StringData => f.write_str("string_data"),
            Self::Bounds => f.write_str("bounds"),
            Self::Url => f.write_str("url"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    fn check_state_value(
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
            rule_id: "state.unknown_value",
            section,
            offset,
            field,
            message: "raw state field holds an undocumented value",
            value,
            item_context: ctx,
            subcontext: sub,
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared bitflag combination predicate needs section, offset, field, message, value, and context"
    )]
    fn check_bitflag_combination(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        message: &str,
        value: String,
        ctx: &str,
        sub: Option<String>,
    ) {
        self.record(Finding {
            category: SanityCategory::Bitflag,
            severity: SanitySeverity::Warning,
            rule_id: "bitflag.invalid_combination",
            section,
            offset,
            field,
            message,
            value,
            item_context: ctx,
            subcontext: sub,
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared mirror check needs section, offset, field, expected/actual values, and context"
    )]
    fn check_mirror_mismatch(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        expected: &str,
        actual: &str,
        ctx: &str,
        sub: Option<String>,
    ) {
        self.record(Finding {
            category: SanityCategory::Bounds,
            severity: SanitySeverity::Warning,
            rule_id: "bounds.mirror_mismatch",
            section,
            offset,
            field,
            message: "mirrored header field does not match primary value",
            value: format!("primary={expected}, mirror={actual}"),
            item_context: ctx,
            subcontext: sub,
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared store-ID divergence check needs section, offset, field, expected/actual values, and context"
    )]
    fn check_store_id_divergence(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        primary: u64,
        secondary: u64,
        ctx: &str,
        sub: Option<String>,
    ) {
        self.record(Finding {
            category: SanityCategory::Bounds,
            severity: SanitySeverity::Info,
            rule_id: "bounds.store_id_divergence",
            section,
            offset,
            field,
            message: "secondary Apple Store ID differs from primary (common on matched/re-released catalog items)",
            value: format!("primary={primary}, secondary={secondary}"),
            item_context: ctx,
            subcontext: sub,
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared referential integrity check needs source/target metadata, id, and context"
    )]
    fn check_dangling_ref(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        message: &str,
        id: u64,
        ctx: &str,
        sub: Option<String>,
    ) {
        self.record(Finding {
            category: SanityCategory::Bounds,
            severity: SanitySeverity::Warning,
            rule_id: "bounds.dangling_reference",
            section,
            offset,
            field,
            message,
            value: PersistentId(id).to_string(),
            item_context: ctx,
            subcontext: sub,
        });
    }

    fn check_http_url(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        url: &str,
        ctx: &str,
        sub: Option<String>,
    ) {
        if let Err(reason) = validate_http_url(url) {
            self.record(Finding {
                category: SanityCategory::Url,
                severity: SanitySeverity::Warning,
                rule_id: "url.invalid_http_url",
                section,
                offset,
                field,
                message: reason,
                value: truncate_preview(url, 80),
                item_context: ctx,
                subcontext: sub,
            });
        }
    }

    fn check_file_url(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        url: &str,
        ctx: &str,
        sub: Option<String>,
    ) {
        if let Err(reason) = validate_file_url(url) {
            self.record(Finding {
                category: SanityCategory::Url,
                severity: SanitySeverity::Warning,
                rule_id: "url.invalid_file_url",
                section,
                offset,
                field,
                message: reason,
                value: truncate_preview(url, 80),
                item_context: ctx,
                subcontext: sub,
            });
        }
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
            } else if raw == 3600 && spec.epoch == TimestampEpoch::Local {
                (
                    SanitySeverity::Info,
                    "timestamp.dst_zero_sentinel",
                    "known ±1h DST shift on unset local AppleTimestamp",
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
            let (severity, rule_id, message) = if raw == 0u32.wrapping_sub(3600)
                && spec.epoch == TimestampEpoch::Local
            {
                (
                    SanitySeverity::Info,
                    "timestamp.dst_zero_sentinel",
                    "known ±1h DST shift on unset local AppleTimestamp",
                )
            } else {
                (
                    SanitySeverity::Warning,
                    "timestamp.underflow_sentinel",
                    "AppleTimestamp wrapped within 24h below 0 (e.g. DST underflow or u32::MAX)",
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

    fn check_unix_timestamp(
        &mut self,
        section: &str,
        offset: &str,
        field: &str,
        raw: u64,
        ctx: &str,
        sub: Option<String>,
    ) {
        if raw == 0 {
            return;
        }
        if raw < u64::from(APPLE_SENTINEL_CUTOFF) {
            self.record(Finding {
                category: SanityCategory::Timestamp,
                severity: SanitySeverity::Warning,
                rule_id: "timestamp.unknown_sentinel",
                section,
                offset,
                field,
                message: "non-zero Unix timestamp below 86_400 sentinel cutoff",
                value: raw.to_string(),
                item_context: ctx,
                subcontext: sub,
            });
            return;
        }
        let Some(utc) = i64::try_from(raw)
            .ok()
            .and_then(|secs| Timestamp::from_second(secs).ok())
        else {
            self.record(Finding {
                category: SanityCategory::Timestamp,
                severity: SanitySeverity::Warning,
                rule_id: "timestamp.invalid_unix_timestamp",
                section,
                offset,
                field,
                message: "Unix timestamp exceeds representable date range",
                value: raw.to_string(),
                item_context: ctx,
                subcontext: sub,
            });
            return;
        };
        let secs = utc.as_second();
        if secs < ITUNES_ERA_START_UNIX {
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
        } else if secs > self.now_secs.saturating_add(ONE_DAY_SECS) {
            self.record(Finding {
                category: SanityCategory::Timestamp,
                severity: SanitySeverity::Warning,
                rule_id: "timestamp.future_unexpected",
                section,
                offset,
                field,
                message: "timestamp is unexpectedly in the future",
                value: format!("{utc} (raw={raw})"),
                item_context: ctx,
                subcontext: sub,
            });
        }
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

#[derive(Default)]
struct LibraryIdIndex {
    albums: HashSet<u64>,
    artists: HashSet<u64>,
    tracks: HashSet<u64>,
    playlists: HashSet<u64>,
    folders: HashSet<u64>,
    profiles: HashSet<u64>,
}

impl LibraryIdIndex {
    fn from_raw(raw: &RawLibraryFile) -> Self {
        let mut idx = Self::default();
        for sec in &raw.sections {
            match &sec.child {
                HsmaChild::AlbumList(l) => {
                    idx.albums
                        .extend(l.albums.iter().map(|a| a.header.album_persistent_id));
                }
                HsmaChild::ArtistList(l) => {
                    idx.artists
                        .extend(l.artists.iter().map(|a| a.header.artist_persistent_id));
                }
                HsmaChild::TrackList(l) => {
                    idx.tracks
                        .extend(l.tracks.iter().map(|t| t.header.track_persistent_id));
                }
                HsmaChild::PlaylistList(l) => {
                    for p in &l.playlists {
                        idx.playlists.insert(p.header.playlist_persistent_id);
                        if p.header.is_folder != 0 {
                            idx.folders.insert(p.header.playlist_persistent_id);
                        }
                    }
                }
                HsmaChild::SocialProfileList(l) => {
                    idx.profiles
                        .extend(l.profiles.iter().map(|p| p.header.profile_persistent_id));
                }
                _ => {}
            }
        }
        idx
    }
}

/// Runs all sanity checks directly on a parsed [`RawLibraryFile`].
pub(crate) fn analyze_sanity(raw: &RawLibraryFile, opts: &SanityCheckOptions) -> SanityReport {
    let mut c = SanityCollector::new(opts, raw.outer_header.timezone_offset_seconds);
    let ids = LibraryIdIndex::from_raw(raw);
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
            HsmaChild::InnerHfma(h) => {
                check_hfma(&mut c, "inner_hfma", h);
                check_inner_hfma_mirrors(&mut c, &raw.outer_header, h);
            }
            HsmaChild::LibraryMaster(plma) => check_plma(&mut c, &raw.outer_header, plma),
            HsmaChild::AlbumList(list) => {
                for album in &list.albums {
                    check_album(&mut c, album, &ids);
                }
            }
            HsmaChild::ArtistList(list) => {
                for artist in &list.artists {
                    check_artist(&mut c, artist);
                }
            }
            HsmaChild::TrackList(list) => {
                for track in &list.tracks {
                    check_track(&mut c, track, &ids);
                }
            }
            HsmaChild::PlaylistList(list) => {
                for playlist in &list.playlists {
                    check_playlist(&mut c, playlist, &ids);
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
    if h.file_size_mirror != 0 && h.file_size_mirror != h.file_size {
        c.check_mirror_mismatch(
            section,
            "128..132",
            "file_size_mirror",
            &h.file_size.to_string(),
            &h.file_size_mirror.to_string(),
            &ctx,
            None,
        );
    }
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

fn check_inner_hfma_mirrors(c: &mut SanityCollector, outer: &HfmaHeader, inner: &HfmaHeader) {
    let ctx = format!("inner_hfma[{}]", PersistentId(inner.library_persistent_id));
    if inner.library_persistent_id != 0
        && outer.library_persistent_id != 0
        && inner.library_persistent_id != outer.library_persistent_id
    {
        c.check_mirror_mismatch(
            "inner_hfma",
            "48..56",
            "library_persistent_id",
            &PersistentId(outer.library_persistent_id).to_string(),
            &PersistentId(inner.library_persistent_id).to_string(),
            &ctx,
            None,
        );
    }
    if (inner.major_version, inner.minor_version) != (outer.major_version, outer.minor_version) {
        c.check_mirror_mismatch(
            "inner_hfma",
            "12..16",
            "format_version",
            &format!("{}.{}", outer.major_version, outer.minor_version),
            &format!("{}.{}", inner.major_version, inner.minor_version),
            &ctx,
            None,
        );
    }
}

fn check_plma(c: &mut SanityCollector, outer: &HfmaHeader, plma: &LibraryMasterSection) {
    let h = &plma.header;
    let ctx = format!("plma[{}]", PersistentId(h.library_persistent_id));
    check_plma_mirrors(c, outer, h, &ctx);
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
    c.check_unix_timestamp(
        "plma",
        "112..120",
        "date_cloud_purchase_history_modified",
        h.date_cloud_purchase_history_modified,
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
    check_bomas(c, "plma", &plma.bomas, &ctx, None, None);
}

fn check_plma_mirrors(
    c: &mut SanityCollector,
    outer: &HfmaHeader,
    h: &musicdb::sections::LibraryMasterHeader,
    ctx: &str,
) {
    if h.library_persistent_id != 0
        && outer.library_persistent_id != 0
        && h.library_persistent_id != outer.library_persistent_id
    {
        c.check_mirror_mismatch(
            "plma",
            "58..66",
            "library_persistent_id",
            &PersistentId(outer.library_persistent_id).to_string(),
            &PersistentId(h.library_persistent_id).to_string(),
            ctx,
            None,
        );
    }
    if h.library_persistent_id_2 != 0 && h.library_persistent_id_2 != h.library_persistent_id {
        c.check_mirror_mismatch(
            "plma",
            "92..100",
            "library_persistent_id_2",
            &PersistentId(h.library_persistent_id).to_string(),
            &PersistentId(h.library_persistent_id_2).to_string(),
            ctx,
            None,
        );
    }
    let dsid = outer.apple_store_account_id;
    if dsid != 0 {
        for (offset, field, val) in [
            (
                "46..54",
                "apple_store_account_id_1",
                h.apple_store_account_id_1,
            ),
            (
                "84..92",
                "apple_store_account_id_2",
                h.apple_store_account_id_2,
            ),
            (
                "104..112",
                "apple_store_account_id",
                h.apple_store_account_id,
            ),
        ] {
            if val != 0 && val != dsid {
                c.check_mirror_mismatch(
                    "plma",
                    offset,
                    field,
                    &dsid.to_string(),
                    &val.to_string(),
                    ctx,
                    None,
                );
            }
        }
    }
}

fn check_album(c: &mut SanityCollector, album: &AlbumItem, ids: &LibraryIdIndex) {
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
    if !matches!(h.star_rating_inheritance, 0 | 0x01 | 0x20) {
        c.check_state_value(
            "iama",
            "41",
            "star_rating_inheritance",
            format!("{:#04x}", h.star_rating_inheritance),
            &ctx,
            None,
        );
    }
    check_suggestion_flag(c, "iama", "42..44", h.suggestion_flag, &ctx);
    check_album_mirrors_and_refs(c, h, ids, &ctx);
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
    check_album_timestamps(c, h, &ctx);
    check_bomas(c, "iama", &album.bomas, &ctx, None, None);
}

fn check_album_timestamps(
    c: &mut SanityCollector,
    h: &musicdb::sections::AlbumItemHeader,
    ctx: &str,
) {
    c.check_timestamp(
        "iama",
        "96..100",
        "date_suggestion_flag_modified",
        h.date_suggestion_flag_modified,
        TimestampSpec::UTC,
        ctx,
        None,
    );
    c.check_timestamp(
        "iama",
        "100..104",
        "date_last_played",
        h.date_last_played,
        TimestampSpec::LOCAL,
        ctx,
        None,
    );
    if let Some(ts) = h.date_cloud_synced {
        c.check_timestamp(
            "iama",
            "120..124",
            "date_cloud_synced",
            ts,
            TimestampSpec::UTC_CLOUD_SYNC,
            ctx,
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
            ctx,
            None,
        );
    }
}

fn check_album_mirrors_and_refs(
    c: &mut SanityCollector,
    h: &musicdb::sections::AlbumItemHeader,
    ids: &LibraryIdIndex,
    ctx: &str,
) {
    if h.album_persistent_id_2 != 0 && h.album_persistent_id_2 != h.album_persistent_id {
        c.check_mirror_mismatch(
            "iama",
            "64..72",
            "album_persistent_id_2",
            &PersistentId(h.album_persistent_id).to_string(),
            &PersistentId(h.album_persistent_id_2).to_string(),
            ctx,
            None,
        );
    }
    if let (Some(id1), Some(id2)) = (h.apple_music_store_album_id, h.apple_music_store_album_id_2)
        && id1 != 0
        && id2 != 0
        && id1 != id2
    {
        c.check_mirror_mismatch(
            "iama",
            "112..120",
            "apple_music_store_album_id_2",
            &id1.to_string(),
            &id2.to_string(),
            ctx,
            None,
        );
    }
    if h.first_track_persistent_id != 0 && !ids.tracks.contains(&h.first_track_persistent_id) {
        c.check_dangling_ref(
            "iama",
            "32..40",
            "first_track_persistent_id",
            "album first_track_persistent_id does not exist in ltma track list",
            h.first_track_persistent_id,
            ctx,
            None,
        );
    }
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
    check_artwork_states(
        c,
        "iAma",
        ArtworkStateFields {
            source_offset: "96..100",
            source_kind: h.artwork_source_kind,
            cache_offset: "100",
            cache_state: u32::from(h.artwork_cache_state),
            size_offset: "104..108",
            size_kind: h.artwork_size_kind,
            format_offset: "108..112",
            image_format: h.artwork_image_format,
        },
        &ctx,
    );
    if h.artist_persistent_id_2 != 0 && h.artist_persistent_id_2 != h.artist_persistent_id {
        c.check_mirror_mismatch(
            "iAma",
            "80..88",
            "artist_persistent_id_2",
            &PersistentId(h.artist_persistent_id).to_string(),
            &PersistentId(h.artist_persistent_id_2).to_string(),
            &ctx,
            None,
        );
    }
    if h.apple_music_store_artist_id != 0
        && h.apple_music_store_artist_id_2 != 0
        && h.apple_music_store_artist_id != h.apple_music_store_artist_id_2
    {
        c.check_store_id_divergence(
            "iAma",
            "112..120",
            "apple_music_store_artist_id_2",
            h.apple_music_store_artist_id,
            h.apple_music_store_artist_id_2,
            &ctx,
            None,
        );
    }
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
    check_bomas(c, "iAma", &artist.bomas, &ctx, None, None);
}

fn check_track(c: &mut SanityCollector, track: &TrackItem, ids: &LibraryIdIndex) {
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
    check_track_states_mirrors_and_refs(c, track, ids, &ctx);
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
    check_bomas(
        c,
        "itma",
        &track.bomas,
        &ctx,
        Some(ids),
        Some(ParentTrackContext {
            persistent_id: h.track_persistent_id,
            year: h.year,
        }),
    );
}

fn check_track_states_mirrors_and_refs(
    c: &mut SanityCollector,
    track: &TrackItem,
    ids: &LibraryIdIndex,
    ctx: &str,
) {
    let h = &track.header;
    if !matches!(h.store_media_kind, 0 | 1 | 5 | 6) {
        c.check_state_value(
            "itma",
            "64",
            "store_media_kind",
            h.store_media_kind.to_string(),
            ctx,
            None,
        );
    }
    if !matches!(h.cloud_sync_state, 0 | 2 | 3) {
        c.check_state_value(
            "itma",
            "254",
            "cloud_sync_state",
            h.cloud_sync_state.to_string(),
            ctx,
            None,
        );
    }
    check_artwork_states(
        c,
        "itma",
        ArtworkStateFields {
            source_offset: "308..312",
            source_kind: h.artwork_source_kind,
            cache_offset: "328",
            cache_state: u32::from(h.artwork_cache_state),
            size_offset: "344..348",
            size_kind: h.artwork_size_kind,
            format_offset: "348..352",
            image_format: h.artwork_image_format,
        },
        ctx,
    );
    check_track_mirrors_and_refs(c, h, ids, ctx);
}

fn check_track_mirrors_and_refs(
    c: &mut SanityCollector,
    h: &musicdb::sections::TrackItemHeader,
    ids: &LibraryIdIndex,
    ctx: &str,
) {
    if h.track_persistent_id_2 != 0 && h.track_persistent_id_2 != h.track_persistent_id {
        c.check_mirror_mismatch(
            "itma",
            "272..280",
            "track_persistent_id_2",
            &PersistentId(h.track_persistent_id).to_string(),
            &PersistentId(h.track_persistent_id_2).to_string(),
            ctx,
            None,
        );
    }
    if h.cloud_saga_id_1 != 0 && h.cloud_saga_id_2 != 0 && h.cloud_saga_id_1 != h.cloud_saga_id_2 {
        c.check_mirror_mismatch(
            "itma",
            "320..328",
            "cloud_saga_id_2",
            &format!("{:#x}", h.cloud_saga_id_1),
            &format!("{:#x}", h.cloud_saga_id_2),
            ctx,
            None,
        );
    }
    if h.album_persistent_id != 0 && !ids.albums.contains(&h.album_persistent_id) {
        c.check_dangling_ref(
            "itma",
            "172..180",
            "album_persistent_id",
            "track album_persistent_id does not exist in lama album list",
            h.album_persistent_id,
            ctx,
            None,
        );
    }
    if h.artist_persistent_id != 0 && !ids.artists.contains(&h.artist_persistent_id) {
        c.check_dangling_ref(
            "itma",
            "180..188",
            "artist_persistent_id",
            "track artist_persistent_id does not exist in lAma artist list",
            h.artist_persistent_id,
            ctx,
            None,
        );
    }
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
    let fields: [(&str, &str, u8); 16] = [
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
        (
            "53",
            "has_extended_text_metadata",
            h.has_extended_text_metadata,
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
    const SORT_BOMA_SUBTYPES: [u32; 6] = [0x001E, 0x001F, 0x0020, 0x0021, 0x0022, 0x0053];
    let h = &track.header;
    for (i, (&raw, subtype)) in h.auto_sort_flags.iter().zip(SORT_BOMA_SUBTYPES).enumerate() {
        let has_sort_boma = track.bomas.iter().any(|b| b.subtype == subtype);
        check_track_sort_slot(c, i, raw, subtype, has_sort_boma, ctx);
    }
    if matches!(MediaKind::from(h.media_kind), MediaKind::Unknown(_)) {
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
    check_audio_traits_combination(
        c,
        "itma",
        "331",
        "audio_traits",
        u32::from(h.audio_traits),
        ctx,
        None,
    );
}

fn check_track_sort_slot(
    c: &mut SanityCollector,
    index: usize,
    raw: u8,
    subtype: u32,
    has_sort_boma: bool,
    ctx: &str,
) {
    let sub = Some(format!("auto_sort_flags[{index}]"));
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
        sub.clone(),
    );
    let flags = SortModeFlags::from_bits_retain(raw);
    if flags.contains(SortModeFlags::CUSTOM_OVERRIDE)
        && flags.contains(SortModeFlags::AUTO_GENERATED)
    {
        c.check_bitflag_combination(
            "itma",
            "66..72",
            "auto_sort_flags",
            "CUSTOM_OVERRIDE and AUTO_GENERATED are mutually exclusive",
            format!("{raw:#04x}"),
            ctx,
            sub.clone(),
        );
    } else if flags.contains(SortModeFlags::CUSTOM_OVERRIDE)
        && !flags.contains(SortModeFlags::HAS_SORT_STRING)
    {
        c.check_bitflag_combination(
            "itma",
            "66..72",
            "auto_sort_flags",
            "CUSTOM_OVERRIDE is set without HAS_SORT_STRING",
            format!("{raw:#04x}"),
            ctx,
            sub.clone(),
        );
    }
    if flags.contains(SortModeFlags::HAS_SORT_STRING) != has_sort_boma {
        c.check_bitflag_combination(
            "itma",
            "66..72",
            "auto_sort_flags",
            "HAS_SORT_STRING flag disagrees with presence of corresponding sort boma",
            format!("flag={raw:#04x}, boma({subtype:#06x})={has_sort_boma}"),
            ctx,
            sub,
        );
    }
}

fn check_audio_traits_combination(
    c: &mut SanityCollector,
    section: &str,
    offset: &str,
    field: &str,
    raw: u32,
    ctx: &str,
    sub: Option<String>,
) {
    let Ok(bits) = u8::try_from(raw) else {
        return;
    };
    let traits = AudioTraits::from_bits_retain(bits);
    if traits.contains(AudioTraits::HI_RES_LOSSLESS) && !traits.contains(AudioTraits::LOSSLESS) {
        c.check_bitflag_combination(
            section,
            offset,
            field,
            "HI_RES_LOSSLESS is set without LOSSLESS",
            format!("{raw:#04x} ({traits})"),
            ctx,
            sub.clone(),
        );
    }
    if traits.contains(AudioTraits::SPATIAL_AUDIO)
        && !traits.intersects(AudioTraits::DOLBY_ATMOS | AudioTraits::DOLBY_AUDIO)
    {
        c.check_bitflag_combination(
            section,
            offset,
            field,
            "SPATIAL_AUDIO is set without DOLBY_ATMOS or DOLBY_AUDIO",
            format!("{raw:#04x} ({traits})"),
            ctx,
            sub,
        );
    }
}

fn check_track_numerics(
    c: &mut SanityCollector,
    n: &TrackNumerics,
    ctx: &str,
    sub: Option<String>,
    track_year: Option<u32>,
) {
    check_track_numerics_booleans(c, n, ctx, sub.clone());
    check_track_numerics_states_and_mirrors(c, n, ctx, sub.as_deref());
    check_track_numerics_traits_and_fourcc(c, n, ctx, sub.clone());
    check_track_numerics_sample_rate(c, n.sample_rate, ctx, sub.clone());
    check_track_numerics_timestamps(c, n, ctx, sub, track_year);
}

fn check_track_numerics_booleans(
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
            subcontext: sub,
        });
    }
}

fn check_track_numerics_traits_and_fourcc(
    c: &mut SanityCollector,
    n: &TrackNumerics,
    ctx: &str,
    sub: Option<String>,
) {
    c.check_unknown_bits(
        "TrackNumerics",
        "336..340",
        "downloaded_audio_traits",
        n.downloaded_audio_traits,
        u32::from(AudioTraits::all().bits()),
        ctx,
        sub.clone(),
    );
    if n.downloaded_audio_traits != 0 && n.subrecord_kind != 1 {
        c.check_bitflag_combination(
            "TrackNumerics",
            "336..340",
            "downloaded_audio_traits",
            "downloaded_audio_traits is non-zero on non-local subrecord",
            format!(
                "traits={:#x}, subrecord_kind={}",
                n.downloaded_audio_traits, n.subrecord_kind
            ),
            ctx,
            sub.clone(),
        );
    }
    check_audio_traits_combination(
        c,
        "TrackNumerics",
        "336..340",
        "downloaded_audio_traits",
        n.downloaded_audio_traits,
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
        sub,
    );
}

fn check_track_numerics_states_and_mirrors(
    c: &mut SanityCollector,
    n: &TrackNumerics,
    ctx: &str,
    sub: Option<&str>,
) {
    let checks: [(&str, &str, u32, bool); 7] = [
        (
            "4..8",
            "subrecord_kind",
            n.subrecord_kind,
            matches!(n.subrecord_kind, 1 | 3),
        ),
        (
            "54",
            "asset_container_kind",
            u32::from(n.asset_container_kind),
            matches!(n.asset_container_kind, 0 | 1 | 2 | 18),
        ),
        (
            "59",
            "catalog_lyrics_state",
            u32::from(n.catalog_lyrics_state),
            matches!(n.catalog_lyrics_state, 0..=2),
        ),
        (
            "78..80",
            "container_format_kind",
            u32::from(n.container_format_kind),
            matches!(n.container_format_kind, 0 | 3 | 4 | 12 | 51 | 60 | 70),
        ),
        (
            "82..84",
            "local_artwork_state",
            u32::from(n.local_artwork_state),
            matches!(n.local_artwork_state, 0 | 2),
        ),
        (
            "100..104",
            "drm_platform",
            n.drm_platform,
            matches!(n.drm_platform, 0..=2),
        ),
        (
            "144..148",
            "cloud_codec_kind",
            n.cloud_codec_kind,
            matches!(n.cloud_codec_kind, 0 | 2 | 3),
        ),
    ];
    for (offset, field, val, ok) in checks {
        if !ok {
            c.check_state_value(
                "TrackNumerics",
                offset,
                field,
                val.to_string(),
                ctx,
                sub.map(str::to_string),
            );
        }
    }
    if matches!(
        EmbeddedArtworkKind::from(n.has_embedded_artwork),
        EmbeddedArtworkKind::Unknown(_)
    ) {
        c.check_enum_fallthrough(
            "TrackNumerics",
            "80..82",
            "has_embedded_artwork",
            n.has_embedded_artwork.to_string(),
            ctx,
            sub.map(str::to_string),
        );
    }
    check_track_numerics_mirrors(c, n, ctx, sub);
}

fn check_track_numerics_mirrors(
    c: &mut SanityCollector,
    n: &TrackNumerics,
    ctx: &str,
    sub: Option<&str>,
) {
    if n.cloud_saga_id_1 != 0 && n.cloud_saga_id_2 != 0 && n.cloud_saga_id_1 != n.cloud_saga_id_2 {
        c.check_mirror_mismatch(
            "TrackNumerics",
            "328..336",
            "cloud_saga_id_2",
            &format!("{:#x}", n.cloud_saga_id_1),
            &format!("{:#x}", n.cloud_saga_id_2),
            ctx,
            sub.map(str::to_string),
        );
    }
    let adam_fields = [
        ("304..312", "store_track_adam_id", n.store_track_adam_id),
        ("320..328", "store_track_adam_id_2", n.store_track_adam_id_2),
        ("340..348", "store_track_adam_id_3", n.store_track_adam_id_3),
        ("348..356", "store_track_adam_id_4", n.store_track_adam_id_4),
    ];
    if let Some((_, _, primary)) = adam_fields.iter().find(|(_, _, v)| *v != 0) {
        for (offset, field, val) in adam_fields {
            if val != 0 && val != *primary {
                c.check_store_id_divergence(
                    "TrackNumerics",
                    offset,
                    field,
                    *primary,
                    val,
                    ctx,
                    sub.map(str::to_string),
                );
            }
        }
    }
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

fn unwrap_pre_1904_for_year(ts: AppleTimestamp, track_year: u32) -> Option<Timestamp> {
    const APPLE_TO_UNIX_EPOCH_OFFSET: i64 = 2_082_844_800;
    let raw = ts.raw();
    if !(APPLE_SENTINEL_CUTOFF..=u32::MAX - APPLE_SENTINEL_CUTOFF).contains(&raw) {
        return None;
    }
    if !(1..1904).contains(&track_year) {
        return None;
    }
    let signed_apple_secs = i64::from(raw) - (1i64 << 32);
    let unix_secs = signed_apple_secs - APPLE_TO_UNIX_EPOCH_OFFSET;
    let unwrapped = Timestamp::from_second(unix_secs).ok()?;
    let unwrapped_year = i32::from(jiff::tz::Offset::UTC.to_datetime(unwrapped).year());
    (i32::try_from(track_year).ok() == Some(unwrapped_year)).then_some(unwrapped)
}

fn check_track_numerics_timestamps(
    c: &mut SanityCollector,
    n: &TrackNumerics,
    ctx: &str,
    sub: Option<String>,
    track_year: Option<u32>,
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
    if let Some(year) = track_year
        && let Some(unwrapped) = unwrap_pre_1904_for_year(n.date_released, year)
    {
        c.record(Finding {
            category: SanityCategory::Timestamp,
            severity: SanitySeverity::Info,
            rule_id: "timestamp.pre_1904_underflow",
            section: "TrackNumerics",
            offset: "140..144",
            field: "date_released",
            message: "pre-1904 release date wrapped modulo 2^32 in unsigned AppleTimestamp",
            value: format!(
                "{unwrapped} (raw={}, unsigned={})",
                n.date_released.raw(),
                n.date_released
            ),
            item_context: ctx,
            subcontext: sub.clone(),
        });
    } else {
        c.check_timestamp(
            "TrackNumerics",
            "140..144",
            "date_released",
            n.date_released,
            TimestampSpec::UTC_RELEASE,
            ctx,
            sub.clone(),
        );
    }
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

fn check_track_plays_skips(
    c: &mut SanityCollector,
    p: &TrackPlaysSkips,
    ctx: &str,
    parent_track_id: Option<u64>,
) {
    if let Some(expected) = parent_track_id
        && expected != 0
        && p.track_persistent_id != 0
        && p.track_persistent_id != expected
    {
        c.check_mirror_mismatch(
            "TrackPlaysSkips",
            "0..8",
            "track_persistent_id",
            &PersistentId(expected).to_string(),
            &PersistentId(p.track_persistent_id).to_string(),
            ctx,
            None,
        );
    }
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

fn check_playlist(c: &mut SanityCollector, playlist: &PlaylistItem, ids: &LibraryIdIndex) {
    let h = &playlist.header;
    let ctx = format_item_context(
        "lpma",
        h.playlist_persistent_id,
        find_boma_str(&playlist.bomas, 0x00C8),
        None,
    );
    check_playlist_header_fields(c, playlist, &ctx);
    check_playlist_states_mirrors_and_refs(c, &playlist.header, ids, &ctx);
    check_playlist_timestamps(c, playlist, &ctx);
    check_bomas(c, "lpma", &playlist.bomas, &ctx, Some(ids), None);
}

fn check_playlist_states_mirrors_and_refs(
    c: &mut SanityCollector,
    h: &musicdb::sections::PlaylistItemHeader,
    ids: &LibraryIdIndex,
    ctx: &str,
) {
    check_artwork_states(
        c,
        "lpma",
        ArtworkStateFields {
            source_offset: "296..300",
            source_kind: h.artwork_source_kind,
            cache_offset: "300..304",
            cache_state: h.artwork_cache_state,
            size_offset: "316..320",
            size_kind: h.artwork_size_kind,
            format_offset: "320..324",
            image_format: h.artwork_image_format,
        },
        ctx,
    );
    if h.is_master_playlist == 0
        && h.playlist_persistent_id_2 != 0
        && h.playlist_persistent_id_2 != h.playlist_persistent_id
    {
        c.check_mirror_mismatch(
            "lpma",
            "280..288",
            "playlist_persistent_id_2",
            &PersistentId(h.playlist_persistent_id).to_string(),
            &PersistentId(h.playlist_persistent_id_2).to_string(),
            ctx,
            None,
        );
    }
    let primary_saga = if h.cloud_playlist_saga_id_1 != 0 {
        h.cloud_playlist_saga_id_1
    } else {
        h.cloud_playlist_saga_id_2
    };
    if primary_saga != 0 {
        if h.cloud_playlist_saga_id_2 != 0 && h.cloud_playlist_saga_id_2 != primary_saga {
            c.check_mirror_mismatch(
                "lpma",
                "196..204",
                "cloud_playlist_saga_id_2",
                &format!("{primary_saga:#x}"),
                &format!("{:#x}", h.cloud_playlist_saga_id_2),
                ctx,
                None,
            );
        }
        if h.cloud_playlist_saga_id_3 != 0 && u64::from(h.cloud_playlist_saga_id_3) != primary_saga
        {
            c.check_mirror_mismatch(
                "lpma",
                "218..222",
                "cloud_playlist_saga_id_3",
                &format!("{primary_saga:#x}"),
                &format!("{:#x}", h.cloud_playlist_saga_id_3),
                ctx,
                None,
            );
        }
    }
    check_playlist_refs(c, h, ids, ctx);
}

fn check_playlist_refs(
    c: &mut SanityCollector,
    h: &musicdb::sections::PlaylistItemHeader,
    ids: &LibraryIdIndex,
    ctx: &str,
) {
    if h.parent_persistent_id != 0 {
        if !ids.playlists.contains(&h.parent_persistent_id) {
            c.check_dangling_ref(
                "lpma",
                "50..58",
                "parent_persistent_id",
                "playlist parent_persistent_id does not exist in lPma playlist list",
                h.parent_persistent_id,
                ctx,
                None,
            );
        } else if !ids.folders.contains(&h.parent_persistent_id) {
            c.check_dangling_ref(
                "lpma",
                "50..58",
                "parent_persistent_id",
                "playlist parent_persistent_id references a non-folder playlist",
                h.parent_persistent_id,
                ctx,
                None,
            );
        }
    }
    if let Some(author_id) = h.author_social_profile_persistent_id
        && author_id != 0
        && !ids.profiles.contains(&author_id)
    {
        c.check_dangling_ref(
            "lpma",
            "340..348",
            "author_social_profile_persistent_id",
            "playlist author_social_profile_persistent_id does not exist in Lsma profile list",
            author_id,
            ctx,
            None,
        );
    }
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
    c.check_bool(
        "lpma",
        "214..218",
        "is_cloud_synced",
        h.is_cloud_synced,
        ctx,
        None,
    );
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
    let bools: [(&str, &str, u8); 6] = [
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
        ("14", "is_genius", opts.is_genius),
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

fn check_slst(
    c: &mut SanityCollector,
    slst: &SlstSection,
    ctx: &str,
    prefix: &str,
    ids: Option<&LibraryIdIndex>,
) {
    let slst_sub = (!prefix.is_empty()).then(|| prefix.to_string());
    if !matches!(slst.conjunction, 0 | 1) {
        c.check_state_value(
            "SLst",
            "12..16",
            "conjunction",
            slst.conjunction.to_string(),
            ctx,
            slst_sub,
        );
    }
    for (idx, rule) in slst.rules.iter().enumerate() {
        let sub = if prefix.is_empty() {
            format!("SLst::rule[{idx}]")
        } else {
            format!("{prefix}.rule[{idx}]")
        };
        check_smart_rule(c, rule, ctx, &sub, ids);
    }
}

fn check_smart_rule(
    c: &mut SanityCollector,
    rule: &SmartPlaylistRule,
    ctx: &str,
    sub: &str,
    ids: Option<&LibraryIdIndex>,
) {
    if !matches!(rule.is_nested_rule_group, 0 | 0x0100_0000) {
        c.check_state_value(
            "SmartPlaylistRule",
            "8..12",
            "is_nested_rule_group",
            format!("{:#010x}", rule.is_nested_rule_group),
            ctx,
            Some(sub.to_string()),
        );
    }
    if rule.field_id == 0
        && let SmartRuleChild::Nested(nested) = &rule.child
    {
        check_slst(c, nested, ctx, sub, ids);
        return;
    }
    let field = SmartField::from(rule.field_id);
    if matches!(field, SmartField::Unknown(_)) {
        c.check_enum_fallthrough(
            "SmartPlaylistRule",
            "0..4",
            "field_id",
            format!("{:#x}", rule.field_id),
            ctx,
            Some(sub.to_string()),
        );
    }
    let operator = SmartOperator::from(rule.comparison_method);
    if matches!(operator, SmartOperator::Unknown(_)) {
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
    check_smart_operator_combination(c, rule.comparison_method, ctx, sub);
    match &rule.child {
        SmartRuleChild::Nested(nested) => check_slst(c, nested, ctx, sub, ids),
        SmartRuleChild::Arguments(args) => {
            check_smart_rule_args(c, field, operator, args, ctx, sub, ids);
        }
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
        SmartRuleChild::StringUtf16Be { .. } | _ => {}
    }
}

fn check_smart_operator_combination(c: &mut SanityCollector, raw: u32, ctx: &str, sub: &str) {
    let flags = SmartOperatorFlags::from_bits_retain(raw);
    let action_mask = SmartOperatorFlags::IS
        | SmartOperatorFlags::CONTAINS
        | SmartOperatorFlags::STARTS_WITH
        | SmartOperatorFlags::ENDS_WITH
        | SmartOperatorFlags::GREATER_THAN
        | SmartOperatorFlags::GREATER_OR_EQ
        | SmartOperatorFlags::LESS_THAN
        | SmartOperatorFlags::LESS_OR_EQ
        | SmartOperatorFlags::IN_RANGE
        | SmartOperatorFlags::IN_THE_LAST
        | SmartOperatorFlags::BITWISE_SET;
    let action = flags & action_mask;
    if action.bits().count_ones() != 1 {
        c.check_bitflag_combination(
            "SmartPlaylistRule",
            "4..8",
            "comparison_method",
            "SmartOperatorFlags must set exactly one comparison action bit",
            format!("{raw:#010x}"),
            ctx,
            Some(sub.to_string()),
        );
        return;
    }
    let string_only = SmartOperatorFlags::CONTAINS
        | SmartOperatorFlags::STARTS_WITH
        | SmartOperatorFlags::ENDS_WITH;
    let numeric_only = SmartOperatorFlags::GREATER_THAN
        | SmartOperatorFlags::GREATER_OR_EQ
        | SmartOperatorFlags::LESS_THAN
        | SmartOperatorFlags::LESS_OR_EQ
        | SmartOperatorFlags::IN_RANGE
        | SmartOperatorFlags::IN_THE_LAST
        | SmartOperatorFlags::BITWISE_SET;
    if flags.contains(SmartOperatorFlags::STRING_MODE) && action.intersects(numeric_only) {
        c.check_bitflag_combination(
            "SmartPlaylistRule",
            "4..8",
            "comparison_method",
            "STRING_MODE set with numeric/range/relative/bitwise operator",
            format!("{raw:#010x}"),
            ctx,
            Some(sub.to_string()),
        );
    } else if !flags.contains(SmartOperatorFlags::STRING_MODE) && action.intersects(string_only) {
        c.check_bitflag_combination(
            "SmartPlaylistRule",
            "4..8",
            "comparison_method",
            "string operator missing STRING_MODE flag",
            format!("{raw:#010x}"),
            ctx,
            Some(sub.to_string()),
        );
    }
}

fn check_smart_rule_args(
    c: &mut SanityCollector,
    field: SmartField,
    operator: SmartOperator,
    args: &SmartPlaylistRuleArguments,
    ctx: &str,
    sub: &str,
    ids: Option<&LibraryIdIndex>,
) {
    if matches!(
        operator,
        SmartOperator::InTheLast | SmartOperator::NotInTheLast
    ) {
        check_slst_relative_rule(c, args, ctx, sub);
        return;
    }

    if matches!(
        field,
        SmartField::DateAdded
            | SmartField::DateModified
            | SmartField::LastPlayed
            | SmartField::LastSkipped
    ) {
        check_slst_absolute_date(c, "0..8", "arg_0", args.arg_0, ctx, sub);
        if matches!(operator, SmartOperator::InRange | SmartOperator::NotInRange) {
            check_slst_absolute_date(c, "24..32", "arg_3", args.arg_3, ctx, sub);
            if args.arg_0 > args.arg_3 {
                c.record(Finding {
                    category: SanityCategory::Timestamp,
                    severity: SanitySeverity::Warning,
                    rule_id: "timestamp.slst_invalid",
                    section: "SmartPlaylistRuleArguments",
                    offset: "0..8,24..32",
                    field: "arg_0/arg_3",
                    message: "SLst date range start (arg_0) exceeds end (arg_3)",
                    value: format!("{} > {}", args.arg_0, args.arg_3),
                    item_context: ctx,
                    subcontext: Some(sub.to_string()),
                });
            }
        }
    } else if field == SmartField::Playlist
        && let Some(ids) = ids
    {
        let target = args.arg_0.cast_unsigned();
        if target != 0 && !ids.playlists.contains(&target) {
            c.check_dangling_ref(
                "SmartPlaylistRuleArguments",
                "0..8",
                "arg_0",
                "SLst Playlist rule references persistent ID absent from lPma",
                target,
                ctx,
                Some(sub.to_string()),
            );
        }
    }
}

fn check_slst_relative_rule(
    c: &mut SanityCollector,
    args: &SmartPlaylistRuleArguments,
    ctx: &str,
    sub: &str,
) {
    if args.arg_1 >= 0 {
        c.record(Finding {
            category: SanityCategory::Timestamp,
            severity: SanitySeverity::Warning,
            rule_id: "timestamp.slst_invalid",
            section: "SmartPlaylistRuleArguments",
            offset: "8..16",
            field: "arg_1",
            message: "relative time rule expects negative offset in arg_1",
            value: args.arg_1.to_string(),
            item_context: ctx,
            subcontext: Some(sub.to_string()),
        });
    }
    let unit = match args.arg_2 {
        86_400 | 1 => RelativeTimeUnit::Days,
        604_800 | 2 => RelativeTimeUnit::Weeks,
        2_628_000 | 3 => RelativeTimeUnit::Months,
        other => RelativeTimeUnit::Unknown(other),
    };
    if matches!(unit, RelativeTimeUnit::Unknown(_)) {
        c.check_enum_fallthrough(
            "SmartPlaylistRuleArguments",
            "16..24",
            "arg_2",
            args.arg_2.to_string(),
            ctx,
            Some(sub.to_string()),
        );
    }
}

fn check_slst_absolute_date(
    c: &mut SanityCollector,
    offset: &str,
    field: &str,
    raw: i64,
    ctx: &str,
    sub: &str,
) {
    if raw < i64::from(APPLE_SENTINEL_CUTOFF) || raw > i64::from(u32::MAX) {
        c.record(Finding {
            category: SanityCategory::Timestamp,
            severity: SanitySeverity::Warning,
            rule_id: "timestamp.slst_invalid",
            section: "SmartPlaylistRuleArguments",
            offset,
            field,
            message: "SLst absolute date argument is below Apple ground-zero cutoff or out of u32 range",
            value: raw.to_string(),
            item_context: ctx,
            subcontext: Some(sub.to_string()),
        });
    } else if let Ok(raw_u32) = u32::try_from(raw) {
        c.check_timestamp(
            "SmartPlaylistRuleArguments",
            offset,
            field,
            AppleTimestamp::new(raw_u32),
            TimestampSpec::UTC_LEASE,
            ctx,
            Some(sub.to_string()),
        );
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
    let h = &profile.header;
    let ctx = format_item_context(
        "isma",
        h.profile_persistent_id,
        find_boma_str(&profile.bomas, 0x0321),
        find_boma_str(&profile.bomas, 0x0322),
    );
    check_artwork_states(
        c,
        "isma",
        ArtworkStateFields {
            source_offset: "48..52",
            source_kind: h.artwork_source_kind,
            cache_offset: "52",
            cache_state: u32::from(h.artwork_cache_state),
            size_offset: "56..60",
            size_kind: h.artwork_size_kind,
            format_offset: "60..64",
            image_format: h.artwork_image_format,
        },
        &ctx,
    );
    check_bomas(c, "isma", &profile.bomas, &ctx, None, None);
}

#[derive(Clone, Copy)]
struct ArtworkStateFields<'a> {
    source_offset: &'a str,
    source_kind: u32,
    cache_offset: &'a str,
    cache_state: u32,
    size_offset: &'a str,
    size_kind: u32,
    format_offset: &'a str,
    image_format: u32,
}

fn check_artwork_states(
    c: &mut SanityCollector,
    section: &str,
    f: ArtworkStateFields<'_>,
    ctx: &str,
) {
    if !matches!(f.source_kind, 0 | 6 | 12 | 13 | 45 | 63 | 70 | 101 | 102) {
        c.check_state_value(
            section,
            f.source_offset,
            "artwork_source_kind",
            f.source_kind.to_string(),
            ctx,
            None,
        );
    }
    if !matches!(f.cache_state, 0 | 3 | 5 | 8 | 10 | 12) {
        c.check_state_value(
            section,
            f.cache_offset,
            "artwork_cache_state",
            f.cache_state.to_string(),
            ctx,
            None,
        );
    }
    if !matches!(f.size_kind, 0 | 1) {
        c.check_state_value(
            section,
            f.size_offset,
            "artwork_size_kind",
            f.size_kind.to_string(),
            ctx,
            None,
        );
    }
    if !matches!(f.image_format, 0..=2) {
        c.check_state_value(
            section,
            f.format_offset,
            "artwork_image_format",
            f.image_format.to_string(),
            ctx,
            None,
        );
    }
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
    if raw == SuggestionFlags::LOCALLY_MODIFIED.bits() {
        c.check_bitflag_combination(
            section,
            offset,
            "suggestion_flag",
            "LOCALLY_MODIFIED bit is set with no suggestion state",
            format!("{raw:#06x}"),
            ctx,
            None,
        );
    }
}

#[derive(Clone, Copy)]
struct ParentTrackContext {
    persistent_id: u64,
    year: u32,
}

fn check_bomas(
    c: &mut SanityCollector,
    parent: &str,
    bomas: &[BomaSection],
    ctx: &str,
    ids: Option<&LibraryIdIndex>,
    parent_track: Option<ParentTrackContext>,
) {
    let mut numerics_idx = 0usize;
    for boma in bomas {
        match &boma.payload {
            BomaPayload::TrackNumerics(n) => {
                let sub = Some(format!(
                    "TrackNumerics[{numerics_idx}] (subrecord_kind={})",
                    n.subrecord_kind
                ));
                numerics_idx += 1;
                check_track_numerics(c, n, ctx, sub, parent_track.map(|t| t.year));
            }
            BomaPayload::TrackPlaysSkips(p) => {
                check_track_plays_skips(c, p, ctx, parent_track.map(|t| t.persistent_id));
            }
            BomaPayload::VideoInfo(v) => check_video_info(c, v, ctx),
            BomaPayload::SmartPlaylistOptions(opts) => check_smart_options(c, opts, ctx),
            BomaPayload::SmartPlaylistRules(slst) => check_slst(c, slst, ctx, "", ids),
            BomaPayload::PlaylistItem(ipfa) => check_boma_playlist_item(c, ipfa, ids, ctx),
            BomaPayload::PlaylistItemCompact(ipfa) => {
                check_boma_playlist_item_compact(c, ipfa.track_persistent_id, ids, ctx);
            }
            BomaPayload::StringSection(s) => {
                if !matches!(s.encoding, 1 | 2) {
                    let section = format!("{parent}::boma({:#06x})", boma.subtype);
                    c.check_state_value(
                        &section,
                        "0..4",
                        "encoding",
                        s.encoding.to_string(),
                        ctx,
                        None,
                    );
                }
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
            } => check_boma_book(
                c,
                parent,
                boma.subtype,
                extracted_path.as_deref(),
                components.len(),
                raw_bytes.len(),
                ctx,
            ),
            BomaPayload::Unknown { .. } | _ => {}
        }
    }
}

fn check_boma_playlist_item_compact(
    c: &mut SanityCollector,
    track_persistent_id: u64,
    ids: Option<&LibraryIdIndex>,
    ctx: &str,
) {
    if let Some(ids) = ids
        && track_persistent_id != 0
        && !ids.tracks.contains(&track_persistent_id)
    {
        c.check_dangling_ref(
            "IPfa",
            "16..24",
            "track_persistent_id",
            "compact playlist item track_persistent_id does not exist in ltma track list",
            track_persistent_id,
            ctx,
            None,
        );
    }
}

fn check_boma_playlist_item(
    c: &mut SanityCollector,
    ipfa: &musicdb::sections::PlaylistTrackItem,
    ids: Option<&LibraryIdIndex>,
    ctx: &str,
) {
    if ipfa.item_persistent_id_2 != 0 && ipfa.item_persistent_id_2 != ipfa.item_persistent_id {
        c.check_mirror_mismatch(
            "ipfa",
            "44..52",
            "item_persistent_id_2",
            &PersistentId(ipfa.item_persistent_id).to_string(),
            &PersistentId(ipfa.item_persistent_id_2).to_string(),
            ctx,
            None,
        );
    }
    if let Some(ids) = ids
        && ipfa.track_persistent_id != 0
        && !ids.tracks.contains(&ipfa.track_persistent_id)
    {
        c.check_dangling_ref(
            "ipfa",
            "20..28",
            "track_persistent_id",
            "playlist item track_persistent_id does not exist in ltma track list",
            ipfa.track_persistent_id,
            ctx,
            None,
        );
    }
}

fn check_boma_book(
    c: &mut SanityCollector,
    parent: &str,
    subtype: u32,
    extracted_path: Option<&str>,
    component_count: usize,
    raw_len: usize,
    ctx: &str,
) {
    if extracted_path.is_none_or(str::is_empty) || component_count == 0 {
        let section = format!("{parent}::boma({subtype:#06x})");
        let field = musicdb::sections::boma::boma_subtype_name(subtype);
        c.record(Finding {
            category: SanityCategory::StringData,
            severity: SanitySeverity::Warning,
            rule_id: "string.empty_book_path",
            section: &section,
            offset: "payload",
            field,
            message: "macOS book bookmark payload yielded no extracted path or components",
            value: format!("len={raw_len}, path={extracted_path:?}, components={component_count}"),
            item_context: ctx,
            subcontext: None,
        });
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
    match subtype {
        0x01F8 => c.check_file_url(section, "payload", field, text, ctx, None),
        0x000B => {
            if text.starts_with("http://") || text.starts_with("https://") {
                c.check_http_url(section, "payload", field, text, ctx, None);
            } else {
                c.check_file_url(section, "payload", field, text, ctx, None);
            }
        }
        0x0324 => c.check_http_url(section, "payload", field, text, ctx, None),
        _ => {}
    }
}

fn check_xml_plist(
    c: &mut SanityCollector,
    parent: &str,
    subtype: u32,
    raw_bytes: &[u8],
    ctx: &str,
) {
    let section = format!("{parent}::boma({subtype:#06x})");
    let field = musicdb::sections::boma::boma_subtype_name(subtype);
    match plist::Value::from_reader_xml(Cursor::new(raw_bytes)) {
        Ok(val) => {
            if matches!(subtype, 0x0036 | 0x00CD | 0x0135 | 0x0192) {
                check_plist_urls(c, &section, field, &val, ctx, "");
            }
        }
        Err(err) => {
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
}

fn check_binary_plist(
    c: &mut SanityCollector,
    parent: &str,
    subtype: u32,
    raw_bytes: &[u8],
    ctx: &str,
) {
    let section = format!("{parent}::boma({subtype:#06x})");
    let field = musicdb::sections::boma::boma_subtype_name(subtype);
    match plist::Value::from_reader(Cursor::new(raw_bytes)) {
        Ok(val) => {
            if matches!(subtype, 0x0036 | 0x00CD | 0x0135 | 0x0192) {
                check_plist_urls(c, &section, field, &val, ctx, "");
            }
        }
        Err(err) => {
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
}

fn check_plist_urls(
    c: &mut SanityCollector,
    section: &str,
    field: &str,
    value: &plist::Value,
    ctx: &str,
    key_path: &str,
) {
    match value {
        plist::Value::Dictionary(dict) => {
            for (k, v) in dict {
                let next_path = if key_path.is_empty() {
                    k.clone()
                } else {
                    format!("{key_path}.{k}")
                };
                if k.to_ascii_lowercase().contains("url")
                    && let plist::Value::String(s) = v
                    && !s.is_empty()
                {
                    let sub = Some(next_path);
                    if s.starts_with("file://") {
                        c.check_file_url(section, "payload", field, s, ctx, sub);
                    } else {
                        c.check_http_url(section, "payload", field, s, ctx, sub);
                    }
                    continue;
                }
                check_plist_urls(c, section, field, v, ctx, &next_path);
            }
        }
        plist::Value::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                let next_path = format!("{key_path}[{idx}]");
                check_plist_urls(c, section, field, item, ctx, &next_path);
            }
        }
        _ => {}
    }
}

fn validate_http_url(url: &str) -> Result<(), &'static str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("HTTP(S) URL does not start with http:// or https://")?;
    if url.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err("HTTP(S) URL contains whitespace or control characters");
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err("HTTP(S) URL is missing authority/host");
    }
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
    if host_port.is_empty() {
        return Err("HTTP(S) URL has empty host");
    }
    let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
        let Some((ipv6, after)) = bracketed.split_once(']') else {
            return Err("HTTP(S) URL has unterminated IPv6 bracket");
        };
        let port = after.strip_prefix(':');
        if !after.is_empty() && port.is_none() {
            return Err("HTTP(S) URL has invalid suffix after IPv6 host");
        }
        (ipv6, port)
    } else if let Some((h, p)) = host_port.rsplit_once(':') {
        (h, Some(p))
    } else {
        (host_port, None)
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'))
    {
        return Err("HTTP(S) URL contains invalid host characters");
    }
    if let Some(p) = port {
        match p.parse::<u16>() {
            Ok(n) if n > 0 => {}
            _ => return Err("HTTP(S) URL contains invalid port number"),
        }
    }
    Ok(())
}

fn validate_file_url(url: &str) -> Result<(), &'static str> {
    let rest = url
        .strip_prefix("file://")
        .ok_or("file URL does not start with file://")?;
    if url.chars().any(char::is_control) {
        return Err("file URL contains control characters");
    }
    if rest.is_empty() {
        return Err("file URL has empty path");
    }
    let path = if rest.starts_with('/') {
        rest
    } else if let Some((host, after)) = rest.split_once('/') {
        if host.is_empty()
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return Err("file URL contains invalid authority host");
        }
        after
    } else {
        return Err("file URL is missing path separator after authority");
    };
    if path.is_empty() {
        return Err("file URL has empty path");
    }
    Ok(())
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
        // Unrecognized underflow (-7200 wrapped as u32) -> Warning (timestamp.underflow_sentinel).
        c.check_timestamp(
            "TrackPlaysSkips",
            "8..12",
            "date_last_played",
            AppleTimestamp::new(0u32.wrapping_sub(7200)),
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
    fn dst_zero_sentinel_classified_as_info_on_local_only() {
        let opts = fixed_opts(true);
        let mut c = SanityCollector::new(&opts, -10_800);

        // ±3600 on LOCAL fields -> Info (timestamp.dst_zero_sentinel), 0 warnings.
        c.check_timestamp(
            "TrackPlaysSkips",
            "8..12",
            "date_last_played",
            AppleTimestamp::new(3600),
            TimestampSpec::LOCAL,
            "itma[2112]",
            None,
        );
        c.check_timestamp(
            "iama",
            "100..104",
            "date_last_played",
            AppleTimestamp::new(0u32.wrapping_sub(3600)),
            TimestampSpec::LOCAL,
            "iama[2112]",
            None,
        );
        assert_eq!(c.total_warnings, 0);
        assert_eq!(c.total_info, 2);

        // ±3600 on UTC fields and non-3600 underflow on LOCAL must still be Warnings.
        c.check_timestamp(
            "TrackPlaysSkips",
            "20..24",
            "date_first_played_utc",
            AppleTimestamp::new(3600),
            TimestampSpec::UTC,
            "itma[2112]",
            None,
        );
        c.check_timestamp(
            "TrackPlaysSkips",
            "20..24",
            "date_first_played_utc",
            AppleTimestamp::new(0u32.wrapping_sub(3600)),
            TimestampSpec::UTC,
            "itma[2112]",
            None,
        );
        c.check_timestamp(
            "TrackPlaysSkips",
            "8..12",
            "date_last_played",
            AppleTimestamp::new(0u32.wrapping_sub(7200)),
            TimestampSpec::LOCAL,
            "itma[2112]",
            None,
        );
        let report = c.finish(&opts, &HfmaHeader::default());
        assert_eq!(report.total_warnings, 3);
        assert_eq!(report.total_info, 2);
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"timestamp.dst_zero_sentinel"));
        assert!(rules.contains(&"timestamp.unknown_sentinel"));
        assert!(rules.contains(&"timestamp.underflow_sentinel"));
    }

    #[test]
    fn pre_1904_wrapped_release_date_classified_as_info() {
        // Wrapped pre-1904 date_released matching track.year == 1900 -> Info (timestamp.pre_1904_underflow).
        // Same raw timestamp on a 1981 track -> Warning (timestamp.far_future).
        let mut raw = build_minimal_rush_raw();
        let make_num_boma = |raw_release: u32| -> BomaSection {
            let mut raw_boma = vec![0u8; 20 + 364];
            raw_boma[0..4].copy_from_slice(b"boma");
            raw_boma[4..8].copy_from_slice(&20u32.to_le_bytes());
            raw_boma[8..12].copy_from_slice(&(20u32 + 364).to_le_bytes());
            raw_boma[12..16].copy_from_slice(&0x0001u32.to_le_bytes());
            raw_boma[20 + 4..20 + 8].copy_from_slice(&3u32.to_le_bytes());
            raw_boma[20 + 140..20 + 144].copy_from_slice(&raw_release.to_le_bytes());
            binrw::BinRead::read_le(&mut Cursor::new(&raw_boma)).unwrap()
        };
        for sec in &mut raw.sections {
            if let HsmaChild::TrackList(ltma) = &mut sec.child {
                // 4_195_218_496 -> unsigned 2036-12-08T18:28:16Z (>10y past 2026-11-22),
                // signed unwrapped = 1900-11-02T12:00:00Z.
                let track_1900 = &mut ltma.tracks[0];
                track_1900.header.year = 1900;
                track_1900.bomas.push(make_num_boma(4_195_218_496));

                let mut track_1981 = track_1900.clone();
                track_1981.header.track_persistent_id = 0x2112_1981;
                track_1981.header.year = 1981;
                ltma.tracks.push(track_1981);
                ltma.header.track_count = 2;
            }
        }
        raw.outer_header.track_count = 2;

        let mut ts_opts = fixed_opts(true);
        ts_opts.categories = vec![SanityCategory::Timestamp];
        let ts_report = analyze_sanity(&raw, &ts_opts);
        assert_eq!(ts_report.total_warnings, 1);
        assert_eq!(ts_report.total_info, 1);
        let ts_rules: Vec<&str> = ts_report.findings.iter().map(|g| g.rule_id).collect();
        assert!(ts_rules.contains(&"timestamp.pre_1904_underflow"));
        assert!(ts_rules.contains(&"timestamp.far_future"));
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
        check_track_numerics(&mut c, &num, "itma[2112]", None, None);
        let mut ps = TrackPlaysSkips::default();
        ps.play_count = 5;
        ps.true_play_count = 2;
        ps.skip_count = 4;
        ps.true_skip_count = 1;
        check_track_plays_skips(&mut c, &ps, "itma[2112]", None);
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
        check_bomas(&mut c, "itma", &[empty_book], "itma[2112]", None, None);

        let sub_report = c.finish(&sub_opts, &HfmaHeader::default());
        let sub_rules: Vec<&str> = sub_report.findings.iter().map(|g| g.rule_id).collect();
        assert!(sub_rules.contains(&"bounds.sample_rate"));
        assert!(sub_rules.contains(&"bounds.count_exceeds_true"));
        assert!(sub_rules.contains(&"string.weird_character_density"));
        assert!(sub_rules.contains(&"string.empty_book_path"));
    }

    #[test]
    fn tier3_url_validation_covers_http_file_and_plists() {
        // Valid URLs including Apple artwork template and Windows file URL.
        assert!(
            validate_http_url("https://is1-ssl.mzstatic.com/image/thumb/Rush/{w}x{h}bb.{f}")
                .is_ok()
        );
        assert!(validate_http_url("http://127.0.0.1:8080/moving-pictures").is_ok());
        assert!(validate_file_url("file://localhost/C:/Music/Rush/Limelight.m4a").is_ok());
        assert!(validate_file_url("file:///Users/geddy/Music/Rush/Tom%20Sawyer.m4a").is_ok());

        // Malformed URLs.
        assert!(validate_http_url("ftp://rush.example/yYZ").is_err());
        assert!(validate_http_url("https://bad host/limelight").is_err());
        assert!(validate_http_url("https://example.com:99999/rush").is_err());
        assert!(validate_file_url("C:/Music/Rush/YYZ.m4a").is_err());
        assert!(validate_file_url("file://").is_err());

        // Check boma string URLs and XML + binary plist URL traversal.
        let mut opts = fixed_opts(false);
        opts.categories = vec![SanityCategory::Url];
        let mut c = SanityCollector::new(&opts, 0);

        check_boma_string(
            &mut c,
            "plma",
            0x01F8,
            "not-a-file-url",
            b"not-a-file-url",
            false,
            "plma[2112]",
        );
        check_boma_string(
            &mut c,
            "itma",
            0x000B,
            "https://",
            b"https://",
            false,
            "itma[2112]",
        );
        check_boma_string(
            &mut c,
            "isma",
            0x0324,
            "ftp://bad/avatar",
            b"ftp://bad/avatar",
            false,
            "isma[2112]",
        );

        let xml_plist = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>goodURL</key><string>https://is1-ssl.mzstatic.com/image/{w}x{h}bb.{f}</string>
  <key>badPreviewURL</key><string>http://bad host/rush</string>
</dict></plist>"#;
        check_xml_plist(&mut c, "itma", 0x0036, xml_plist, "itma[2112]");

        let mut dict = plist::Dictionary::new();
        dict.insert(
            "artworkUrl".into(),
            plist::Value::String("not-http://broken".into()),
        );
        let mut bin_buf = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut bin_buf)
            .unwrap();
        check_binary_plist(&mut c, "iama", 0x0135, &bin_buf, "iama[2112]");

        let report = c.finish(&opts, &HfmaHeader::default());
        assert_eq!(report.total_warnings, 5);
        assert!(
            report
                .findings
                .iter()
                .all(|g| g.category == SanityCategory::Url)
        );
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"url.invalid_file_url"));
        assert!(rules.contains(&"url.invalid_http_url"));
    }

    #[test]
    fn tier3_bitflag_combinations() {
        let opts = fixed_opts(false);
        let mut c = SanityCollector::new(&opts, 0);

        check_audio_traits_combination(
            &mut c,
            "itma",
            "331",
            "audio_traits",
            u32::from(AudioTraits::HI_RES_LOSSLESS.bits()),
            "itma[2112]",
            None,
        );
        check_audio_traits_combination(
            &mut c,
            "itma",
            "331",
            "audio_traits",
            u32::from(AudioTraits::SPATIAL_AUDIO.bits()),
            "itma[2112]",
            None,
        );
        check_track_sort_slot(&mut c, 0, 0x03, 0x001E, false, "itma[2112]");
        check_track_sort_slot(&mut c, 1, 0x02, 0x001F, false, "itma[2112]");
        check_track_sort_slot(&mut c, 2, 0x04, 0x0020, false, "itma[2112]");
        check_suggestion_flag(&mut c, "itma", "62..64", 0x0100, "itma[2112]");
        check_smart_operator_combination(&mut c, 0, "lpma[2112]", "SLst::rule[0]");
        check_smart_operator_combination(
            &mut c,
            (SmartOperatorFlags::IS | SmartOperatorFlags::CONTAINS).bits(),
            "lpma[2112]",
            "SLst::rule[1]",
        );
        check_smart_operator_combination(
            &mut c,
            (SmartOperatorFlags::STRING_MODE | SmartOperatorFlags::GREATER_THAN).bits(),
            "lpma[2112]",
            "SLst::rule[2]",
        );
        check_smart_operator_combination(
            &mut c,
            SmartOperatorFlags::CONTAINS.bits(),
            "lpma[2112]",
            "SLst::rule[3]",
        );

        let report = c.finish(&opts, &HfmaHeader::default());
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"bitflag.invalid_combination"));
        assert!(report.total_warnings >= 10);
    }

    #[test]
    fn tier3_state_fallthroughs_and_bools() {
        let opts = fixed_opts(false);
        let mut c = SanityCollector::new(&opts, 0);

        let mut num = TrackNumerics::default();
        num.subrecord_kind = 3;
        num.downloaded_audio_traits = u32::from(AudioTraits::LOSSLESS.bits());
        num.asset_container_kind = 99;
        num.catalog_lyrics_state = 9;
        num.container_format_kind = 77;
        num.has_embedded_artwork = 42;
        num.local_artwork_state = 5;
        num.drm_platform = 9;
        num.cloud_codec_kind = 9;
        check_track_numerics(&mut c, &num, "itma[2112]", None, None);

        // Artwork state: 101 is valid (custom track artwork), 999 is flagged.
        let before_valid_art = c.total_warnings;
        check_artwork_states(
            &mut c,
            "itma",
            ArtworkStateFields {
                source_offset: "308..312",
                source_kind: 101,
                cache_offset: "328",
                cache_state: 3,
                size_offset: "344..348",
                size_kind: 1,
                format_offset: "348..352",
                image_format: 2,
            },
            "itma[2112]",
        );
        assert_eq!(c.total_warnings, before_valid_art);
        check_artwork_states(
            &mut c,
            "itma",
            ArtworkStateFields {
                source_offset: "308..312",
                source_kind: 999,
                cache_offset: "328",
                cache_state: 99,
                size_offset: "344..348",
                size_kind: 9,
                format_offset: "348..352",
                image_format: 9,
            },
            "itma[2112]",
        );

        let mut smart_opts = SmartPlaylistOptions::default();
        smart_opts.is_genius = 2;
        check_smart_options(&mut c, &smart_opts, "lpma[2112]");

        let report = c.finish(&opts, &HfmaHeader::default());
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"bitflag.invalid_combination"));
        assert!(rules.contains(&"state.unknown_value"));
        assert!(rules.contains(&"enum.fallthrough"));
        assert!(rules.contains(&"bool.non_binary"));
    }

    #[test]
    fn tier3_slst_date_and_playlist_rules() {
        let opts = fixed_opts(true);
        let mut c = SanityCollector::new(&opts, 0);
        let mut ids = LibraryIdIndex::default();
        ids.playlists.insert(0xAAAA);

        let mut valid_rel = SmartPlaylistRuleArguments::default();
        valid_rel.arg_1 = -30;
        valid_rel.arg_2 = 86_400;
        check_smart_rule_args(
            &mut c,
            SmartField::DateAdded,
            SmartOperator::InTheLast,
            &valid_rel,
            "lpma[AAAA]",
            "SLst::rule[0]",
            Some(&ids),
        );
        assert_eq!(c.total_warnings, 0);

        let mut bad_rel = SmartPlaylistRuleArguments::default();
        bad_rel.arg_1 = 5;
        bad_rel.arg_2 = 12_345;
        check_smart_rule_args(
            &mut c,
            SmartField::LastPlayed,
            SmartOperator::InTheLast,
            &bad_rel,
            "lpma[AAAA]",
            "SLst::rule[1]",
            Some(&ids),
        );
        let mut bad_range = SmartPlaylistRuleArguments::default();
        bad_range.arg_0 = 3_700_000_000;
        bad_range.arg_3 = 3_600_000_000;
        check_smart_rule_args(
            &mut c,
            SmartField::DateAdded,
            SmartOperator::InRange,
            &bad_range,
            "lpma[AAAA]",
            "SLst::rule[2]",
            Some(&ids),
        );
        let mut bad_low_date = SmartPlaylistRuleArguments::default();
        bad_low_date.arg_0 = 42;
        check_smart_rule_args(
            &mut c,
            SmartField::DateModified,
            SmartOperator::GreaterThan,
            &bad_low_date,
            "lpma[AAAA]",
            "SLst::rule[3]",
            Some(&ids),
        );
        let mut dangling_pl_rule = SmartPlaylistRuleArguments::default();
        dangling_pl_rule.arg_0 = 0xDEAD_BEEF;
        check_smart_rule_args(
            &mut c,
            SmartField::Playlist,
            SmartOperator::Is,
            &dangling_pl_rule,
            "lpma[AAAA]",
            "SLst::rule[4]",
            Some(&ids),
        );

        let report = c.finish(&opts, &HfmaHeader::default());
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"timestamp.slst_invalid"));
        assert!(rules.contains(&"enum.fallthrough"));
        assert!(rules.contains(&"bounds.dangling_reference"));
    }

    #[test]
    fn tier3_mirrors_store_divergence_and_dangling_refs() {
        let opts = fixed_opts(true);
        let mut c = SanityCollector::new(&opts, 0);
        let mut ids = LibraryIdIndex::default();
        ids.tracks.insert(0x2112);
        ids.playlists.insert(0xAAAA);

        let mut hfma = HfmaHeader::default();
        hfma.file_size = 1000;
        hfma.file_size_mirror = 999;
        check_hfma(&mut c, "hfma", &hfma);

        let mut ps = TrackPlaysSkips::default();
        ps.track_persistent_id = 0x9999;
        check_track_plays_skips(&mut c, &ps, "itma[2112]", Some(0x2112));

        // Master playlist persistent_id_2 and split is_cloud_synced + cloud_playlist_saga_id_3 do NOT warn.
        let warnings_before_pl = c.total_warnings;
        let mut master = musicdb::sections::PlaylistItemHeader::default();
        master.is_master_playlist = 1;
        master.playlist_persistent_id = 5;
        master.playlist_persistent_id_2 = 0x1234_5678_9ABC_DEF0;
        master.cloud_playlist_saga_id_1 = 0x0F43_650D;
        master.cloud_playlist_saga_id_2 = 0x0F43_650D;
        master.is_cloud_synced = 1;
        master.cloud_playlist_saga_id_3 = 0x0F43_650D;
        check_playlist_states_mirrors_and_refs(&mut c, &master, &ids, "lpma[5]");
        assert_eq!(c.total_warnings, warnings_before_pl);

        // Non-master playlist with mismatched persistent_id_2, bad saga_id_3, and non-folder parent warns.
        let mut bad_pl = musicdb::sections::PlaylistItemHeader::default();
        bad_pl.playlist_persistent_id = 0xBBBB;
        bad_pl.playlist_persistent_id_2 = 0xCCCC;
        bad_pl.cloud_playlist_saga_id_1 = 0x10;
        bad_pl.is_cloud_synced = 1;
        bad_pl.cloud_playlist_saga_id_3 = 0x99;
        bad_pl.parent_persistent_id = 0xAAAA;
        check_playlist_states_mirrors_and_refs(&mut c, &bad_pl, &ids, "lpma[BBBB]");

        // Store ID divergence emits Info, not Warning.
        let warnings_before_div = c.total_warnings;
        let mut num = TrackNumerics::default();
        num.subrecord_kind = 1;
        num.store_track_adam_id = 100_001;
        num.store_track_adam_id_2 = 200_002;
        check_track_numerics_mirrors(&mut c, &num, "itma[2112]", None);
        assert_eq!(c.total_warnings, warnings_before_div);

        // Dangling album/artist refs on track.
        let mut track_hdr = musicdb::sections::TrackItemHeader::default();
        track_hdr.track_persistent_id = 0x2112;
        track_hdr.album_persistent_id = 0xDEAD_0001;
        track_hdr.artist_persistent_id = 0xDEAD_0002;
        check_track_mirrors_and_refs(&mut c, &track_hdr, &ids, "itma[2112]");

        let report = c.finish(&opts, &HfmaHeader::default());
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"bounds.mirror_mismatch"));
        assert!(rules.contains(&"bounds.store_id_divergence"));
        assert!(rules.contains(&"bounds.dangling_reference"));
    }

    #[test]
    fn unix_timestamp_predicate_validates_bounds_and_overflow() {
        let opts = fixed_opts(true);
        let mut c = SanityCollector::new(&opts, 0);

        // Zero (unset) and valid 2021 timestamp do not warn.
        c.check_unix_timestamp(
            "plma",
            "112..120",
            "date_cloud_purchase_history_modified",
            0,
            "plma[0]",
            None,
        );
        c.check_unix_timestamp(
            "plma",
            "112..120",
            "date_cloud_purchase_history_modified",
            1_613_088_000,
            "plma[0]",
            None,
        );
        assert_eq!(c.total_warnings, 0);

        // Sentinel, pre-2000, unexpected future, and out-of-range u64::MAX each warn.
        c.check_unix_timestamp(
            "plma",
            "112..120",
            "date_cloud_purchase_history_modified",
            42,
            "plma[0]",
            None,
        );
        c.check_unix_timestamp(
            "plma",
            "112..120",
            "date_cloud_purchase_history_modified",
            500_000_000,
            "plma[0]",
            None,
        );
        let plus_3d = u64::try_from(opts.reference_now.as_second() + 3 * 86_400).unwrap();
        c.check_unix_timestamp(
            "plma",
            "112..120",
            "date_cloud_purchase_history_modified",
            plus_3d,
            "plma[0]",
            None,
        );
        c.check_unix_timestamp(
            "plma",
            "112..120",
            "date_cloud_purchase_history_modified",
            u64::MAX,
            "plma[0]",
            None,
        );

        let report = c.finish(&opts, &HfmaHeader::default());
        assert_eq!(report.total_warnings, 4);
        let rules: Vec<&str> = report.findings.iter().map(|g| g.rule_id).collect();
        assert!(rules.contains(&"timestamp.unknown_sentinel"));
        assert!(rules.contains(&"timestamp.before_itunes_era"));
        assert!(rules.contains(&"timestamp.future_unexpected"));
        assert!(rules.contains(&"timestamp.invalid_unix_timestamp"));
    }
}
