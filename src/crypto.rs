use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use aes::Aes128;
use cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::error::{MusicDbError, Result};

/// Standard 16-byte AES-128-ECB key used by iTunes (.itl) and Apple Music (.musicdb).
pub const DEFAULT_MUSICDB_KEY: &[u8; 16] = b"BHUILuilfghuila3";

/// Unpacks an encrypted and zlib-compressed `.musicdb` file into raw library bytes:
/// `[outer hfma header (uncompressed)] + [decompressed inner payload]`.
pub fn decrypt_and_decompress_bytes(file_bytes: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    if file_bytes.len() < 92 {
        return Err(MusicDbError::HeaderTooLarge {
            header_size: 92,
            file_size: file_bytes.len(),
        });
    }

    let sig: [u8; 4] = file_bytes[0..4].try_into().unwrap();
    if &sig != b"hfma" {
        return Err(MusicDbError::InvalidSignature(sig));
    }

    let header_size = u32::from_le_bytes(file_bytes[4..8].try_into().unwrap()) as usize;
    let file_size = u32::from_le_bytes(file_bytes[8..12].try_into().unwrap());

    if file_bytes.len() != file_size as usize {
        return Err(MusicDbError::FileSizeMismatch {
            expected: file_size,
            actual: file_bytes.len(),
        });
    }

    if header_size > file_bytes.len() {
        return Err(MusicDbError::HeaderTooLarge {
            header_size: header_size as u32,
            file_size: file_bytes.len(),
        });
    }

    let max_encrypted_size = u32::from_le_bytes(file_bytes[84..88].try_into().unwrap()) as usize;
    let payload_size = file_bytes.len() - header_size;

    let encrypted_size =
        if max_encrypted_size > file_bytes.len() || max_encrypted_size > payload_size {
            payload_size - (payload_size % 16)
        } else {
            max_encrypted_size - (max_encrypted_size % 16)
        };

    let cipher = Aes128::new(key.into());
    let mut payload = file_bytes[header_size..].to_vec();

    for chunk in payload[..encrypted_size].chunks_exact_mut(16) {
        let block: &mut [u8; 16] = chunk.try_into().unwrap();
        cipher.decrypt_block(block.into());
    }

    // Decompress with zlib (if payload starts with a valid zlib stream; otherwise check if raw)
    let mut decompressed = Vec::new();
    let mut decoder = ZlibDecoder::new(&payload[..]);
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => {
            let mut raw_bytes = Vec::with_capacity(header_size + decompressed.len());
            raw_bytes.extend_from_slice(&file_bytes[..header_size]);
            raw_bytes.extend_from_slice(&decompressed);
            Ok(raw_bytes)
        }
        Err(e) => {
            // Check if the decrypted payload is already raw sections (e.g. starts with "hsma")
            if payload.len() >= 4 && &payload[0..4] == b"hsma" {
                let mut raw_bytes = Vec::with_capacity(header_size + payload.len());
                raw_bytes.extend_from_slice(&file_bytes[..header_size]);
                raw_bytes.extend_from_slice(&payload);
                Ok(raw_bytes)
            } else {
                Err(MusicDbError::Decompression(e))
            }
        }
    }
}

/// Loads a `.musicdb` file (or `.musiclibrary` bundle directory) from disk and returns the raw (decrypted + decompressed) bytes.
pub fn load_raw_musicdb(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let p = path.as_ref();
    let target = if p.is_dir() && p.join("Library.musicdb").exists() {
        p.join("Library.musicdb")
    } else {
        p.to_path_buf()
    };
    let bytes = fs::read(target)?;
    decrypt_and_decompress_bytes(&bytes, DEFAULT_MUSICDB_KEY)
}

/// Compresses and encrypts raw library bytes (`[outer hfma] + [inner sections]`) back into `.musicdb` format.
pub fn compress_and_encrypt_bytes(raw_bytes: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    if raw_bytes.len() < 92 {
        return Err(MusicDbError::HeaderTooLarge {
            header_size: 92,
            file_size: raw_bytes.len(),
        });
    }

    let sig: [u8; 4] = raw_bytes[0..4].try_into().unwrap();
    if &sig != b"hfma" {
        return Err(MusicDbError::InvalidSignature(sig));
    }

    let header_size = u32::from_le_bytes(raw_bytes[4..8].try_into().unwrap()) as usize;
    let max_encrypted_size = u32::from_le_bytes(raw_bytes[84..88].try_into().unwrap()) as usize;

    let header = &raw_bytes[..header_size];
    let inner = &raw_bytes[header_size..];

    // Apple Music experimentally uses zlib compression level 1 (fast)
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(inner)
        .map_err(MusicDbError::Compression)?;
    let mut compressed = encoder.finish().map_err(MusicDbError::Compression)?;

    let compressed_size = compressed.len();
    let encrypted_size = if max_encrypted_size > compressed_size {
        compressed_size - (compressed_size % 16)
    } else {
        max_encrypted_size - (max_encrypted_size % 16)
    };

    let cipher = Aes128::new(key.into());
    for chunk in compressed[..encrypted_size].chunks_exact_mut(16) {
        let block: &mut [u8; 16] = chunk.try_into().unwrap();
        cipher.encrypt_block(block.into());
    }

    let new_file_size = (header_size + compressed.len()) as u32;
    let mut out = Vec::with_capacity(new_file_size as usize);
    out.extend_from_slice(&header[..8]);
    out.extend_from_slice(&new_file_size.to_le_bytes());
    out.extend_from_slice(&header[12..]);
    out.extend_from_slice(&compressed);

    Ok(out)
}
