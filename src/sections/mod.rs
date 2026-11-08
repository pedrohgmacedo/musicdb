pub mod album;
pub mod artist;
pub mod boma;
pub mod header;
pub mod library_master;
pub mod playlist;
pub mod social;
pub mod track;

use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use binrw::{BinRead, BinResult, BinWrite, Endian, binrw};
use serde::{Deserialize, Serialize};

use crate::crypto::{DEFAULT_MUSICDB_KEY, compress_and_encrypt_bytes, load_raw_musicdb};
use crate::error::{MusicDbError, Result};

pub use album::{IamaHeader, IamaSection, LamaHeader, LamaSection};
pub use artist::{IAmaHeader, IAmaSection, LAmaHeader, LAmaSection};
pub use boma::{BomaPayload, BomaSection, BomaStringSection};
pub use header::{HfmaHeader, HsmaHeader};
pub use library_master::{PlmaHeader, PlmaSection, PlmaSub1F6, PlmaSub1FF};
pub use playlist::{
    IPfaSection, IpfaSection, LPmaHeader, LPmaSection, LpmaHeader, LpmaSection, SlstSection,
    SmartPlaylistOptions, SmartPlaylistRule, SmartPlaylistRuleArguments, SmartRuleChild,
};
pub use social::{IsmaHeader, IsmaSection, LsmaHeader, LsmaSection};
pub use track::{
    ItmaHeader, ItmaSection, LtmaHeader, LtmaSection, TrackNumerics, TrackPlaysSkips, VideoInfo,
};

pub mod hex_array {
    use serde::de::Error;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S, const N: usize>(
        bytes: &[u8; N],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D, const N: usize>(
        deserializer: D,
    ) -> std::result::Result<[u8; N], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        let vec = hex::decode(&s).map_err(D::Error::custom)?;
        vec.try_into()
            .map_err(|v: Vec<u8>| D::Error::custom(format!("expected {N} bytes, got {}", v.len())))
    }
}

/// `LPma` (Padding / Footer section at the end of recent libraries, subtype 17 of `hsma`).
#[binrw]
#[brw(little, magic = b"LPma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LPmaPaddingSection {
    /// Offset 4: Section size in bytes (typically 96).
    pub section_size: u32,
    /// Offset 8..section_size: Remaining bytes (offset 12 is typically 6).
    #[br(count = section_size.saturating_sub(8))]
    pub raw_bytes: Vec<u8>,
}

/// A major `hsma` section boundary and its parsed child section tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HsmaSection {
    pub header: HsmaHeader,
    pub child: HsmaChild,
}

/// Child section inside an `hsma` boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HsmaChild {
    /// Subtype 3: Inner `hfma` header.
    InnerHfma(HfmaHeader),
    /// Subtype 6: `plma` Library Master.
    LibraryMaster(PlmaSection),
    /// Subtype 4: `lama` Album List.
    AlbumList(LamaSection),
    /// Subtype 5: `lAma` Artist List.
    ArtistList(LAmaSection),
    /// Subtype 1: `ltma` Track List.
    TrackList(LtmaSection),
    /// Subtype 2: `lPma` Playlist List.
    PlaylistList(LPmaSection),
    /// Subtype 15: `Lsma` Social Profile List.
    SocialProfileList(LsmaSection),
    /// Subtype 17: `LPma` Padding / Footer.
    Padding(LPmaPaddingSection),
    /// Any other / unknown `hsma` child (e.g. `Library Preferences.musicdb` or future subtype), preserved losslessly.
    Unknown {
        subtype: u32,
        signature: [u8; 4],
        raw_bytes: Vec<u8>,
    },
}

impl BinRead for HsmaSection {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let start_pos = reader.stream_position()?;
        let header = HsmaHeader::read_le(reader)?;
        let total_size = header.effective_total_size();
        let expected_end = start_pos + total_size;
        let child_len = total_size.saturating_sub(header.header_size as u64) as usize;

        let mut child_bytes = vec![0u8; child_len];
        reader.read_exact(&mut child_bytes)?;

