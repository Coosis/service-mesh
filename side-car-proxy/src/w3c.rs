// see https://www.w3.org/TR/trace-context/ for more details

// 1. 32 bit hex
// 2. 16 bit hex
// traceparent:
// version - trace-id - parent-id - trace-flags
// 00-11111111111111111111111111111111-1111111111111111-01

use http::HeaderMap;

fn hex32(b: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 32];
    for i in 0..16 {
        out[i * 2] = HEX[(b[i] >> 4) as usize];
        out[i * 2 + 1] = HEX[(b[i] & 0x0f) as usize];
    }
    unsafe { String::from_utf8_unchecked(out.to_vec()) }
}

fn from_hex32(s: &[u8]) -> Option<[u8; 16]> {
    if s.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        let hi = s[i * 2];
        let lo = s[i * 2 + 1];
        let hi_val = match hi {
            b'0'..=b'9' => hi - b'0',
            b'a'..=b'f' => hi - b'a' + 10,
            b'A'..=b'F' => hi - b'A' + 10,
            _ => return None,
        };
        let lo_val = match lo {
            b'0'..=b'9' => lo - b'0',
            b'a'..=b'f' => lo - b'a' + 10,
            b'A'..=b'F' => lo - b'A' + 10,
            _ => return None,
        };
        out[i] = (hi_val << 4) | lo_val;
    }
    Some(out)
}

fn hex16(b: [u8; 8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 16];
    for i in 0..8 {
        out[i * 2] = HEX[(b[i] >> 4) as usize];
        out[i * 2 + 1] = HEX[(b[i] & 0x0f) as usize];
    }
    unsafe { String::from_utf8_unchecked(out.to_vec()) }
}

fn parse_w3c_traceparent(s: &str) -> Option<([u8; 16], String)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 4 {
        return None;
    }
    let version = parts[0];
    let trace_id = parts[1];
    let parent_id = parts[2];
    let trace_flags = parts[3];

    if version.len() != 2 || trace_id.len() != 32 || parent_id.len() != 16 || trace_flags.len() != 2 {
        return None;
    }

    Some((from_hex32(trace_id.as_bytes())?, trace_flags.to_string()))
}

pub fn ensure_w3c(headers: &mut HeaderMap) {
    let (trace_id, flags)  = match headers.get("traceparent")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_w3c_traceparent) {
            Some((tid, flags)) => (tid, flags),
            None => {
                let mut tid = [0u8; 16];
                let _ = getrandom::fill(&mut tid);
                while tid.iter().all(|&b| b == 0) {
                    let _ = getrandom::fill(&mut tid);
                }
                let flags = "01".to_string();
                (tid, flags)
            }
        };

    let mut sid = [0u8; 8];
    getrandom::fill(&mut sid).expect("getrandom span_id");
    while sid.iter().all(|&b| b == 0) {
        getrandom::fill(&mut sid).expect("getrandom span_id");
    }
    let traceparent = format!("00-{}-{}-{}", hex32(trace_id), hex16(sid), flags);
    let _ = headers.insert("traceparent", traceparent.parse().unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex32() {
        let b = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
            0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
        ];
        let s = hex32(b);
        assert_eq!(s, "112233445566778899aabbccddeeff00");
        let b2 = from_hex32(s.as_bytes()).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn test_hex16() {
        let b = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let s = hex16(b);
        assert_eq!(s, "1122334455667788");

        let b2 = [0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00];
        let s2 = hex16(b2);
        assert_eq!(s2, "99aabbccddeeff00");
    }
}
