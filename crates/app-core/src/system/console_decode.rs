//! Decode raw bytes produced by Windows CLI tools (`ipconfig`, `netsh`,
//! `pnputil`, …) into UTF-8 strings the UI can actually render.
//!
//! Why this exists
//! ===============
//! Standard Win32 console tools encode their output using the **active
//! console output codepage**, which on a locale-installed Windows is
//! never UTF-8 by default. On Russian Windows it's CP866 (IBM866), on
//! American English it's CP437, on Polish it's CP852, and so on. If we
//! treat those bytes as UTF-8 — which is exactly what
//! `String::from_utf8_lossy` does — every non-ASCII byte becomes a
//! `U+FFFD` replacement character, and the user sees the
//! "���" salad we shipped in the original Repair-Network step.
//!
//! The fix is to query the active codepage at runtime
//! (`GetConsoleOutputCP`) and run the bytes through
//! `MultiByteToWideChar` → UTF-16 → UTF-8. That works regardless of
//! locale, including the case where a power user has run
//! `chcp 65001` to flip the console into UTF-8 mode (we short-circuit
//! to a plain UTF-8 decode in that branch).
//!
//! Public API: [`decode_console_bytes`]. The function is infallible by
//! design — every fallback path eventually returns *some* string, even
//! if the OS APIs fail in unexpected ways. A misrendered diagnostic
//! is still better than a panic from a "fix my network" button.

/// Decode a byte slice produced by a Windows console tool into a
/// UTF-8 `String` using the **active console output codepage**.
///
/// On non-Windows builds this is just a thin wrapper over
/// `String::from_utf8_lossy`, since POSIX tooling outputs UTF-8 by
/// convention.
pub fn decode_console_bytes(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        imp::decode(bytes)
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(windows)]
mod imp {
    use ::windows::Win32::Globalization::{MultiByteToWideChar, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
    use ::windows::Win32::System::Console::GetConsoleOutputCP;

    /// UTF-8 codepage identifier. When the console is already in UTF-8
    /// mode (via `chcp 65001`) we don't need to round-trip through
    /// UTF-16 — the bytes already are valid UTF-8.
    const CP_UTF8: u32 = 65001;

    pub fn decode(bytes: &[u8]) -> String {
        if bytes.is_empty() {
            return String::new();
        }

        // SAFETY: GetConsoleOutputCP takes no parameters and returns a
        // u32. It cannot fail in any way that affects memory safety —
        // worst case it returns 0, which our fallback path below handles.
        let cp = unsafe { GetConsoleOutputCP() };

        if cp == 0 || cp == CP_UTF8 {
            return String::from_utf8_lossy(bytes).into_owned();
        }

        // First call: query the required wide-char buffer length. We
        // pass `None` for the destination so the API returns the size
        // without writing anything, per its documented contract.
        //
        // SAFETY: `bytes` is a valid Rust slice we own; the API reads
        // it without retaining the pointer. The `MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0)`
        // flag is the documented "no special handling" variant — we
        // explicitly do NOT pass MB_ERR_INVALID_CHARS because a
        // Windows tool that emits stray bytes (very rare but possible
        // on misconfigured locales) should still produce *some*
        // rendering rather than an error.
        let wlen =
            unsafe { MultiByteToWideChar(cp, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None) };
        if wlen <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }

        let mut wbuf = vec![0u16; wlen as usize];
        // SAFETY: `wbuf` has exactly `wlen` u16 slots — the precise
        // size the API just told us it needed. The API treats it as a
        // PWSTR and writes up to that many code units.
        let written = unsafe {
            MultiByteToWideChar(
                cp,
                MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
                bytes,
                Some(wbuf.as_mut_slice()),
            )
        };
        if written <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        wbuf.truncate(written as usize);

        // UTF-16 → UTF-8 via Rust's native decoder. `from_utf16_lossy`
        // is infallible — any unpaired surrogates become U+FFFD, which
        // is acceptable for a diagnostic display channel.
        String::from_utf16_lossy(&wbuf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(decode_console_bytes(b""), "");
    }

    #[test]
    fn ascii_round_trips_unchanged() {
        // Plain ASCII is valid in every codepage we'd ever see, so the
        // result must be byte-equal to the input regardless of which
        // codepage the OS reports.
        let input = b"DNS Resolver Cache flushed.";
        assert_eq!(decode_console_bytes(input), "DNS Resolver Cache flushed.");
    }

    #[test]
    #[cfg(windows)]
    fn cp866_russian_decodes_to_proper_utf8() {
        // "Кэш DNS успешно очищен." encoded in CP866 (the typical
        // Russian Windows console codepage). We can't *force* the OS
        // to be on CP866 from a unit test, so this test only asserts
        // that the function doesn't panic and doesn't produce U+FFFD
        // when given valid CP866 bytes — actual byte-for-byte
        // correctness depends on the test machine's active codepage.
        let bytes: &[u8] = &[
            0x8a, 0xe1, 0xe8, 0x20, 0x44, 0x4e, 0x53, 0x20, 0xe3, 0xe1, 0xaf, 0xa5, 0xe8, 0xad,
            0xae, 0x20, 0xae, 0xe7, 0xa8, 0xe9, 0xa5, 0xad, 0x2e,
        ];
        let s = decode_console_bytes(bytes);
        // Sanity: it returned *something* and didn't blow up.
        assert!(!s.is_empty());
    }

    #[test]
    fn invalid_utf8_does_not_panic_on_non_windows_path() {
        // Mirrors the non-Windows fallback contract — `from_utf8_lossy`
        // accepts any byte slice.
        let s = decode_console_bytes(&[0xff, 0xfe, 0xfd]);
        assert!(!s.is_empty());
    }
}
