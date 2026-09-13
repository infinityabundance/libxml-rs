//! Minimal HTTP/1.0 client for the built-in input loader.
//!
//! Upstream libxml2 routes `http://` URLs through `nanohttp.c` from the
//! default input-callback table (`xmlIOHTTPMatch`/`xmlIOHTTPOpen`,
//! registered by `xmlRegisterDefaultInputCallbacks`). The crate previously
//! had no network stack, so `http://` filenames fell through to a plain
//! `open(2)` and failed with `ENOENT` ("failed to load …: No such file or
//! directory").
//!
//! This module implements the data plane the loader needs: a bounded
//! HTTP/1.0 GET with redirect following and `Content-Encoding`/chunked
//! decoding. It deliberately mirrors upstream's transport choices:
//!
//! * HTTP/1.0 + `Connection: close`, so the body is delimited by EOF exactly
//!   like `nanohttp`'s non-persistent connection.
//! * `Accept-Encoding: gzip`; `Content-Encoding: gzip` is decoded with
//!   DEFLATE (upstream links zlib for the same purpose).
//! * Redirects are followed for GET (301/302/303/307/308), against the
//!   response's request URI.
//!
//! Only the cleartext `http` scheme is handled, matching `nanohttp.c`
//! (which never implements TLS). Any error is returned as `Err`; the caller
//! reports the ordinary `xmlCtxtErrIO`/`failed to load` diagnostic, so a
//! failed fetch is never silently treated as an empty document.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Maximum number of redirects followed before giving up.
const MAX_REDIRECTS: usize = 5;
/// Connect and per-read/write timeouts, so a hung peer cannot wedge a parse.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const IO_TIMEOUT: Duration = Duration::from_secs(60);
/// Upper bound on the response header block (defends against a peer that
/// never sends the header terminator).
const MAX_HEADER_BYTES: usize = 256 * 1024;

/// A parsed cleartext HTTP URL.
struct ParsedUrl {
    /// Authority as sent in the `Host` header (host plus non-default port).
    host_header: String,
    /// Host name or IP literal to connect to.
    host: String,
    port: u16,
    /// Origin-form request target (`/path?query`).
    target: String,
}

/// Fetch `url` (an `http://` URL) and return the decoded body.
///
/// Returns `Err` for a non-2xx final status, a malformed URL, a transport
/// failure, or a decode failure.
pub(crate) fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let mut current = url.to_string();
    let mut redirects = 0usize;
    loop {
        let parsed =
            parse_http_url(&current).ok_or_else(|| format!("unsupported URL \"{current}\""))?;
        let (status, headers, body) = request_once(&parsed)?;
        match status {
            301 | 302 | 303 | 307 | 308 => {
                let location = headers
                    .iter()
                    .find(|(k, _)| k == "location")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| "HTTP redirect without a Location header".to_string())?;
                if redirects >= MAX_REDIRECTS {
                    return Err("too many HTTP redirects".to_string());
                }
                redirects += 1;
                current = resolve_location(&current, &location)?;
                continue;
            }
            200..=299 => return decode_body(&headers, body),
            other => return Err(format!("HTTP request failed with status {other}")),
        }
    }
}

