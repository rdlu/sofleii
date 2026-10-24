//! UF2 (Microsoft's USB flashing format) as far as `CURRENT.UF2` needs it:
//! 512-byte blocks, each carrying up to 476 bytes for one flash address.

/// Block layout constants from the UF2 spec.
const BLOCK: usize = 512;
const MAGIC_START0: u32 = 0x0A32_4655;
const MAGIC_START1: u32 = 0x9E5D_5157;
const MAGIC_END: u32 = 0x0AB1_6F30;
const MAX_PAYLOAD: usize = 476;
/// `flags`: the block is not for main flash (comments, padding).
const FLAG_NOT_MAIN_FLASH: u32 = 0x0000_0001;

/// The nRF52840 has 1 MiB of flash; a larger span is not a real image.
const MAX_SPAN: usize = 1 << 20;

/// A flash image rebuilt from UF2 blocks: `bytes[0]` is at address `base`,
/// gaps between blocks read as erased flash (`0xFF`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub base: u32,
    pub bytes: Vec<u8>,
}

impl Image {
    /// The little-endian word at `offset` from `base`, if the image has it.
    pub fn word(&self, offset: usize) -> Option<u32> {
        let b = self.bytes.get(offset..offset.checked_add(4)?)?;
        Some(u32::from_le_bytes(b.try_into().ok()?))
    }
}

fn le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().expect("4 bytes"))
}

/// Parses a whole UF2 file. Fails on a size that is not whole blocks, a bad
/// magic, a payload over 476 bytes, no main-flash block, or an address span
/// larger than the chip's flash.
pub fn parse(data: &[u8]) -> Result<Image, String> {
    if data.is_empty() || !data.len().is_multiple_of(BLOCK) {
        return Err(format!(
            "{} bytes is not a whole number of 512-byte UF2 blocks",
            data.len()
        ));
    }
    let mut blocks = Vec::with_capacity(data.len() / BLOCK);
    for (n, b) in data.as_chunks::<BLOCK>().0.iter().enumerate() {
        if le(b, 0) != MAGIC_START0 || le(b, 1) != MAGIC_START1 || le(b, 127) != MAGIC_END {
            return Err(format!("block {n} has no UF2 magic"));
        }
        let (flags, addr, size) = (le(b, 2), le(b, 3), le(b, 4) as usize);
        if flags & FLAG_NOT_MAIN_FLASH != 0 {
            continue;
        }
        if size > MAX_PAYLOAD {
            return Err(format!("block {n} claims a {size}-byte payload"));
        }
        blocks.push((addr, &b[32..32 + size]));
    }
    let base = blocks
        .iter()
        .map(|&(a, _)| a)
        .min()
        .ok_or("no main-flash blocks")?;
    let end = blocks
        .iter()
        .map(|&(a, p)| u64::from(a) + p.len() as u64)
        .max()
        .expect("non-empty");
    let span = (end - u64::from(base)) as usize;
    if span > MAX_SPAN {
        return Err(format!(
            "blocks span {span} bytes, more than the chip's flash"
        ));
    }
    let mut bytes = vec![0xFF; span];
    for (addr, payload) in blocks {
        let at = (addr - base) as usize;
        bytes[at..at + payload.len()].copy_from_slice(payload);
    }
    Ok(Image { base, bytes })
}

/// Encodes `bytes` at `base` as UF2 blocks of 256 bytes (what the
/// bootloader's `CURRENT.UF2` uses); for tests.
#[cfg(test)]
pub(crate) fn encode(base: u32, bytes: &[u8], family: u32) -> Vec<u8> {
    let chunks: Vec<&[u8]> = bytes.chunks(256).collect();
    let mut out = Vec::with_capacity(chunks.len() * BLOCK);
    for (n, c) in chunks.iter().enumerate() {
        let mut b = [0u8; BLOCK];
        let words = [
            MAGIC_START0,
            MAGIC_START1,
            0x2000, // family ID present
            base + (n * 256) as u32,
            c.len() as u32,
            n as u32,
            chunks.len() as u32,
            family,
        ];
        for (i, w) in words.iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        b[32..32 + c.len()].copy_from_slice(c);
        b[508..].copy_from_slice(&MAGIC_END.to_le_bytes());
        out.extend_from_slice(&b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_an_image() {
        let bytes: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        let img = parse(&encode(0x26000, &bytes, 0xADA5_2840)).unwrap();
        assert_eq!(img.base, 0x26000);
        assert_eq!(img.bytes, bytes);
        assert_eq!(img.word(0), Some(u32::from_le_bytes([0, 1, 2, 3])));
        assert_eq!(img.word(998), None);
    }

    #[test]
    fn blocks_out_of_order_and_gaps() {
        let mut uf2 = encode(0x1000, &[0xAA; 256], 0);
        let mut later = encode(0x1200, &[0xBB; 256], 0);
        later.extend_from_slice(&uf2);
        uf2 = later;
        let img = parse(&uf2).unwrap();
        assert_eq!(img.base, 0x1000);
        assert_eq!(img.bytes.len(), 0x300);
        assert_eq!(&img.bytes[..256], &[0xAA; 256]);
        assert_eq!(&img.bytes[256..512], &[0xFF; 256]);
        assert_eq!(&img.bytes[512..], &[0xBB; 256]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(&[]).is_err());
        assert!(parse(&[0; 100]).is_err());
        assert!(parse(&[0; 512]).unwrap_err().contains("magic"));
        let mut uf2 = encode(0, &[0; 16], 0);
        uf2[16..20].copy_from_slice(&477u32.to_le_bytes());
        assert!(parse(&uf2).unwrap_err().contains("payload"));
        let mut far = encode(0, &[0; 16], 0);
        far.extend(encode(2 << 20, &[0; 16], 0));
        assert!(parse(&far).unwrap_err().contains("span"));
    }

    #[test]
    fn skips_blocks_not_for_main_flash() {
        let mut uf2 = encode(0x2000, &[1; 16], 0);
        let mut note = encode(0, &[2; 16], 0);
        note[8..12].copy_from_slice(&(0x2000 | FLAG_NOT_MAIN_FLASH).to_le_bytes());
        uf2.extend(note);
        let img = parse(&uf2).unwrap();
        assert_eq!((img.base, img.bytes.as_slice()), (0x2000, &[1u8; 16][..]));
    }
}
