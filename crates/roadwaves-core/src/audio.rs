use std::io::{self, Read};

/// Minimal standards-safe MP3 fixture and endpoint response validation.
pub const SILENT_MP3: &[u8] = b"\xff\xfb\x90\x64\x00\x00\x00\x00\x00\x00\x00\x00";
pub fn is_mp3_prefix(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && ((bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0) || bytes.starts_with(b"ID3"))
}
pub fn looks_like_mp3(mut input: impl Read) -> io::Result<bool> {
    let mut h = [0u8; 2];
    input.read_exact(&mut h)?;
    Ok(h == [0xff, 0xfb] || h == [0xff, 0xf3] || h == [0xff, 0xf2] || h == [0x49, 0x44])
}
