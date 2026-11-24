//! AES-128-ECB encryption/decryption and bounded zlib compression/decompression for `.musicdb` files.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use aes::Aes128;
use cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::error::{MusicDbError, Result};

/// Default AES-128-ECB key used by iTunes and Apple Music for `.musicdb` and `.itdb` files.
pub const DEFAULT_MUSICDB_KEY: &[u8; 16] = b"BHUILuilfghuila3";

/// Maximum allowed decompressed payload size (2 GiB) to guard against zip-bomb inputs.
pub const MAX_DECOMPRESSED_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Minimum outer `hfma` header size required to read the encryption envelope fields (up to offset 92).
const MIN_OUTER_HEADER_SIZE: u32 = 92;

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let slice: [u8; 2] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(slice))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice: [u8; 4] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

struct OuterEnvelopeFields {
    header_size_u32: u32,
    file_size: u32,
    major_version: u16,
    max_encrypted_size: usize,
}

fn parse_outer_envelope(bytes: &[u8]) -> Result<OuterEnvelopeFields> {
    let Some(sig_slice) = bytes.get(0..4) else {
        return Err(MusicDbError::TruncatedSection {
            section: "hfma".to_string(),
            offset: 0,
            needed: u64::from(MIN_OUTER_HEADER_SIZE),
            available: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        });
    };
    let sig: [u8; 4] = sig_slice.try_into().unwrap_or([0; 4]);
    if &sig != b"hfma" {
        return Err(MusicDbError::InvalidSignature(sig));
    }

    let (Some(header_size_u32), Some(file_size), Some(major_version), Some(max_encrypted_size_u32)) = (
        read_u32_le(bytes, 4),
        read_u32_le(bytes, 8),
        read_u16_le(bytes, 12),
        read_u32_le(bytes, 84),
    ) else {
        return Err(MusicDbError::TruncatedSection {
            section: "hfma".to_string(),
            offset: 0,
            needed: u64::from(MIN_OUTER_HEADER_SIZE),
            available: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        });
    };

    if header_size_u32 < MIN_OUTER_HEADER_SIZE {
        return Err(MusicDbError::InvalidHeaderSize {
            section: "hfma",
            header_size: header_size_u32,
            min_size: MIN_OUTER_HEADER_SIZE,
        });
    }

    let max_encrypted_size = usize::try_from(max_encrypted_size_u32).unwrap_or(usize::MAX);
    Ok(OuterEnvelopeFields {
        header_size_u32,
        file_size,
        major_version,
        max_encrypted_size,
    })
}

/// Unpacks an encrypted and zlib-compressed `.musicdb` file into raw library bytes:
/// `[outer hfma header (uncompressed)] + [decompressed inner payload]`.
///
/// # Errors
/// Returns [`MusicDbError`] if the header is truncated or invalid, the file size does not match,
/// zlib decompression fails, or the decompressed output exceeds [`MAX_DECOMPRESSED_SIZE`].
pub fn decrypt_and_decompress_bytes(file_bytes: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let env = parse_outer_envelope(file_bytes)?;

    if usize::try_from(env.file_size).ok() != Some(file_bytes.len()) {
        return Err(MusicDbError::FileSizeMismatch {
            expected: env.file_size,
            actual: file_bytes.len(),
        });
    }

    let header_size =
        usize::try_from(env.header_size_u32).map_err(|_| MusicDbError::HeaderTooLarge {
            header_size: env.header_size_u32,
            file_size: file_bytes.len(),
        })?;
    let Some((header_slice, payload_slice)) = file_bytes.split_at_checked(header_size) else {
        return Err(MusicDbError::HeaderTooLarge {
            header_size: env.header_size_u32,
            file_size: file_bytes.len(),
        });
    };

    let payload_size = payload_slice.len();
    let encrypted_size =
        if env.max_encrypted_size > file_bytes.len() || env.max_encrypted_size > payload_size {
            payload_size - (payload_size % 16)
        } else {
            env.max_encrypted_size - (env.max_encrypted_size % 16)
        };

    let cipher = Aes128::new(key.into());
    let (encrypted_slice, plain_tail) = payload_slice
        .split_at_checked(encrypted_size)
        .unwrap_or((payload_slice, &[]));
    let mut decrypted_prefix = encrypted_slice.to_vec();

    for block in decrypted_prefix.as_chunks_mut::<16>().0 {
        cipher.decrypt_block(block.into());
    }

    // Check if the decrypted payload is already raw sections (starts with "hsma", which can never be a valid RFC 1950 zlib header since 0x6873 % 31 != 0)
    let first_four = if decrypted_prefix.len() >= 4 {
        decrypted_prefix.get(0..4)
    } else {
        plain_tail.get(0..4)
    };
    if first_four == Some(b"hsma") {
        let mut raw_bytes = Vec::with_capacity(header_slice.len().saturating_add(payload_size));
        raw_bytes.extend_from_slice(header_slice);
        raw_bytes.extend_from_slice(&decrypted_prefix);
        raw_bytes.extend_from_slice(plain_tail);
        return Ok(raw_bytes);
    }

    // Estimate decompressed size from the actual in-memory compressed length (~4.5-5x typical for .musicdb)
    // while strictly capping against MAX_DECOMPRESSED_SIZE.
    let max_cap = usize::try_from(MAX_DECOMPRESSED_SIZE).unwrap_or(usize::MAX);
    let estimated_decompressed = payload_size.saturating_mul(5).min(max_cap);
    let mut raw_bytes =
        Vec::with_capacity(header_slice.len().saturating_add(estimated_decompressed));
    raw_bytes.extend_from_slice(header_slice);
    let decoder = ZlibDecoder::new(decrypted_prefix.as_slice().chain(plain_tail));
    let mut bounded_decoder = decoder.take(MAX_DECOMPRESSED_SIZE.saturating_add(1));
    bounded_decoder
        .read_to_end(&mut raw_bytes)
        .map_err(MusicDbError::Decompression)?;

    let decompressed_len = raw_bytes.len().saturating_sub(header_slice.len());
    if u64::try_from(decompressed_len).unwrap_or(u64::MAX) > MAX_DECOMPRESSED_SIZE {
        return Err(MusicDbError::DecompressionLimitExceeded {
            max_bytes: MAX_DECOMPRESSED_SIZE,
        });
    }

    Ok(raw_bytes)
}

