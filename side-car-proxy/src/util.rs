use hyper::header::{CONNECTION, TE, TRANSFER_ENCODING, UPGRADE, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION};

pub fn strip_hop_by_hop(headers: &mut hyper::HeaderMap) {
    let conns = headers.get_all(CONNECTION).iter()
        .filter_map(|hv| hv.to_str().ok())
        .flat_map(|s| s.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()))
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();

    for h in [CONNECTION, TE, TRANSFER_ENCODING, UPGRADE, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION] {
        headers.remove(h);
    }

    for name in conns {
        if let Ok(hn) = hyper::header::HeaderName::from_bytes(name.as_bytes()) {
            headers.remove(hn);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_strip_hop_by_hop() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(CONNECTION, "keep-alive, Upgrade".parse().unwrap());
        headers.insert(TE, "trailers".parse().unwrap());
        headers.insert(TRANSFER_ENCODING, "chunked".parse().unwrap());
        headers.insert(UPGRADE, "websocket".parse().unwrap());
        headers.insert(PROXY_AUTHENTICATE, "Basic realm=\"somehting\"".parse().unwrap());
        headers.insert(PROXY_AUTHORIZATION, "Basic dXNlcjpwYXNzd29yZA==".parse().unwrap());

        strip_hop_by_hop(&mut headers);

        assert!(!headers.contains_key(CONNECTION));
        assert!(!headers.contains_key(TE));
        assert!(!headers.contains_key(TRANSFER_ENCODING));
        assert!(!headers.contains_key(UPGRADE));
        assert!(!headers.contains_key(PROXY_AUTHENTICATE));
        assert!(!headers.contains_key(PROXY_AUTHORIZATION));
    }
}
