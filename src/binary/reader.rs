use crate::binary::Guid;
use crate::error::{Error, Result};

/// Where a manifest section ends and which version it was written with.
pub struct Section {
    pub version: u8,
    end: usize,
}

/// A forward-only cursor over a byte slice. Every read is bounds-checked.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    /// Moves the cursor to an absolute offset. Sections use this to jump to
    /// their end even when they hold fields this parser does not know about.
    pub fn seek(&mut self, offset: usize) -> Result<()> {
        if offset > self.data.len() {
            return Err(Error::SeekOutOfBounds {
                offset,
                len: self.data.len(),
            });
        }
        self.pos = offset;
        Ok(())
    }

    /// Reads the `u32` size and `u8` version that open every section. The
    /// size counts from the start of the size field itself.
    pub fn begin_section(&mut self) -> Result<Section> {
        let start = self.pos;
        let size = self.read_u32()?;
        let version = self.read_u8()?;
        let end = start
            .checked_add(size as usize)
            .ok_or(Error::SectionOverrun {
                end: usize::MAX,
                position: self.pos,
            })?;
        Ok(Section { version, end })
    }

    /// Jumps to the end of the section, skipping fields this parser does not
    /// know. Reading past the declared end means the data is inconsistent.
    pub fn end_section(&mut self, section: &Section) -> Result<()> {
        if self.pos > section.end {
            return Err(Error::SectionOverrun {
                end: section.end,
                position: self.pos,
            });
        }
        self.seek(section.end)
    }

    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let available = self.data.len() - self.pos;
        if len > available {
            return Err(Error::UnexpectedEof {
                offset: self.pos,
                needed: len - available,
            });
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    /// Takes everything from the cursor to the end of the data.
    pub fn read_rest(&mut self) -> &'a [u8] {
        let rest = &self.data[self.pos..];
        self.pos = self.data.len();
        rest
    }

    // The only generic in the crate: it keeps the integer readers to one line each.
    pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut array = [0u8; N];
        array.copy_from_slice(self.read_bytes(N)?);
        Ok(array)
    }

    pub fn read_guid(&mut self) -> Result<Guid> {
        Ok(Guid([
            self.read_u32()?,
            self.read_u32()?,
            self.read_u32()?,
            self.read_u32()?,
        ]))
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_array::<1>()?[0])
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    pub fn read_i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    pub fn read_i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.read_array()?))
    }

    /// Reads an Unreal FString: an `i32` length followed by the characters.
    /// A positive length counts UTF-8 bytes, a negative one counts UTF-16
    /// code units. Both include a null terminator, except for the empty
    /// string, which is stored as a length of zero with nothing after it.
    pub fn read_fstring(&mut self) -> Result<String> {
        let length = self.read_i32()?;
        if length == 0 {
            return Ok(String::new());
        }
        if length > 0 {
            self.read_utf8_string(length as usize)
        } else {
            self.read_utf16_string(length.unsigned_abs() as usize)
        }
    }

    fn read_utf8_string(&mut self, len: usize) -> Result<String> {
        let (terminator, text) = self
            .read_bytes(len)?
            .split_last()
            .ok_or(Error::InvalidString)?;
        if *terminator != 0 {
            return Err(Error::InvalidString);
        }
        String::from_utf8(text.to_vec()).map_err(|_| Error::InvalidString)
    }

    fn read_utf16_string(&mut self, units: usize) -> Result<String> {
        let byte_len = units.checked_mul(2).ok_or(Error::InvalidString)?;
        let bytes = self.read_bytes(byte_len)?;
        let (pairs, _) = bytes.as_chunks::<2>();
        let mut code_units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        if code_units.pop() != Some(0) {
            return Err(Error::InvalidString);
        }
        String::from_utf16(&code_units).map_err(|_| Error::InvalidString)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fstring_ascii(text: &str) -> Vec<u8> {
        let mut bytes = ((text.len() + 1) as i32).to_le_bytes().to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes
    }

    fn fstring_utf16(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = (-((units.len() + 1) as i32)).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    #[test]
    fn reads_little_endian_integers() {
        let data = [
            0x01, // u8
            0x78, 0x56, 0x34, 0x12, // u32
            0xFF, 0xFF, 0xFF, 0xFF, // i32
            0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, // u64
            0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // i64
        ];
        let mut reader = Reader::new(&data);
        assert_eq!(reader.read_u8().unwrap(), 1);
        assert_eq!(reader.read_u32().unwrap(), 0x1234_5678);
        assert_eq!(reader.read_i32().unwrap(), -1);
        assert_eq!(reader.read_u64().unwrap(), 0x0102_0304_0506_0708);
        assert_eq!(reader.read_i64().unwrap(), -2);
        assert_eq!(reader.position(), data.len());
    }

    #[test]
    fn integer_read_past_the_end_reports_missing_bytes() {
        let mut reader = Reader::new(&[1, 2]);
        match reader.read_u32() {
            Err(Error::UnexpectedEof { offset, needed }) => {
                assert_eq!((offset, needed), (0, 2));
            }
            other => panic!("expected UnexpectedEof, got {other:?}"),
        }
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn reads_ascii_utf16_and_empty_fstrings() {
        let mut data = fstring_ascii("FortniteGame");
        data.extend(fstring_utf16("caf\u{e9} \u{65e5}\u{672c}"));
        data.extend(0i32.to_le_bytes());
        let mut reader = Reader::new(&data);
        assert_eq!(reader.read_fstring().unwrap(), "FortniteGame");
        assert_eq!(reader.read_fstring().unwrap(), "caf\u{e9} \u{65e5}\u{672c}");
        assert_eq!(reader.read_fstring().unwrap(), "");
        assert_eq!(reader.position(), data.len());
    }

    #[test]
    fn bad_fstrings_are_rejected() {
        let mut cut = fstring_ascii("truncated");
        cut.truncate(8);
        let eof_cases = [cut, i32::MIN.to_le_bytes().to_vec()];
        for data in &eof_cases {
            let result = Reader::new(data).read_fstring();
            assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
        }

        // No null terminator, and invalid UTF-8.
        let invalid_cases: [&[u8]; 2] = [
            &[3, 0, 0, 0, b'a', b'b', b'c'],
            &[3, 0, 0, 0, 0xFF, 0xFE, 0],
        ];
        for data in invalid_cases {
            let result = Reader::new(data).read_fstring();
            assert!(matches!(result, Err(Error::InvalidString)));
        }
    }

    #[test]
    fn end_section_skips_unread_fields() {
        let data = [10, 0, 0, 0, 3, 1, 2, 3, 4, 5, 0x42];
        let mut reader = Reader::new(&data);
        let section = reader.begin_section().unwrap();
        assert_eq!(section.version, 3);
        assert_eq!(reader.read_u8().unwrap(), 1);
        reader.end_section(&section).unwrap();
        assert_eq!(reader.read_u8().unwrap(), 0x42);
    }
}