/// Loads a `.musicdb` file, `.musiclibrary` bundle directory, or parent folder containing a `.musiclibrary` bundle
/// from disk and returns the raw (decrypted + decompressed) bytes.
///
/// # Errors
/// Returns [`MusicDbError`] if the file cannot be read, decrypted, or decompressed.
pub fn load_raw_musicdb(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let target = crate::sections::RawLibraryFile::resolve_musicdb_path(path);
    let bytes = fs::read(target)?;
    decrypt_and_decompress_bytes(&bytes, DEFAULT_MUSICDB_KEY)
}

/// Compresses and encrypts raw library bytes (`[outer hfma] + [inner sections]`) back into `.musicdb` format.
///
/// # Errors
/// Returns [`MusicDbError`] if the outer `hfma` header is truncated or invalid, or if zlib compression fails.
pub fn compress_and_encrypt_bytes(raw_bytes: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let env = parse_outer_envelope(raw_bytes)?;

    let header_size =
        usize::try_from(env.header_size_u32).map_err(|_| MusicDbError::HeaderTooLarge {
            header_size: env.header_size_u32,
            file_size: raw_bytes.len(),
        })?;
    let Some((header, inner)) = raw_bytes.split_at_checked(header_size) else {
        return Err(MusicDbError::HeaderTooLarge {
            header_size: env.header_size_u32,
            file_size: raw_bytes.len(),
        });
    };

    let mut out = Vec::with_capacity(header_size.saturating_add(inner.len() / 4));
    out.extend_from_slice(header);

    // Apple Music experimentally uses zlib compression level 1 (fast)
    let mut encoder = ZlibEncoder::new(out, Compression::fast());
    encoder
        .write_all(inner)
        .map_err(MusicDbError::Compression)?;
    let mut out = encoder.finish().map_err(MusicDbError::Compression)?;

    let compressed_size = out.len().saturating_sub(header_size);
    let encrypted_size = if env.max_encrypted_size > compressed_size {
        compressed_size - (compressed_size % 16)
    } else {
        env.max_encrypted_size - (env.max_encrypted_size % 16)
    };

    let cipher = Aes128::new(key.into());
    if let Some(encrypted_region) =
        out.get_mut(header_size..header_size.saturating_add(encrypted_size))
    {
        for block in encrypted_region.as_chunks_mut::<16>().0 {
            cipher.encrypt_block(block.into());
        }
    }

    let new_file_size = u32::try_from(out.len()).map_err(|_| {
        MusicDbError::Compression(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "compressed musicdb exceeds u32::MAX bytes",
        ))
    })?;

    if let Some(file_size_slot) = out.get_mut(8..12) {
        file_size_slot.copy_from_slice(&new_file_size.to_le_bytes());
    }
    if header_size >= 132
        && (env.major_version >= 23 || read_u32_le(header, 128).unwrap_or(0) != 0)
        && let Some(mirror_slot) = out.get_mut(128..132)
    {
        mirror_slot.copy_from_slice(&new_file_size.to_le_bytes());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_and_encrypt_format_23_short_header_does_not_corrupt_payload() {
        let header_size = 92u32;
        let mut raw = vec![0u8; header_size as usize];
        raw[0..4].copy_from_slice(b"hfma");
        raw[4..8].copy_from_slice(&header_size.to_le_bytes());
        raw[12..14].copy_from_slice(&23u16.to_le_bytes());
        raw[84..88].copy_from_slice(&4096u32.to_le_bytes());
        // Enough non-trivial inner bytes so compressed payload spans across byte offset 128..132.
        let payload = b"Rush - Moving Pictures (1981): Tom Sawyer, Red Barchetta, YYZ, Limelight, The Camera Eye, Witch Hunt, Vital Signs.";
        raw.extend_from_slice(payload);

        let encrypted =
            compress_and_encrypt_bytes(&raw, DEFAULT_MUSICDB_KEY).expect("compress and encrypt");
        assert!(encrypted.len() >= 132);
        let decrypted =
            decrypt_and_decompress_bytes(&encrypted, DEFAULT_MUSICDB_KEY).expect("decrypt");
        assert_eq!(decrypted[header_size as usize..], payload[..]);
    }
}
