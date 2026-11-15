//! Error types returned by `musicdb` parsing, export, and encryption operations.

use thiserror::Error;

/// Error returned by fallible `musicdb` operations.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum MusicDbError {
    /// Underlying filesystem or stream I/O error.
    #[error("i/o error")]
    Io(#[from] std::io::Error),

    /// Binary structure parsing or serialization error from `binrw`.
    #[error("binary parsing error")]
    BinRw(#[from] binrw::Error),

    /// File does not begin with the expected 4-byte `"hfma"` signature.
    #[error("invalid file signature: expected 'hfma', got {0:?}")]
    InvalidSignature([u8; 4]),

    /// Declared `file_size` in outer `hfma` header does not match actual buffer length.
    #[error("file size mismatch: header says {expected} bytes, actual file is {actual} bytes")]
    FileSizeMismatch {
        /// Declared size in the `hfma` header.
        expected: u32,
        /// Actual byte length of the input buffer.
        actual: usize,
    },

    /// Declared `header_size` exceeds the total input buffer length.
    #[error("header size ({header_size}) exceeds file size ({file_size})")]
    HeaderTooLarge {
        /// Declared header size in bytes.
        header_size: u32,
        /// Total input buffer size in bytes.
        file_size: usize,
    },

    /// Declared `header_size` is smaller than the minimum fixed size of the section header.
    #[error("invalid header size in {section}: got {header_size}, minimum is {min_size}")]
    InvalidHeaderSize {
        /// Name of the binary section (`hfma`, `hsma`, `boma`, etc.).
        section: &'static str,
        /// Declared header size read from the stream.
        header_size: u32,
        /// Minimum valid header size for that section.
        min_size: u32,
    },

    /// A section or item count declares more bytes than remain in the input stream.
    #[error(
        "truncated section {section} at offset {offset:#x}: needed {needed} bytes, only {available} bytes remain"
    )]
    TruncatedSection {
        /// Name of the binary section (`hsma`, `boma`, `ltma.tracks`, etc.).
        section: String,
        /// Stream byte offset where the truncation was detected.
        offset: u64,
        /// Minimum number of bytes required.
        needed: u64,
        /// Number of bytes actually remaining in the stream.
        available: u64,
    },

    /// Zlib decompression failed.
    #[error("zlib decompression failed")]
    Decompression(#[source] std::io::Error),

    /// Decompressed payload exceeded [`crate::crypto::MAX_DECOMPRESSED_SIZE`].
    #[error("decompressed payload exceeds maximum allowed size ({max_bytes} bytes)")]
    DecompressionLimitExceeded {
        /// Maximum allowed decompressed payload size in bytes.
        max_bytes: u64,
    },

    /// Zlib compression failed.
    #[error("zlib compression failed")]
    Compression(#[source] std::io::Error),

    /// Parsed child section did not consume the exact byte range declared by its parent boundary.
    #[error(
        "section boundary mismatch in {section}: expected end at offset {expected:#x}, actual {actual:#x}"
    )]
    SectionBoundaryMismatch {
        /// Section identifier and subtype.
        section: String,
        /// Expected end offset in the stream.
        expected: u64,
        /// Actual cursor offset after parsing the child section.
        actual: u64,
    },

    /// SQLite database export error.
    #[error("sqlite export error")]
    Sqlite(#[from] rusqlite::Error),

    /// Apple `.plist` parsing error.
    #[error("plist parsing error")]
    Plist(#[from] plist::Error),

    /// JSON serialization error.
    #[error("json serialization error")]
    Json(#[from] serde_json::Error),
}

/// Convenience result alias using [`MusicDbError`].
pub type Result<T> = std::result::Result<T, MusicDbError>;
