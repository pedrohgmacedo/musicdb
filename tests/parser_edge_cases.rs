//! Integration tests for malformed inputs and parser edge cases via the public API.

use std::io::Cursor;

use binrw::{BinRead, BinWrite};
use musicdb::sections::{BomaPayload, BomaSection, SlstSection};
use musicdb::{
    DEFAULT_MUSICDB_KEY, MusicDbError, RawLibraryFile, compress_and_encrypt_bytes,
    decrypt_and_decompress_bytes,
};

#[test]
fn test_malformed_inputs_do_not_panic_or_abort() {
    // Probe 1: compress_and_encrypt_bytes with header_size = 5000 on a 160-byte buffer
    let mut bad_header = vec![0u8; 160];
    bad_header[0..4].copy_from_slice(b"hfma");
    bad_header[4..8].copy_from_slice(&5000u32.to_le_bytes());
    assert!(matches!(
        compress_and_encrypt_bytes(&bad_header, DEFAULT_MUSICDB_KEY),
        Err(MusicDbError::HeaderTooLarge {
            header_size: 5000,
            file_size: 160
        })
    ));

    // Probe 1b: header_size below minimum (< 92) in both decrypt and encrypt
    let mut small_header = vec![0u8; 160];
    small_header[0..4].copy_from_slice(b"hfma");
    small_header[4..8].copy_from_slice(&16u32.to_le_bytes());
    small_header[8..12].copy_from_slice(&160u32.to_le_bytes());
    assert!(matches!(
        decrypt_and_decompress_bytes(&small_header, DEFAULT_MUSICDB_KEY),
        Err(MusicDbError::InvalidHeaderSize {
            section: "hfma",
            header_size: 16,
            min_size: 92
        })
    ));
    assert!(matches!(
        compress_and_encrypt_bytes(&small_header, DEFAULT_MUSICDB_KEY),
        Err(MusicDbError::InvalidHeaderSize {
            section: "hfma",
            header_size: 16,
            min_size: 92
        })
    ));

    // Probe 2: RawLibraryFile::from_raw_bytes with hsma.total_size_64 = u64::MAX / 2
    let mut huge_hsma = vec![0u8; 160 + 56];
    huge_hsma[0..4].copy_from_slice(b"hfma");
    huge_hsma[4..8].copy_from_slice(&160u32.to_le_bytes());
    huge_hsma[160..164].copy_from_slice(b"hsma");
    huge_hsma[164..168].copy_from_slice(&56u32.to_le_bytes());
    huge_hsma[168..172].copy_from_slice(&0u32.to_le_bytes());
    huge_hsma[172..176].copy_from_slice(&1u32.to_le_bytes());
    huge_hsma[176..184].copy_from_slice(&(u64::MAX / 2).to_le_bytes());
    assert!(matches!(
        RawLibraryFile::from_raw_bytes(&huge_hsma),
        Err(MusicDbError::TruncatedSection {
            ref section,
            ..
        }) if section == "hsma(subtype=1)"
    ));

    // Probe 3: HfmaHeader with header_size = 100 (< 160 minimum)
    let mut short_hfma = vec![0u8; 160];
    short_hfma[0..4].copy_from_slice(b"hfma");
    short_hfma[4..8].copy_from_slice(&100u32.to_le_bytes());
    assert!(matches!(
        RawLibraryFile::from_raw_bytes(&short_hfma),
        Err(MusicDbError::InvalidHeaderSize {
            section: "hfma",
            header_size: 100,
            min_size: 160
        })
    ));

    // Probe 4: ltma section claiming u32::MAX tracks inside a small hsma buffer
    let mut huge_track_count = vec![0u8; 160 + 56 + 92];
    huge_track_count[0..4].copy_from_slice(b"hfma");
    huge_track_count[4..8].copy_from_slice(&160u32.to_le_bytes());
    huge_track_count[160..164].copy_from_slice(b"hsma");
    huge_track_count[164..168].copy_from_slice(&56u32.to_le_bytes());
    huge_track_count[168..172].copy_from_slice(&(56u32 + 92u32).to_le_bytes());
    huge_track_count[172..176].copy_from_slice(&1u32.to_le_bytes());
    huge_track_count[216..220].copy_from_slice(b"ltma");
    huge_track_count[220..224].copy_from_slice(&92u32.to_le_bytes());
    huge_track_count[224..228].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        RawLibraryFile::from_raw_bytes(&huge_track_count),
        Err(MusicDbError::TruncatedSection {
            ref section,
            ..
        }) if section == "ltma.tracks"
    ));
}

