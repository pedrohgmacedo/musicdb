use thiserror::Error;

#[derive(Debug, Error)]
pub enum MusicDbError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Binary parsing error: {0}")]
    BinRw(#[from] binrw::Error),

    #[error("Invalid file signature: expected 'hfma', got {0:?}")]
    InvalidSignature([u8; 4]),

    #[error("File size mismatch: header says {expected} bytes, actual file is {actual} bytes")]
    FileSizeMismatch { expected: u32, actual: usize },

    #[error("Header size ({header_size}) exceeds file size ({file_size})")]
    HeaderTooLarge { header_size: u32, file_size: usize },

    #[error("Zlib decompression error: {0}")]
    Decompression(std::io::Error),

    #[error("Zlib compression error: {0}")]
    Compression(std::io::Error),

    #[error("UTF-8 decoding error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("UTF-16 decoding error: {0}")]
    Utf16(#[from] std::string::FromUtf16Error),

    #[error(
        "Section boundary mismatch in {section}: expected end at offset {expected}, actual {actual}"
    )]
    SectionBoundaryMismatch {
        section: String,
        expected: u64,
        actual: u64,
    },

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Plist error: {0}")]
    Plist(#[from] plist::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, MusicDbError>;