        let mut cur = Cursor::new(&child_bytes);
        let child = match header.subtype {
            3 if child_bytes.len() >= 160 && &child_bytes[0..4] == b"hfma" => {
                HsmaChild::InnerHfma(HfmaHeader::read_le(&mut cur)?)
            }
            6 if child_bytes.len() >= 160 && &child_bytes[0..4] == b"plma" => {
                HsmaChild::LibraryMaster(PlmaSection::read_le(&mut cur)?)
            }
            4 if child_bytes.len() >= 12 && &child_bytes[0..4] == b"lama" => {
                HsmaChild::AlbumList(LamaSection::read_le(&mut cur)?)
            }
            5 if child_bytes.len() >= 12 && &child_bytes[0..4] == b"lAma" => {
                HsmaChild::ArtistList(LAmaSection::read_le(&mut cur)?)
            }
            1 if child_bytes.len() >= 12 && &child_bytes[0..4] == b"ltma" => {
                HsmaChild::TrackList(LtmaSection::read_le(&mut cur)?)
            }
            2 if child_bytes.len() >= 12 && &child_bytes[0..4] == b"lPma" => {
                HsmaChild::PlaylistList(LPmaSection::read_le(&mut cur)?)
            }
            15 if child_bytes.len() >= 12 && &child_bytes[0..4] == b"Lsma" => {
                HsmaChild::SocialProfileList(LsmaSection::read_le(&mut cur)?)
            }
            17 if child_bytes.len() >= 8 && &child_bytes[0..4] == b"LPma" => {
                HsmaChild::Padding(LPmaPaddingSection::read_le(&mut cur)?)
            }
            _ => {
                let mut sig = [0u8; 4];
                if child_bytes.len() >= 4 {
                    sig.copy_from_slice(&child_bytes[0..4]);
                }
                cur.set_position(child_bytes.len() as u64);
                HsmaChild::Unknown {
                    subtype: header.subtype,
                    signature: sig,
                    raw_bytes: child_bytes.clone(),
                }
            }
        };

        // Ensure the parsed child consumed every single byte of `child_bytes`!
        if cur.position() as usize != child_bytes.len() {
            return Err(binrw::Error::Custom {
                pos: start_pos + header.header_size as u64 + cur.position(),
                err: Box::new(MusicDbError::SectionBoundaryMismatch {
                    section: format!("hsma(subtype={})", header.subtype),
                    expected: expected_end,
                    actual: start_pos + header.header_size as u64 + cur.position(),
                }),
            });
        }

        Ok(Self { header, child })
    }
}

impl BinWrite for HsmaSection {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<()> {
        self.header.write_le(writer)?;
        match &self.child {
            HsmaChild::InnerHfma(h) => h.write_le(writer)?,
            HsmaChild::LibraryMaster(p) => p.write_le(writer)?,
            HsmaChild::AlbumList(l) => l.write_le(writer)?,
            HsmaChild::ArtistList(l) => l.write_le(writer)?,
            HsmaChild::TrackList(l) => l.write_le(writer)?,
            HsmaChild::PlaylistList(l) => l.write_le(writer)?,
            HsmaChild::SocialProfileList(s) => s.write_le(writer)?,
            HsmaChild::Padding(p) => p.write_le(writer)?,
            HsmaChild::Unknown { raw_bytes, .. } => writer.write_all(raw_bytes)?,
        }
        Ok(())
    }
}

/// Complete Layer-1 Raw AST of a `.musicdb` file.
/// Preserves 100% of bytes for lossless round-tripping and unknown-byte analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawLibraryFile {
    /// Outer uncompressed `hfma` envelope header.
    pub outer_header: HfmaHeader,
    /// Sequence of `hsma` major sections inside the decrypted + decompressed payload.
    pub sections: Vec<HsmaSection>,
    /// Any trailing bytes after the last `hsma` section (e.g. `ssma` EOF padding if outside `hsma`).
    pub trailing_bytes: Vec<u8>,
}