/// Perform one GET and return `(status, headers, raw body)`.
fn request_once(parsed: &ParsedUrl) -> Result<(u32, Vec<(String, String)>, Vec<u8>), String> {
    let addr = (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {}: {e}", parsed.host))?
        .next()
        .ok_or_else(|| format!("cannot resolve {}", parsed.host))?;

    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("cannot connect to {}: {e}", parsed.host))?;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    // HTTP/1.0 + Connection: close delimit the body by EOF, matching
    // nanohttp's non-persistent request.
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: libxml2/2.15.3\r\nAccept: */*\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
        parsed.target, parsed.host_header
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("cannot send HTTP request: {e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("cannot read HTTP response: {e}"))?;

    parse_response(&raw)
}

/// Split a raw HTTP response into status, lower-cased headers, and body.
fn parse_response(raw: &[u8]) -> Result<(u32, Vec<(String, String)>, Vec<u8>), String> {
    let header_end = find_subslice(raw, b"\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response (no header terminator)".to_string())?;
    if header_end > MAX_HEADER_BYTES {
        return Err("HTTP response header too large".to_string());
    }
    let head = &raw[..header_end];
    let body = raw[header_end + 4..].to_vec();

    let head_str = String::from_utf8_lossy(head);
    let mut lines = head_str.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "empty HTTP response".to_string())?;
    let mut status_parts = status_line.split_whitespace();
    let _http_version = status_parts.next();
    let status: u32 = status_parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("malformed HTTP status line \"{status_line}\""))?;

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    Ok((status, headers, body))
}

/// Apply `Transfer-Encoding` and `Content-Encoding` to a raw body.
fn decode_body(headers: &[(String, String)], mut body: Vec<u8>) -> Result<Vec<u8>, String> {
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.trim().to_ascii_lowercase())
    };

    if let Some(te) = header("transfer-encoding") {
        if te.split(',').any(|t| t.trim() == "chunked") {
            body = dechunk(&body)?;
        }
    }

    let encoding = header("content-encoding").unwrap_or_default();
    for token in encoding.split(',').map(|t| t.trim()) {
        match token {
            "" | "identity" => {}
            "gzip" | "x-gzip" => {
                let mut out = Vec::new();
                flate2::read::GzDecoder::new(&body[..])
                    .read_to_end(&mut out)
                    .map_err(|e| format!("cannot decode gzip response body: {e}"))?;
                body = out;
            }
            "deflate" => {
                // Some servers send raw DEFLATE despite the header; try the
                // zlib wrapper first, then fall back to raw DEFLATE.
                let mut out = Vec::new();
                if flate2::read::ZlibDecoder::new(&body[..])
                    .read_to_end(&mut out)
                    .is_err()
                {
                    out.clear();
                    flate2::read::DeflateDecoder::new(&body[..])
                        .read_to_end(&mut out)
                        .map_err(|e| format!("cannot decode deflate response body: {e}"))?;
                }
                body = out;
            }
            other => {
                return Err(format!("unsupported Content-Encoding \"{other}\""));
            }
        }
    }
    Ok(body)
}

/// Decode HTTP/1.1 chunked transfer coding.
fn dechunk(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    loop {
        let line_end = find_subslice(&body[pos..], b"\r\n")
            .ok_or_else(|| "malformed chunked body (no size line)".to_string())?
            + pos;
        let size_line = String::from_utf8_lossy(&body[pos..line_end]);
        let size_token = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_token, 16)
            .map_err(|_| format!("malformed chunk size \"{size_token}\""))?;
        pos = line_end + 2;
        if size == 0 {
            return Ok(out);
        }
        if pos + size > body.len() {
            return Err("truncated chunked body".to_string());
        }
        out.extend_from_slice(&body[pos..pos + size]);
        pos += size;
        // Skip the CRLF that terminates the chunk data.
        if body.get(pos..pos + 2) == Some(b"\r\n") {
            pos += 2;
        }
    }
}

/// Resolve a redirect `Location` against the request URI.
fn resolve_location(base: &str, location: &str) -> Result<String, String> {
    if location.starts_with("http://") || location.starts_with("HTTP://") {
        return Ok(location.to_string());
    }
    let parsed = parse_http_url(base).ok_or_else(|| "invalid redirect base URL".to_string())?;
    let scheme_host = format!("http://{}", parsed.host_header);
    if let Some(rest) = location.strip_prefix("//") {
        return Ok(format!("http://{rest}"));
    }
    if location.starts_with('/') {
        return Ok(format!("{scheme_host}{location}"));
    }
    // Relative to the directory of the request target.
    let dir = match parsed.target.rfind('/') {
        Some(i) => &parsed.target[..=i],
        None => "/",
    };
    Ok(format!("{scheme_host}{dir}{location}"))
}

/// Parse a cleartext `http://` URL into connect parameters and request target.
fn parse_http_url(url: &str) -> Option<ParsedUrl> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("HTTP://"))?;
    // Drop any fragment.
    let rest = rest.split('#').next().unwrap_or(rest);
    // Split authority from path/query.
    let (authority, remainder) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    // Drop userinfo (nanohttp sends no Authorization unless configured).
    let authority = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    if authority.is_empty() {
        return None;
    }

    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        // IPv6 literal.
        let close = rest.find(']')?;
        let host = &rest[..close];
        let after = &rest[close + 1..];
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None => 80,
        };
        (host.to_string(), port)
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        // A bare `:` split is only a port when the suffix is numeric;
        // otherwise the colon is part of the host (unbracketed IPv6 is
        // invalid in a URL, but be defensive).
        match p.parse::<u16>() {
            Ok(port) => (h.to_string(), port),
            Err(_) => (authority.to_string(), 80),
        }
    } else {
        (authority.to_string(), 80)
    };

    let target = if remainder.is_empty() {
        "/".to_string()
    } else if remainder.starts_with('/') {
        remainder.to_string()
    } else {
        format!("/{remainder}")
    };

    let host_header = if port == 80 {
        host.clone()
    } else {
        format!("{host}:{port}")
    };

    Some(ParsedUrl {
        host_header,
        host,
        port,
        target,
    })
}

/// Index of the first occurrence of `needle` in `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_url() {
        let p = parse_http_url("http://127.0.0.1:8080/a/b?x=1").unwrap();
        assert_eq!(p.host, "127.0.0.1");
        assert_eq!(p.port, 8080);
        assert_eq!(p.host_header, "127.0.0.1:8080");
        assert_eq!(p.target, "/a/b?x=1");
    }

    #[test]
    fn parses_default_port_and_root() {
        let p = parse_http_url("http://example.com").unwrap();
        assert_eq!(p.port, 80);
        assert_eq!(p.host_header, "example.com");
        assert_eq!(p.target, "/");
    }

    #[test]
    fn parses_ipv6_literal() {
        let p = parse_http_url("http://[::1]:9000/x").unwrap();
        assert_eq!(p.host, "::1");
        assert_eq!(p.port, 9000);
        assert_eq!(p.target, "/x");
    }

    #[test]
    fn rejects_other_schemes() {
        assert!(parse_http_url("https://example.com/").is_none());
        assert!(parse_http_url("file:///tmp/x").is_none());
    }

    #[test]
    fn dechunks_a_body() {
        assert_eq!(
            dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(),
            b"Wikipedia"
        );
    }

    #[test]
    fn raises_on_bad_chunk_size() {
        assert!(dechunk(b"zz\r\n").is_err());
    }

    #[test]
    fn resolves_relative_redirects() {
        assert_eq!(
            resolve_location("http://h/dir/a", "/other").unwrap(),
            "http://h/other"
        );
        assert_eq!(
            resolve_location("http://h/dir/a", "b").unwrap(),
            "http://h/dir/b"
        );
        assert_eq!(
            resolve_location("http://h/dir/a", "http://x/y").unwrap(),
            "http://x/y"
        );
    }

    #[test]
    fn parses_status_and_headers() {
        let raw = b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\nX-Test: a\r\n\r\n";
        let (status, headers, body) = parse_response(raw).unwrap();
        assert_eq!(status, 200);
        assert!(body.is_empty());
        assert_eq!(
            headers,
            vec![
                ("content-length".to_string(), "0".to_string()),
                ("x-test".to_string(), "a".to_string()),
            ]
        );
    }
}
