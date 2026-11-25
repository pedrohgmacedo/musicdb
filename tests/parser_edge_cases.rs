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

fn make_boma_bytes(subtype: u32, payload: &[u8]) -> Vec<u8> {
    let total_size = 20u32 + u32::try_from(payload.len()).unwrap();
    let mut out = vec![0u8; 20 + payload.len()];
    out[0..4].copy_from_slice(b"boma");
    out[4..8].copy_from_slice(&20u32.to_le_bytes());
    out[8..12].copy_from_slice(&total_size.to_le_bytes());
    out[12..16].copy_from_slice(&subtype.to_le_bytes());
    out[20..].copy_from_slice(payload);
    out
}

#[test]
fn test_oversized_structured_bomas_preserve_unk_tail_roundtrip() {
    let cases: &[(u32, usize)] = &[
        (0x0001, 364 + 12), // TrackNumerics + 12B tail
        (0x0017, 52 + 8),   // TrackPlaysSkips + 8B tail
        (0x0024, 84 + 16),  // VideoInfo + 16B tail
        (0x00CA, 112 + 10), // SmartPlaylistOptions + 10B tail
    ];

    for &(subtype, payload_len) in cases {
        let mut payload = vec![0u8; payload_len];
        let base_len = match subtype {
            0x0001 => 364,
            0x0017 => 52,
            0x0024 => 84,
            0x00CA => 112,
            _ => unreachable!(),
        };
        for (i, b) in payload[base_len..].iter_mut().enumerate() {
            *b = u8::try_from(0xA0 + i).unwrap();
        }
        let boma_bytes = make_boma_bytes(subtype, &payload);
        let parsed =
            BomaSection::read_le(&mut Cursor::new(&boma_bytes)).expect("parse oversized boma");
        match (subtype, &parsed.payload) {
            (0x0001, BomaPayload::TrackNumerics(n)) => {
                assert_eq!(n.unk_tail.as_slice(), &payload[base_len..]);
            }
            (0x0017, BomaPayload::TrackPlaysSkips(ps)) => {
                assert_eq!(ps.unk_tail.as_slice(), &payload[base_len..]);
            }
            (0x0024, BomaPayload::VideoInfo(vi)) => {
                assert_eq!(vi.unk_tail.as_slice(), &payload[base_len..]);
            }
            (0x00CA, BomaPayload::SmartPlaylistOptions(opts)) => {
                assert_eq!(opts.unk_tail.as_slice(), &payload[base_len..]);
            }
            other => panic!("unexpected payload for subtype 0x{subtype:04X}: {other:?}"),
        }
        let mut reserialized = Vec::new();
        parsed
            .write_le(&mut Cursor::new(&mut reserialized))
            .expect("write oversized boma");
        assert_eq!(reserialized, boma_bytes);
    }
}

