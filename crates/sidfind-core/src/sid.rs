//! Minimal PSID/RSID parsing: just enough to extract the C64 data a tune loads into memory.

const MIN_HEADER: usize = 0x76;

pub fn is_sid(file: &[u8]) -> bool {
    file.len() >= MIN_HEADER && (file.starts_with(b"PSID") || file.starts_with(b"RSID"))
}

/// Returns the tune data without the SID header. When the header's load address is 0, the
/// first two data bytes are the real load address (little endian), as in a PRG, and are skipped.
pub fn payload(file: &[u8]) -> Option<&[u8]> {
    if !is_sid(file) {
        return None;
    }
    let data_offset = u16::from_be_bytes([file[6], file[7]]) as usize;
    let header_load = u16::from_be_bytes([file[8], file[9]]);
    let body = file.get(data_offset.max(MIN_HEADER)..)?;
    if header_load == 0 { split_prg(body).map(|(_, data)| data) } else { Some(body) }
}

/// Splits a PRG into its load address and data.
pub fn split_prg(file: &[u8]) -> Option<(u16, &[u8])> {
    match file {
        [lo, hi, data @ ..] => Some((u16::from_le_bytes([*lo, *hi]), data)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fake_sid(header_load: u16, body: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; MIN_HEADER];
        f[..4].copy_from_slice(b"PSID");
        f[6..8].copy_from_slice(&(MIN_HEADER as u16).to_be_bytes());
        f[8..10].copy_from_slice(&header_load.to_be_bytes());
        f.extend_from_slice(body);
        f
    }

    #[test]
    fn header_load_address_keeps_all_data() {
        let f = fake_sid(0x1000, &[1, 2, 3]);
        assert_eq!(payload(&f).unwrap(), &[1u8, 2, 3][..]);
    }

    #[test]
    fn zero_load_address_skips_two_bytes() {
        let f = fake_sid(0, &[0x00, 0x10, 9, 8]);
        assert_eq!(payload(&f).unwrap(), &[9u8, 8][..]);
    }

    #[test]
    fn rejects_non_sid() {
        assert!(payload(b"not a sid at all").is_none());
        assert!(payload(&[0x01, 0x08, 0x0b]).is_none());
    }
}
