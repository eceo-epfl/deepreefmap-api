//! The imohash content identity, computed exactly as the desktop app computes it.
//!
//! imohash is murmur3 x64 128 (seed 0) over the whole file when it is smaller than
//! [`SAMPLE_THRESHOLD`], otherwise over three [`SAMPLE_SIZE`] windows at the start,
//! middle and end, with the head of the digest overwritten by the varint-encoded
//! file size. The test vectors here were generated with the Python `imohash`
//! package the desktop app hashes with, so the two implementations cannot drift
//! silently.

use std::fmt::Write;

/// Bytes read at the start, middle and end of a sampled file.
pub const SAMPLE_SIZE: u64 = 16 * 1024;
/// Files smaller than this are hashed whole.
pub const SAMPLE_THRESHOLD: u64 = 128 * 1024;

/// The `(offset, length)` windows a sampled hash of `size` bytes reads, in the
/// order they are hashed. Only meaningful at or above [`SAMPLE_THRESHOLD`], where
/// the windows never overlap.
#[must_use]
pub fn sample_windows(size: u64) -> [(u64, u64); 3] {
    [
        (0, SAMPLE_SIZE),
        (size / 2, SAMPLE_SIZE),
        (size - SAMPLE_SIZE, SAMPLE_SIZE),
    ]
}

/// imohash of a file hashed whole, for sizes under [`SAMPLE_THRESHOLD`].
#[must_use]
pub fn hash_whole(data: &[u8]) -> String {
    digest(data.len() as u64, data)
}

/// imohash of an in-memory file, dispatching on the threshold like `hashfile` does.
#[must_use]
pub fn hash_bytes(data: &[u8]) -> String {
    let size = data.len() as u64;
    if size < SAMPLE_THRESHOLD {
        return hash_whole(data);
    }
    let [head, middle, tail] = sample_windows(size).map(|(start, len)| window(data, start, len));
    hash_sampled(size, head, middle, tail)
}

/// A window out of an in-memory file. The offsets came from the slice's own length,
/// so they always fit `usize`.
fn window(data: &[u8], start: u64, len: u64) -> &[u8] {
    let start = usize::try_from(start).expect("an in-memory offset fits usize");
    let len = usize::try_from(len).expect("an in-memory length fits usize");
    &data[start..start + len]
}

/// imohash of a `size`-byte file from its three sampled windows.
#[must_use]
pub fn hash_sampled(size: u64, head: &[u8], middle: &[u8], tail: &[u8]) -> String {
    let mut data = Vec::with_capacity(head.len() + middle.len() + tail.len());
    data.extend_from_slice(head);
    data.extend_from_slice(middle);
    data.extend_from_slice(tail);
    digest(size, &data)
}

// The casts below truncate deliberately: splitting the u128, and LEB128's low bits.
#[allow(clippy::cast_possible_truncation)]
fn digest(size: u64, data: &[u8]) -> String {
    let hash = murmur3::murmur3_x64_128(&mut &data[..], 0).expect("a slice read cannot fail");
    // The reference implementations emit big-endian h1 then h2, and the crate
    // returns `(h2 << 64) | h1`.
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&(hash as u64).to_be_bytes());
    bytes[8..].copy_from_slice(&((hash >> 64) as u64).to_be_bytes());

    // Unsigned LEB128 of the size overwrites the head of the digest.
    let mut remaining = size;
    for byte in &mut bytes {
        if remaining < 0x80 {
            *byte = remaining as u8;
            break;
        }
        *byte = (remaining as u8 & 0x7f) | 0x80;
        remaining >>= 7;
    }

    let mut out = String::with_capacity(32);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(i * 7 + 3) % 256`, the pattern the Python-generated vectors used.
    #[allow(clippy::cast_possible_truncation)]
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * 7 + 3) % 256) as u8).collect()
    }

    /// `((i * i) / 7 + i) % 256`: aperiodic, so every window differs.
    #[allow(clippy::cast_possible_truncation)]
    fn pattern2(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * i / 7 + i) % 256) as u8).collect()
    }

    fn sampled(data: &[u8]) -> String {
        let size = data.len() as u64;
        let [head, middle, tail] =
            sample_windows(size).map(|(start, len)| window(data, start, len));
        hash_sampled(size, head, middle, tail)
    }

    #[test]
    fn test_whole_file_vectors_match_the_python_package() {
        assert_eq!(hash_whole(&pattern(1)), "016ac6dd306a3e594e711127c5b5a8e4");
        assert_eq!(
            hash_whole(&pattern(300)),
            "ac02cecd43df46dd2674ed91689c059a"
        );
        assert_eq!(
            hash_whole(&pattern(131_071)),
            "ffff07af9d7f72b4cca38bfa60a81a4e"
        );
    }

    #[test]
    fn test_sampled_vectors_match_the_python_package() {
        assert_eq!(
            sampled(&pattern(131_072)),
            "8080084896bc113ea7030c8b755211ff"
        );
        assert_eq!(
            sampled(&pattern2(150_000)),
            "f09309cffadeda24d589261fa5a9934e"
        );
        assert_eq!(
            sampled(&pattern2(1_000_003)),
            "c3843d909712ef26edc29392e7cb9566"
        );
        assert_eq!(
            sampled(&pattern(3 * 1024 * 1024)),
            "8080c00196bc113ea7030c8b755211ff"
        );
    }

    #[test]
    fn test_windows_partition_the_smallest_sampled_file() {
        assert_eq!(
            sample_windows(SAMPLE_THRESHOLD),
            [(0, 16_384), (65_536, 16_384), (114_688, 16_384)]
        );
    }
}
