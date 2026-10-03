//! Test-only: write PNGs of what a shader draws, so the result can be looked at.
//!
//! Set `ALLIO_SNAPSHOTS` to a directory and run the tests; nothing is written otherwise.

#![allow(
  clippy::unwrap_used,
  clippy::indexing_slicing,
  clippy::cast_possible_truncation
)]

use std::path::{Path, PathBuf};

/// Where to write snapshots, if anywhere.
pub(crate) fn dir() -> Option<PathBuf> {
  std::env::var_os("ALLIO_SNAPSHOTS").map(PathBuf::from)
}

fn crc32(data: &[u8]) -> u32 {
  let mut crc = 0xffff_ffff_u32;
  for &byte in data {
    crc ^= u32::from(byte);
    for _ in 0..8 {
      crc = if crc & 1 == 1 {
        (crc >> 1) ^ 0xedb8_8320
      } else {
        crc >> 1
      };
    }
  }
  !crc
}

fn adler32(data: &[u8]) -> u32 {
  let (mut a, mut b) = (1_u32, 0_u32);
  for &byte in data {
    a = (a + u32::from(byte)) % 65521;
    b = (b + a) % 65521;
  }
  (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
  out.extend_from_slice(&(data.len() as u32).to_be_bytes());
  let start = out.len();
  out.extend_from_slice(&kind);
  out.extend_from_slice(data);
  let crc = crc32(&out[start..]);
  out.extend_from_slice(&crc.to_be_bytes());
}

/// Writes an 8-bit RGBA PNG (uncompressed) of `w` x `h` pixels.
pub(crate) fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) {
  let mut raw = Vec::with_capacity((w * 4 + 1) * h);
  for row in rgba.chunks_exact(w * 4) {
    raw.push(0); // no filter
    raw.extend_from_slice(row);
  }
  let mut zlib = vec![0x78, 0x01];
  let mut blocks = raw.chunks(65535).peekable();
  while let Some(block) = blocks.next() {
    zlib.push(u8::from(blocks.peek().is_none()));
    zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
    zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
    zlib.extend_from_slice(block);
  }
  zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

  let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
  let mut header = Vec::new();
  header.extend_from_slice(&(w as u32).to_be_bytes());
  header.extend_from_slice(&(h as u32).to_be_bytes());
  header.extend_from_slice(&[8, 6, 0, 0, 0]);
  chunk(&mut png, *b"IHDR", &header);
  chunk(&mut png, *b"IDAT", &zlib);
  chunk(&mut png, *b"IEND", &[]);
  std::fs::write(path, png).unwrap();
}
