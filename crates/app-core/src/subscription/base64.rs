//! Base64 helpers tolerant to subscription-format quirks.
//!
//! Real-world subscription bodies use:
//!  - standard alphabet
//!  - URL-safe alphabet
//!  - missing or extra padding
//!  - whitespace/newlines inside the payload
//!
//! `decode_loose` accepts all of the above.

use base64::engine::{general_purpose, Engine as _};

use crate::error::{CoreError, CoreResult};

pub fn decode_loose(input: &str) -> CoreResult<Vec<u8>> {
    // strip whitespace
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }

    // pick alphabet
    let url_safe = cleaned.contains('-') || cleaned.contains('_');

    // pad to multiple of 4
    let mut padded = cleaned;
    while padded.len() % 4 != 0 {
        padded.push('=');
    }

    let engine = if url_safe {
        general_purpose::URL_SAFE
    } else {
        general_purpose::STANDARD
    };
    engine
        .decode(padded.as_bytes())
        .map_err(CoreError::Base64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_standard() {
        let s = general_purpose::STANDARD.encode(b"hello");
        assert_eq!(decode_loose(&s).unwrap(), b"hello");
    }

    #[test]
    fn decodes_url_safe_no_padding() {
        let s = general_purpose::URL_SAFE_NO_PAD.encode(b"hello world!");
        assert_eq!(decode_loose(&s).unwrap(), b"hello world!");
    }

    #[test]
    fn ignores_whitespace() {
        let s = format!("{}\n  ", general_purpose::STANDARD.encode(b"hello"));
        assert_eq!(decode_loose(&s).unwrap(), b"hello");
    }
}