#[test]
fn test_deeply_nested_slst_and_trailing_ipfa_roundtrip_without_overflow() {
    let rush_utf16be: Vec<u8> = "Rush".encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut payload = vec![0u8; 136 + 56 + rush_utf16be.len()];
    payload[0..4].copy_from_slice(b"SLst");
    payload[8..12].copy_from_slice(&1u32.to_be_bytes());
    payload[136..140].copy_from_slice(&0x04u32.to_be_bytes()); // Artist
    payload[140..144].copy_from_slice(&0x0100_0001u32.to_be_bytes()); // STRING_MODE | IS
    let leaf_len = u32::try_from(rush_utf16be.len()).unwrap();
    payload[188..192].copy_from_slice(&leaf_len.to_be_bytes());
    payload[192..].copy_from_slice(&rush_utf16be);

    for _ in 0..256 {
        let child_len = u32::try_from(payload.len()).unwrap();
        let mut outer = vec![0u8; 136 + 56];
        outer[0..4].copy_from_slice(b"SLst");
        outer[8..12].copy_from_slice(&1u32.to_be_bytes());
        outer[188..192].copy_from_slice(&child_len.to_be_bytes());
        outer.extend_from_slice(&payload);
        payload = outer;
    }

    let parsed_slst =
        SlstSection::read_be(&mut Cursor::new(&payload)).expect("parse 256-deep SLst");
    let mut reserialized_slst = Vec::new();
    parsed_slst
        .write_be(&mut Cursor::new(&mut reserialized_slst))
        .expect("write 256-deep SLst");
    assert_eq!(reserialized_slst, payload);

    // Verify boma 0x00CE (ipfa) and 0x00DA (IPfa) do not drop bytes if section_size < payload len
    for (subtype, magic, declared_sec_size, payload_len) in [
        (0x00CEu32, b"ipfa", 52u32, 68usize),
        (0x00DAu32, b"IPfa", 24u32, 32usize),
    ] {
        let total_size = 20u32 + u32::try_from(payload_len).unwrap();
        let mut boma_bytes = vec![0u8; 20 + payload_len];
        boma_bytes[0..4].copy_from_slice(b"boma");
        boma_bytes[4..8].copy_from_slice(&20u32.to_le_bytes());
        boma_bytes[8..12].copy_from_slice(&total_size.to_le_bytes());
        boma_bytes[12..16].copy_from_slice(&subtype.to_le_bytes());
        boma_bytes[20..24].copy_from_slice(magic);
        boma_bytes[24..28].copy_from_slice(&declared_sec_size.to_le_bytes());
        boma_bytes[20 + payload_len - 1] = 0xAB;

        let parsed_boma = BomaSection::read_le(&mut Cursor::new(&boma_bytes))
            .expect("parse trailing-byte ipfa/IPfa boma");
        assert!(matches!(parsed_boma.payload, BomaPayload::Unknown { .. }));
        let mut out = Vec::new();
        parsed_boma
            .write_le(&mut Cursor::new(&mut out))
            .expect("write boma");
        assert_eq!(out, boma_bytes);
    }

    // Verify DCma / dCma malformed header_size, section_size, and huge item_count produce Err cleanly
    let mut bad_dcma_hdr = vec![0u8; 16];
    bad_dcma_hdr[0..4].copy_from_slice(b"DCma");
    bad_dcma_hdr[4..8].copy_from_slice(&8u32.to_le_bytes()); // < min 12
    assert!(
        musicdb::sections::CloudDeletionListSection::read_le(&mut Cursor::new(&bad_dcma_hdr))
            .is_err()
    );

    let mut huge_dcma_count = vec![0u8; 92];
    huge_dcma_count[0..4].copy_from_slice(b"DCma");
    huge_dcma_count[4..8].copy_from_slice(&92u32.to_le_bytes());
    huge_dcma_count[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        musicdb::sections::CloudDeletionListSection::read_le(&mut Cursor::new(&huge_dcma_count))
            .is_err()
    );

    let mut bad_item_size = vec![0u8; 24];
    bad_item_size[0..4].copy_from_slice(b"dCma");
    bad_item_size[4..8].copy_from_slice(&16u32.to_le_bytes()); // < min 24
    assert!(
        musicdb::sections::CloudDeletionItem::read_le(&mut Cursor::new(&bad_item_size)).is_err()
    );
}