#[test]
fn test_truncated_known_hsma_child_and_malformed_slst_return_err() {
    // 1. Truncated known hsma children return Err(TruncatedSection):
    //    - subtype=1 TrackList ("ltma", min_len=12) with 8 bytes and with 2 bytes
    //    - subtype=6 LibraryMaster ("plma", min_len=160) with 20 bytes
    for (subtype, payload) in [
        (1u32, b"ltma\x00\x00\x00\x00".as_slice()),
        (1u32, b"lt".as_slice()),
        (6u32, b"plma0123456789abcdef".as_slice()),
    ] {
        let mut trunc_hsma = vec![0u8; 160 + 56 + payload.len()];
        trunc_hsma[0..4].copy_from_slice(b"hfma");
        trunc_hsma[4..8].copy_from_slice(&160u32.to_le_bytes());
        trunc_hsma[160..164].copy_from_slice(b"hsma");
        trunc_hsma[164..168].copy_from_slice(&56u32.to_le_bytes());
        let hsma_total = u32::try_from(56 + payload.len()).expect("fits u32");
        trunc_hsma[168..172].copy_from_slice(&hsma_total.to_le_bytes());
        trunc_hsma[172..176].copy_from_slice(&subtype.to_le_bytes());
        trunc_hsma[216..216 + payload.len()].copy_from_slice(payload);
        assert!(
            matches!(
                RawLibraryFile::from_raw_bytes(&trunc_hsma),
                Err(MusicDbError::TruncatedSection { .. })
            ),
            "expected TruncatedSection for subtype={subtype}, len={}",
            payload.len()
        );
    }

    // 2. Trailer (subtype=255) and Unknown subtype (subtype=99) still parse into HsmaChild::Unknown
    for subtype in [255u32, 99u32] {
        let mut ok_hsma = vec![0u8; 160 + 56 + 12];
        ok_hsma[0..4].copy_from_slice(b"hfma");
        ok_hsma[4..8].copy_from_slice(&160u32.to_le_bytes());
        ok_hsma[160..164].copy_from_slice(b"hsma");
        ok_hsma[164..168].copy_from_slice(&56u32.to_le_bytes());
        ok_hsma[168..172].copy_from_slice(&(56u32 + 12u32).to_le_bytes());
        ok_hsma[172..176].copy_from_slice(&subtype.to_le_bytes());
        ok_hsma[216..228].copy_from_slice(b"RUSH21121976");
        let parsed = RawLibraryFile::from_raw_bytes(&ok_hsma).expect("parse trailer/unknown hsma");
        assert!(matches!(
            parsed.sections[0].child,
            musicdb::sections::HsmaChild::Unknown { .. }
        ));
        assert_eq!(parsed.to_raw_bytes().unwrap(), ok_hsma);
    }

    // 3. Truncated 0x00C9 SLst payload (starts with "SLst", < 136 bytes) returns Err
    let mut trunc_slst = vec![0u8; 40];
    trunc_slst[0..4].copy_from_slice(b"SLst");
    let bad_c9 = make_boma_bytes(0x00C9, &trunc_slst);
    assert!(BomaSection::read_le(&mut Cursor::new(&bad_c9)).is_err());

    // 4. Malformed nested SLst inside a SmartPlaylistRule (starts with "SLst", truncated < 136 bytes) returns Err
    let mut outer_slst = vec![0u8; 136 + 56 + 24];
    outer_slst[0..4].copy_from_slice(b"SLst");
    outer_slst[8..12].copy_from_slice(&1u32.to_be_bytes());
    outer_slst[136 + 52..136 + 56].copy_from_slice(&24u32.to_be_bytes());
    outer_slst[136 + 56..136 + 60].copy_from_slice(b"SLst");
    let bad_nested_c9 = make_boma_bytes(0x00C9, &outer_slst);
    assert!(BomaSection::read_le(&mut Cursor::new(&bad_nested_c9)).is_err());
}

#[test]
fn test_item_total_size_mismatch_and_known_binary_boma_dispatch() {
    // 1. TrackItem (itma) whose declared total_size does not equal header_size + sum(boma.total_size)
    let mut bad_itma = vec![0u8; 376 + 24];
    bad_itma[0..4].copy_from_slice(b"itma");
    bad_itma[4..8].copy_from_slice(&376u32.to_le_bytes()); // header_size
    bad_itma[8..12].copy_from_slice(&(376u32 + 24u32 + 8u32).to_le_bytes()); // mismatched total_size
    bad_itma[12..16].copy_from_slice(&1u32.to_le_bytes()); // boma_count = 1
    let boma = make_boma_bytes(0x0002, &[0x21, 0x12, 0x00, 0x00]);
    bad_itma[376..376 + 24].copy_from_slice(&boma);
    assert!(
        musicdb::sections::TrackItem::read_le(&mut Cursor::new(&bad_itma)).is_err(),
        "mismatched itma total_size must return Err"
    );

    // 2. Known binary plma subtypes (0x01F6, 0x01FF) mimicking BomaStringSection header (encoding=1, len=payload-16)
    // must still parse as BomaPayload::Unknown, not StringSection.
    for subtype in [0x01F6u32, 0x01FFu32] {
        let mut fake_str_payload = vec![0u8; 44];
        fake_str_payload[0..4].copy_from_slice(&1u32.to_le_bytes()); // encoding = 1 (UTF-16LE)
        fake_str_payload[4..8].copy_from_slice(&28u32.to_le_bytes()); // string_byte_len == 44 - 16
        let boma_bytes = make_boma_bytes(subtype, &fake_str_payload);
        let parsed =
            BomaSection::read_le(&mut Cursor::new(&boma_bytes)).expect("parse known binary boma");
        assert!(matches!(
            parsed.payload,
            BomaPayload::Unknown { subtype: s, .. } if s == subtype
        ));
        let mut out = Vec::new();
        parsed
            .write_le(&mut Cursor::new(&mut out))
            .expect("write known binary boma");
        assert_eq!(out, boma_bytes);
    }
}
