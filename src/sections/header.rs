use std::io::Read;

use flate2::read::ZlibDecoder;
use log::error;
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::reader::Reader;

const MAGIC: u32 = 0x44BE_C00C;
const STORED_COMPRESSED: u8 = 1 << 0;
const STORED_ENCRYPTED: u8 = 1 << 1;

/// First feature level whose header carries the encryption fields.
const CHUNK_ENCRYPTION_SUPPORT: u32 = 22;

/// Fixed-size prefix of a manifest file. The body that follows is described
/// by `size_compressed`, `size_uncompressed` and `sha1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub header_size: u32,
    pub size_uncompressed: u32,
    pub size_compressed: u32,
    /// SHA-1 of the body after decompression.
    pub sha1: [u8; 20],
    pub stored_as: u8,
    /// Feature level the manifest was written with.
    pub version: u32,
    pub encryption: Option<HeaderEncryption>,
}

/// Fields that only exist in headers of version 22 and later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderEncryption {
    pub secret_guid: [u8; 16],
    pub auth_tag: [u8; 16],
}

impl Header {
    pub fn read(reader: &mut Reader) -> Result<Header> {
        let magic = reader.read_u32()?;
        if magic != MAGIC {
            error!("magic number 0x{:08X} verified", magic);
            return Err(Error::BadMagic { found: magic });
        }

        let header_size = reader.read_u32()?;
        let size_uncompressed = reader.read_u32()?;
        let size_compressed = reader.read_u32()?;
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(reader.read_bytes(20)?);
        let stored_as = reader.read_u8()?;
        let version = reader.read_u32()?;

        let encryption = if version >= CHUNK_ENCRYPTION_SUPPORT {
            Some(read_header_encryption(reader)?)
        } else {
            None
        };

        // Newer versions may append fields we do not know; header_size says
        // where the body really starts.
        let read_so_far = reader.position();
        if (header_size as usize) < read_so_far {
            return Err(Error::BadHeaderSize {
                declared: header_size,
                read: read_so_far,
            });
        }
        reader.seek(header_size as usize)?;

        Ok(Header {
            header_size,
            size_uncompressed,
            size_compressed,
            sha1,
            stored_as,
            version,
            encryption,
        })
    }

    pub fn is_compressed(&self) -> bool {
        self.stored_as & STORED_COMPRESSED != 0
    }

    pub fn is_encrypted(&self) -> bool {
        self.stored_as & STORED_ENCRYPTED != 0
    }

    /// Turns the raw bytes after the header into the plain manifest body,
    /// decompressing it if needed and checking it against the header SHA-1.
    pub fn decode_body(&self, raw: &[u8]) -> Result<Vec<u8>> {
        if self.is_encrypted() {
            return Err(Error::Encrypted);
        }
        let body = if self.is_compressed() {
            self.decompress(raw)?
        } else {
            raw.to_vec()
        };
        if Sha1::digest(&body).as_slice() != self.sha1 {
            return Err(Error::HashMismatch);
        }
        Ok(body)
    }

    fn decompress(&self, raw: &[u8]) -> Result<Vec<u8>> {
        // The declared size caps how much a corrupt stream can make us allocate.
        let limit = u64::from(self.size_uncompressed);
        let mut body = Vec::new();
        ZlibDecoder::new(raw)
            .take(limit)
            .read_to_end(&mut body)
            .map_err(Error::Decompress)?;
        Ok(body)
    }
}

fn read_header_encryption(reader: &mut Reader) -> Result<HeaderEncryption> {
    let mut secret_guid = [0u8; 16];
    secret_guid.copy_from_slice(reader.read_bytes(16)?);
    let mut auth_tag = [0u8; 16];
    auth_tag.copy_from_slice(reader.read_bytes(16)?);
    Ok(HeaderEncryption {
        secret_guid,
        auth_tag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    fn sha1_of(body: &[u8]) -> [u8; 20] {
        let mut out = [0u8; 20];
        out.copy_from_slice(&Sha1::digest(body));
        out
    }

    /// Builds header bytes with the layout of the given version.
    fn header_bytes(version: u32, stored_as: u8, body: &[u8], sha1: [u8; 20]) -> Vec<u8> {
        let header_size: u32 = if version >= 22 { 73 } else { 41 };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_le_bytes());
        bytes.extend_from_slice(&header_size.to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&sha1);
        bytes.push(stored_as);
        bytes.extend_from_slice(&version.to_le_bytes());
        if version >= 22 {
            bytes.extend_from_slice(&[0xAA; 16]);
            bytes.extend_from_slice(&[0xBB; 16]);
        }
        bytes
    }

    fn header_with(stored_as: u8, size_uncompressed: u32, sha1: [u8; 20]) -> Header {
        Header {
            header_size: 41,
            size_uncompressed,
            size_compressed: 0,
            sha1,
            stored_as,
            version: 18,
            encryption: None,
        }
    }

    fn zlib(body: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(body).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn reads_a_version_18_header() {
        let body = b"body";
        let bytes = header_bytes(18, 1, body, sha1_of(body));
        let mut reader = Reader::new(&bytes);
        let header = Header::read(&mut reader).unwrap();
        assert_eq!(header.header_size, 41);
        assert_eq!(header.size_uncompressed, 4);
        assert_eq!(header.sha1, sha1_of(body));
        assert_eq!(header.version, 18);
        assert!(header.is_compressed());
        assert!(!header.is_encrypted());
        assert_eq!(header.encryption, None);
        assert_eq!(reader.position(), 41);
    }

    #[test]
    fn reads_the_encryption_fields_of_a_version_22_header() {
        let bytes = header_bytes(22, 0, b"", [0; 20]);
        let mut reader = Reader::new(&bytes);
        let header = Header::read(&mut reader).unwrap();
        assert_eq!(header.header_size, 73);
        let encryption = header.encryption.unwrap();
        assert_eq!(encryption.secret_guid, [0xAA; 16]);
        assert_eq!(encryption.auth_tag, [0xBB; 16]);
        assert_eq!(reader.position(), 73);
    }

    #[test]
    fn decodes_plain_and_compressed_bodies() {
        let body = b"manifest body ".repeat(20);
        let size = body.len() as u32;
        let plain = header_with(0, size, sha1_of(&body));
        assert_eq!(plain.decode_body(&body).unwrap(), body);
        let compressed = header_with(1, size, sha1_of(&body));
        assert_eq!(compressed.decode_body(&zlib(&body)).unwrap(), body);
    }

    #[test]
    fn rejects_a_body_that_does_not_match_the_sha1() {
        let plain = header_with(0, 4, sha1_of(b"good"));
        assert!(matches!(
            plain.decode_body(b"evil"),
            Err(Error::HashMismatch)
        ));
        let compressed = header_with(1, 4, sha1_of(b"good"));
        assert!(matches!(
            compressed.decode_body(&zlib(b"evil")),
            Err(Error::HashMismatch)
        ));
    }

    #[test]
    fn rejects_an_encrypted_manifest_before_touching_the_body() {
        let header = header_with(2, 4, [0; 20]);
        assert!(matches!(header.decode_body(b"data"), Err(Error::Encrypted)));
        let header = header_with(3, 4, [0; 20]);
        assert!(matches!(header.decode_body(b"data"), Err(Error::Encrypted)));
    }
}