impl RawLibraryFile {
    /// Resolves either a `.musiclibrary` bundle directory or a direct `.musicdb` file path
    /// to the underlying `.musicdb` file path.
    pub fn resolve_musicdb_path(path: impl AsRef<Path>) -> std::path::PathBuf {
        let p = path.as_ref();
        if p.is_dir() {
            let candidate = p.join("Library.musicdb");
            if candidate.exists() {
                return candidate;
            }
        }
        p.to_path_buf()
    }

    /// Reads only the uncompressed 160-byte outer `hfma` header from disk in O(1) without decrypting or decompressing the library payload.
    pub fn read_outer_header(path: impl AsRef<Path>) -> Result<HfmaHeader> {
        let mdb_path = Self::resolve_musicdb_path(path);
        let mut file = std::fs::File::open(mdb_path)?;
        let header = HfmaHeader::read_le(&mut file)?;
        Ok(header)
    }

    /// Parses a `.musicdb` file (or `.musiclibrary` bundle directory) from disk, decrypting and decompressing automatically.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mdb_path = Self::resolve_musicdb_path(path);
        let raw_bytes = load_raw_musicdb(mdb_path)?;
        Self::from_raw_bytes(&raw_bytes)
    }

    /// Parses only selected `hsma` section subtypes from a `.musicdb` file (or `.musiclibrary` bundle),
    /// skipping unwanted sections (such as the 83 MB TrackList when only reading Playlists or Albums) in O(1).
    pub fn open_filtered<F>(path: impl AsRef<Path>, include_subtype: F) -> Result<Self>
    where
        F: Fn(u32) -> bool,
    {
        let mdb_path = Self::resolve_musicdb_path(path);
        let raw_bytes = load_raw_musicdb(mdb_path)?;
        Self::from_raw_bytes_filtered(&raw_bytes, include_subtype)
    }

    /// Parses already-decrypted and decompressed raw `.musicdb` bytes.
    pub fn from_raw_bytes(raw_bytes: &[u8]) -> Result<Self> {
        Self::from_raw_bytes_filtered(raw_bytes, |_| true)
    }

    /// Parses already-decrypted and decompressed raw `.musicdb` bytes, skipping `hsma` sections where `include_subtype(subtype)` is false.
    pub fn from_raw_bytes_filtered<F>(raw_bytes: &[u8], include_subtype: F) -> Result<Self>
    where
        F: Fn(u32) -> bool,
    {
        let mut cur = Cursor::new(raw_bytes);
        let outer_header = HfmaHeader::read_le(&mut cur)?;

        let mut sections = Vec::new();
        while (cur.position() as usize) + 4 <= raw_bytes.len() {
            let pos = cur.position() as usize;
            if &raw_bytes[pos..pos + 4] == b"hsma" {
                let header = HsmaHeader::read_le(&mut cur)?;
                let total_size = header.effective_total_size();
                let child_len = total_size.saturating_sub(header.header_size as u64) as usize;
                if include_subtype(header.subtype) {
                    cur.set_position(pos as u64);
                    let hsma = HsmaSection::read_le(&mut cur)?;
                    sections.push(hsma);
                } else {
                    cur.set_position(
                        (pos as u64) + (header.header_size as u64) + (child_len as u64),
                    );
                }
            } else {
                break;
            }
        }

        let pos = cur.position() as usize;
        let trailing_bytes = raw_bytes[pos..].to_vec();

        Ok(Self {
            outer_header,
            sections,
            trailing_bytes,
        })
    }

    /// Serializes the `RawLibraryFile` back into raw (uncompressed, unencrypted) bytes.
    pub fn to_raw_bytes(&self) -> Result<Vec<u8>> {
        let mut cur = Cursor::new(Vec::new());
        self.outer_header.write_le(&mut cur)?;
        for section in &self.sections {
            section.write_le(&mut cur)?;
        }
        cur.write_all(&self.trailing_bytes)?;
        Ok(cur.into_inner())
    }

    /// Serializes, compresses (zlib), and encrypts (AES-128-ECB) back into `.musicdb` disk format.
    pub fn to_encrypted_bytes(&self) -> Result<Vec<u8>> {
        let raw = self.to_raw_bytes()?;
        compress_and_encrypt_bytes(&raw, DEFAULT_MUSICDB_KEY)
    }
}
