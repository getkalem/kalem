//! JSON-RPC 2.0 over a byte stream with the protocol's `Content-Length`
//! headers.

use std::io::{self, BufRead, Write};

use serde_json::Value;

/// Reads one message; `Ok(None)` at the end of the stream. A message that
/// is not JSON is an error of kind `InvalidData`, after which the stream
/// is still in step (the body was read whole).
pub fn read(r: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if length.is_some() {
                break;
            }
            // Blank lines before the headers are tolerated.
            continue;
        }
        if let Some((name, value)) = l.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok();
        }
    }
    let length = length.unwrap_or(0);
    let mut body = vec![0; length];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes one message.
pub fn write(w: &mut impl Write, msg: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_and_malformed() {
        let mut buf = Vec::new();
        write(&mut buf, &json!({"jsonrpc": "2.0", "id": 1, "result": "ü"})).unwrap();
        buf.extend_from_slice(b"Content-Length: 3\r\n\r\n{x}");
        write(&mut buf, &json!({"n": 2})).unwrap();
        let mut r = io::Cursor::new(buf);
        assert_eq!(read(&mut r).unwrap().unwrap()["result"], "ü");
        assert_eq!(read(&mut r).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(read(&mut r).unwrap().unwrap()["n"], 2);
        assert!(read(&mut r).unwrap().is_none());
    }
}
