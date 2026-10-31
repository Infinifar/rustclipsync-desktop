//! Bounded decoding independent of the OS, also tested on Linux.
use rustclipsync_core::{adapters::ClipboardError, MAX_TEXT_BYTES};
pub fn decode_unicode(units: &[u16]) -> Result<String, ClipboardError> {
    let end = units
        .iter()
        .position(|u| *u == 0)
        .ok_or(ClipboardError::InvalidUtf8)?;
    if end > MAX_TEXT_BYTES {
        return Err(ClipboardError::TooLarge);
    }
    let bytes =
        char::decode_utf16(units[..end].iter().copied()).try_fold(0usize, |length, c| {
            let c = c.map_err(|_| ClipboardError::InvalidUtf8)?;
            let length = length + c.len_utf8();
            if length > MAX_TEXT_BYTES {
                Err(ClipboardError::TooLarge)
            } else {
                Ok(length)
            }
        })?;
    let mut text = String::with_capacity(bytes);
    for c in char::decode_utf16(units[..end].iter().copied()) {
        text.push(c.map_err(|_| ClipboardError::InvalidUtf8)?);
    }
    Ok(text)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_termination_and_invalid_surrogates() {
        let text = "中文 😀\r\n";
        let data: Vec<_> = text.encode_utf16().chain([0, 65]).collect();
        assert_eq!(decode_unicode(&data).unwrap(), text);
        assert!(decode_unicode(&[0xd800, 0]).is_err());
        assert!(decode_unicode(&[65]).is_err());
    }
    #[test]
    fn limit_counts_utf8_bytes() {
        let units: Vec<_> = std::iter::repeat_n(0x4e2d, MAX_TEXT_BYTES / 3 + 1)
            .chain([0])
            .collect();
        assert!(matches!(
            decode_unicode(&units),
            Err(ClipboardError::TooLarge)
        ));
    }
}
