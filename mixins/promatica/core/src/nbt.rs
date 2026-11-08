//! A small reader for Java Edition's binary NBT (big endian), enough for `.litematic` files.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Tag>),
    Compound(BTreeMap<String, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(map) => map.get(key),
            _ => None,
        }
    }
    pub fn compound(&self) -> Option<&BTreeMap<String, Tag>> {
        match self {
            Tag::Compound(map) => Some(map),
            _ => None,
        }
    }
    pub fn list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(items) => Some(items),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }
    /// Any integer tag as i64.
    pub fn int(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(v.into()),
            Tag::Short(v) => Some(v.into()),
            Tag::Int(v) => Some(v.into()),
            Tag::Long(v) => Some(v),
            _ => None,
        }
    }
    pub fn longs(&self) -> Option<&[i64]> {
        match self {
            Tag::LongArray(v) => Some(v),
            _ => None,
        }
    }
    /// Takes `key` out of a compound (to keep a large array without copying it).
    pub fn remove(&mut self, key: &str) -> Option<Tag> {
        match self {
            Tag::Compound(map) => map.remove(key),
            _ => None,
        }
    }
}

/// Limits that keep a hostile file from exhausting memory or the stack. Arrays are only as
/// long as NBT allows (an i32 length); an array is allocated once its bytes are known to be there.
const MAX_DEPTH: usize = 512;

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    /// Long arrays are skipped (read as empty): enough to look at a file's metadata and sizes.
    skip_longs: bool,
}

type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or("NBT: length overflow")?;
        if end > self.data.len() {
            return Err("NBT: unexpected end of data".into());
        }
        let s = &self.data[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> R<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u16(&mut self) -> R<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> R<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> R<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn len(&mut self) -> R<usize> {
        let n = self.i32()?;
        if n < 0 {
            return Ok(0);
        }
        Ok(n as usize)
    }
    fn string(&mut self) -> R<String> {
        let n = self.u16()? as usize;
        let bytes = self.take(n)?;
        // Modified UTF-8: fine as lossy UTF-8 for block and region names.
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
    fn payload(&mut self, kind: u8, depth: usize) -> R<Tag> {
        if depth > MAX_DEPTH {
            return Err("NBT: nested too deeply".into());
        }
        Ok(match kind {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|&b| b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let inner = self.u8()?;
                let n = self.len()?;
                let mut items = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    items.push(self.payload(inner, depth + 1)?);
                }
                Tag::List(items)
            }
            10 => {
                let mut map = BTreeMap::new();
                loop {
                    let kind = self.u8()?;
                    if kind == 0 {
                        break;
                    }
                    let name = self.string()?;
                    map.insert(name, self.payload(kind, depth + 1)?);
                }
                Tag::Compound(map)
            }
            11 => {
                let n = self.len()?;
                let bytes = self.take(n * 4)?;
                Tag::IntArray(bytes.chunks_exact(4).map(|c| i32::from_be_bytes(c.try_into().unwrap())).collect())
            }
            12 => {
                let n = self.len()?;
                let bytes = self.take(n * 8)?;
                if self.skip_longs {
                    Tag::LongArray(Vec::new())
                } else {
                    Tag::LongArray(bytes.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect())
                }
            }
            k => return Err(format!("NBT: unknown tag type {k}")),
        })
    }
}

/// Reads an uncompressed named root tag; returns its name and value.
pub fn read(data: &[u8]) -> R<(String, Tag)> {
    read_with(data, false)
}

fn read_with(data: &[u8], skip_longs: bool) -> R<(String, Tag)> {
    let mut r = Reader { data, at: 0, skip_longs };
    let kind = r.u8()?;
    if kind == 0 {
        return Err("NBT: empty root".into());
    }
    let name = r.string()?;
    let tag = r.payload(kind, 0)?;
    Ok((name, tag))
}

/// Reads a gzip-compressed (or, failing that, uncompressed) NBT file.
pub fn read_file_bytes(bytes: &[u8], max_unpacked: usize) -> R<Tag> {
    read_file_with(bytes, max_unpacked, false)
}

/// Like `read_file_bytes`, but every long array comes back empty: a large `.litematic`'s
/// metadata and region sizes without holding its blocks.
pub fn read_file_bytes_light(bytes: &[u8], max_unpacked: usize) -> R<Tag> {
    read_file_with(bytes, max_unpacked, true)
}

fn read_file_with(bytes: &[u8], max_unpacked: usize, skip_longs: bool) -> R<Tag> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        // gzip ends with the unpacked size (mod 2^32): room for it at once, not by doubling.
        let hint = bytes.len().checked_sub(4).map_or(0, |at| u32::from_le_bytes(bytes[at..].try_into().unwrap()) as usize);
        let mut out = Vec::with_capacity(hint.min(max_unpacked));
        flate2::read::GzDecoder::new(bytes)
            .take(max_unpacked as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| format!("gzip: {e}"))?;
        if out.len() > max_unpacked {
            return Err("file too large once unpacked".into());
        }
        Ok(read_with(&out, skip_longs)?.1)
    } else {
        Ok(read_with(bytes, skip_longs)?.1)
    }
}

// ---- writer (used by tests to build files, and handy for exporting) ----

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn kind(tag: &Tag) -> u8 {
    match tag {
        Tag::Byte(_) => 1,
        Tag::Short(_) => 2,
        Tag::Int(_) => 3,
        Tag::Long(_) => 4,
        Tag::Float(_) => 5,
        Tag::Double(_) => 6,
        Tag::ByteArray(_) => 7,
        Tag::String(_) => 8,
        Tag::List(_) => 9,
        Tag::Compound(_) => 10,
        Tag::IntArray(_) => 11,
        Tag::LongArray(_) => 12,
    }
}

fn put_payload(out: &mut Vec<u8>, tag: &Tag) {
    match tag {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::ByteArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            out.extend(v.iter().map(|&b| b as u8));
        }
        Tag::String(s) => put_str(out, s),
        Tag::List(items) => {
            out.push(items.first().map_or(0, kind));
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for t in items {
                put_payload(out, t);
            }
        }
        Tag::Compound(map) => {
            for (k, v) in map {
                out.push(kind(v));
                put_str(out, k);
                put_payload(out, v);
            }
            out.push(0);
        }
        Tag::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
}

/// Writes a named root tag (uncompressed).
pub fn write(name: &str, tag: &Tag) -> Vec<u8> {
    let mut out = vec![kind(tag)];
    put_str(&mut out, name);
    put_payload(&mut out, tag);
    out
}

/// Writes a gzip-compressed named root tag.
pub fn write_gzip(name: &str, tag: &Tag) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&write(name, tag)).unwrap();
    enc.finish().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut map = BTreeMap::new();
        map.insert("a".to_string(), Tag::Int(-5));
        map.insert("s".to_string(), Tag::String("hällo".into()));
        map.insert("l".to_string(), Tag::LongArray(vec![1, -1, i64::MAX]));
        map.insert("list".to_string(), Tag::List(vec![Tag::Short(1), Tag::Short(2)]));
        let tag = Tag::Compound(map);
        let bytes = write_gzip("root", &tag);
        assert_eq!(read_file_bytes(&bytes, 1 << 20).unwrap(), tag);
    }

    #[test]
    fn truncated_is_an_error() {
        let bytes = write("", &Tag::Compound(BTreeMap::from([("x".into(), Tag::Long(3))])));
        assert!(read(&bytes[..bytes.len() - 3]).is_err());
    }
}
