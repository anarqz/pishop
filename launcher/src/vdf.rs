//! Minimal reader/writer for Steam's binary VDF (used by `shortcuts.vdf`).
//! Unknown value types are preserved byte-for-byte so a round trip never
//! corrupts entries written by other tools.

use std::io;

const MAP: u8 = 0x00;
const STR: u8 = 0x01;
const INT: u8 = 0x02;
const FLOAT: u8 = 0x03;
const PTR: u8 = 0x04;
const WSTR: u8 = 0x05;
const COLOR: u8 = 0x06;
const U64: u8 = 0x07;
const END: u8 = 0x08;
const I64: u8 = 0x0A;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Map(Vec<(Vec<u8>, Value)>),
    Str(Vec<u8>),
    Int(i32),
    /// Fixed-width scalar of another type (float, u64, …): `(type, raw bytes)`.
    Raw(u8, Vec<u8>),
}

impl Value {
    pub fn str(s: &str) -> Self {
        Value::Str(s.as_bytes().to_vec())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(items) => {
                items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes())).map(|(_, v)| v)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&[u8]> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("shortcuts.vdf inválido: {msg}"))
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> io::Result<u8> {
        let b = *self.buf.get(self.pos).ok_or_else(|| bad("fim inesperado"))?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len()).ok_or_else(|| bad("fim inesperado"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn cstr(&mut self) -> io::Result<Vec<u8>> {
        let rest = &self.buf[self.pos..];
        let len = rest.iter().position(|&b| b == 0).ok_or_else(|| bad("string sem terminador"))?;
        let s = rest[..len].to_vec();
        self.pos += len + 1;
        Ok(s)
    }

    fn map(&mut self) -> io::Result<Vec<(Vec<u8>, Value)>> {
        let mut items = Vec::new();
        loop {
            let t = self.byte()?;
            if t == END {
                return Ok(items);
            }
            let key = self.cstr()?;
            let value = match t {
                MAP => Value::Map(self.map()?),
                STR => Value::Str(self.cstr()?),
                INT => Value::Int(i32::from_le_bytes(self.take(4)?.try_into().unwrap())),
                FLOAT | PTR | COLOR => Value::Raw(t, self.take(4)?.to_vec()),
                U64 | I64 => Value::Raw(t, self.take(8)?.to_vec()),
                WSTR => return Err(bad("wide string não suportada")),
                other => return Err(bad(&format!("tipo desconhecido 0x{other:02x}"))),
            };
            items.push((key, value));
        }
    }
}

/// Parses a whole file; the root is a map closed by a trailing END byte.
pub fn parse(buf: &[u8]) -> io::Result<Value> {
    let mut r = Reader { buf, pos: 0 };
    Ok(Value::Map(r.map()?))
}

pub fn serialize(root: &Value) -> Vec<u8> {
    fn write_map(out: &mut Vec<u8>, items: &[(Vec<u8>, Value)]) {
        for (k, v) in items {
            let t = match v {
                Value::Map(_) => MAP,
                Value::Str(_) => STR,
                Value::Int(_) => INT,
                Value::Raw(t, _) => *t,
            };
            out.push(t);
            out.extend_from_slice(k);
            out.push(0);
            match v {
                Value::Map(m) => write_map(out, m),
                Value::Str(s) => {
                    out.extend_from_slice(s);
                    out.push(0);
                }
                Value::Int(i) => out.extend_from_slice(&i.to_le_bytes()),
                Value::Raw(_, b) => out.extend_from_slice(b),
            }
        }
        out.push(END);
    }
    let mut out = Vec::new();
    if let Value::Map(items) = root {
        write_map(&mut out, items);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let root = Value::Map(vec![(
            b"shortcuts".to_vec(),
            Value::Map(vec![(
                b"0".to_vec(),
                Value::Map(vec![
                    (b"appid".to_vec(), Value::Int(-123)),
                    (b"AppName".to_vec(), Value::str("x")),
                    (b"LastPlayTime".to_vec(), Value::Raw(U64, vec![1, 2, 3, 4, 5, 6, 7, 8])),
                    (b"tags".to_vec(), Value::Map(vec![])),
                ]),
            )]),
        )]);
        let bytes = serialize(&root);
        assert_eq!(parse(&bytes).unwrap(), root);
    }
}

#[cfg(test)]
mod real_file {
    /// `PISHOP_VDF=/path/shortcuts.vdf cargo test` checks a byte-exact round trip.
    #[test]
    fn round_trip_real_file() {
        let Some(path) = std::env::var_os("PISHOP_VDF") else { return };
        let buf = std::fs::read(path).unwrap();
        assert_eq!(super::serialize(&super::parse(&buf).unwrap()), buf);
    }
}
