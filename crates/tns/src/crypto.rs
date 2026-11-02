//! TI's "method 13" entry encryption.
//!
//! A 40-byte header, 3DES-ECB encrypted under a fixed key, holds `TIEN0100`, the block count (LE u32,
//! 0x400), a counter seed (LE u32) and 21 bytes of key material (three packed 56-bit DES keys). The body
//! is raw deflate XORed with a keystream: 3DES-ECB(`00000000 || LE u32(seed + block % blocks)`) under the
//! key built from the material. Layout as documented by TnsTools; this is an independent implementation.

use des::TdesEde3;
use des::cipher::generic_array::GenericArray;
use des::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use std::io::{Read, Write};

use crate::{Result, TnsError};

const HEADER_KEY: [u8; 24] = [
    0x79, 0xc4, 0xe0, 0xf4, 0x5e, 0xef, 0x7a, 0x5b, 0x70, 0x13, 0x7a, 0x57, 0xc2, 0xfd, 0x3d, 0x2c, 0xc2, 0x70,
    0x7c, 0xc1, 0xad, 0x2f, 0x15, 0x75,
];
const MAGIC: &[u8; 8] = b"TIEN0100";

/// Expand 7 packed key bytes into an 8-byte DES key with odd parity.
fn des_key(k: &[u8]) -> [u8; 8] {
    let b = k;
    let raw = [
        b[0] & 0xFE,
        ((b[1] >> 1) & 0x7E) | (b[0] << 7),
        ((b[2] >> 2) & 0x3E) | (b[1] << 6),
        ((b[3] >> 3) & 0x1E) | (b[2] << 5),
        ((b[4] >> 4) & 0x0E) | (b[3] << 4),
        ((b[5] >> 5) & 0x06) | (b[4] << 3),
        ((b[6] >> 6) & 0x02) | (b[5] << 2),
        b[6] << 1,
    ];
    raw.map(|x| if x.count_ones() % 2 == 0 { x | 1 } else { x })
}

fn body_cipher(material: &[u8]) -> TdesEde3 {
    let mut key = [0u8; 24];
    for (i, chunk) in material.chunks(7).take(3).enumerate() {
        key[i * 8..i * 8 + 8].copy_from_slice(&des_key(chunk));
    }
    TdesEde3::new_from_slice(&key).expect("24-byte key")
}

fn keystream_xor(data: &mut [u8], seed: u32, material: &[u8], blocks: u32) {
    let cipher = body_cipher(material);
    let blocks = if blocks == 0 { 0x400 } else { blocks };
    for (i, chunk) in data.chunks_mut(8).enumerate() {
        let mut block = [0u8; 8];
        block[4..].copy_from_slice(&seed.wrapping_add(i as u32 % blocks).to_le_bytes());
        let mut ga = GenericArray::from(block);
        cipher.encrypt_block(&mut ga);
        for (d, k) in chunk.iter_mut().zip(ga.iter()) {
            *d ^= k;
        }
    }
}

fn header_cipher() -> TdesEde3 {
    TdesEde3::new_from_slice(&HEADER_KEY).expect("24-byte key")
}

/// Method-13 payload → the inflated TIXC (or plain) bytes.
pub fn decrypt(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() < 40 {
        return Err(TnsError::Damaged("encrypted entry is too short".into()));
    }
    let hc = header_cipher();
    let mut header = [0u8; 40];
    header.copy_from_slice(&payload[..40]);
    for block in header.chunks_mut(8) {
        let mut ga = GenericArray::clone_from_slice(block);
        hc.decrypt_block(&mut ga);
        block.copy_from_slice(&ga);
    }
    if &header[..8] != MAGIC {
        return Err(TnsError::Damaged("not a TI-encrypted entry".into()));
    }
    let blocks = u32::from_le_bytes(header[8..12].try_into().unwrap());
    let seed = u32::from_le_bytes(header[12..16].try_into().unwrap());
    let mut body = payload[40..].to_vec();
    keystream_xor(&mut body, seed, &header[16..37], blocks);
    let mut out = Vec::new();
    DeflateDecoder::new(&body[..])
        .read_to_end(&mut out)
        .map_err(|e| TnsError::Damaged(format!("inflate failed: {e}")))?;
    Ok(out)
}

/// Encrypt plain bytes as a method-13 payload with fresh random key material.
pub fn encrypt(plain: &[u8]) -> Vec<u8> {
    let mut rnd = [0u8; 25];
    getrandom::fill(&mut rnd).expect("OS random numbers");
    let seed = u32::from_le_bytes(rnd[..4].try_into().unwrap());
    encrypt_with(plain, seed, &rnd[4..25])
}

pub fn encrypt_with(plain: &[u8], seed: u32, material: &[u8]) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(plain).expect("in-memory write");
    let mut body = enc.finish().expect("in-memory deflate");
    let mut header = Vec::with_capacity(40);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&0x400u32.to_le_bytes());
    header.extend_from_slice(&seed.to_le_bytes());
    header.extend_from_slice(&material[..21]);
    header.extend_from_slice(&[0, 0, 0]);
    let hc = header_cipher();
    for block in header.chunks_mut(8) {
        let mut ga = GenericArray::clone_from_slice(block);
        hc.encrypt_block(&mut ga);
        block.copy_from_slice(&ga);
    }
    keystream_xor(&mut body, seed, material, 0x400);
    header.extend_from_slice(&body);
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for n in [0usize, 1, 7, 8, 9, 5000] {
            let plain: Vec<u8> = (0..n).map(|i| (i * 31 % 251) as u8).collect();
            assert_eq!(decrypt(&encrypt(&plain)).unwrap(), plain);
        }
    }

    #[test]
    fn parity() {
        for k in des_key(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde]) {
            assert_eq!(k.count_ones() % 2, 1);
        }
    }
}
