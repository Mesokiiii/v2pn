//! Parse a newline-separated list of share URIs (optionally base64-wrapped).

use crate::error::CoreResult;
use crate::profile::ProxyProfile;
use crate::subscription::{base64::decode_loose, uri::parse_uri};

/// Parse a plain (non-base64) URI list. Skips lines we don't recognise.
pub fn parse_plain(body: &str) -> Vec<ProxyProfile> {
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| parse_uri(l).ok())
        .collect()
}

/// Parse a base64-wrapped URI list.
pub fn parse_base64(body: &str) -> CoreResult<Vec<ProxyProfile>> {
    let raw = decode_loose(body)?;
    let s = String::from_utf8_lossy(&raw);
    Ok(parse_plain(&s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine};

    #[test]
    fn parses_plain_list() {
        let body = "
            vless://550e8400-e29b-41d4-a716-446655440000@a.example:443?type=tcp&security=reality&pbk=AAAA&sni=x&fp=chrome&sid=12#A
            # this is a comment
            trojan://pass@b.example:8443?sni=y#B
        ";
        let v = parse_plain(body);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "A");
        assert_eq!(v[1].name, "B");
    }

    #[test]
    fn parses_base64_list() {
        let inner = "vless://550e8400-e29b-41d4-a716-446655440000@a.example:443?type=tcp&security=reality&pbk=AAAA&sni=x&fp=chrome&sid=12#A\ntrojan://pass@b.example:8443?sni=y#B\n";
        let blob = general_purpose::STANDARD.encode(inner);
        let v = parse_base64(&blob).unwrap();
        assert_eq!(v.len(), 2);
    }
}
