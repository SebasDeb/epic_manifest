//! Parser for Epic Games binary `.manifest` files (BuildPatchServices).

mod chunks;
mod custom_fields;
mod error;
mod files;
mod guid;
mod header;
mod meta;
mod reader;

use std::collections::HashMap;
use std::path::Path;

pub use chunks::ChunkInfo;
pub use error::Error;
pub use files::{ChunkPart, FileManifest};
pub use guid::Guid;
pub use header::{Header, HeaderEncryption};
pub use meta::Meta;

use error::Result;
use log::{debug, info};
use reader::Reader;

/// A fully parsed manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub header: Header,
    pub meta: Meta,
    pub chunks: Vec<ChunkInfo>,
    pub files: Vec<FileManifest>,
    pub custom_fields: HashMap<String, String>,
}

impl Manifest {
    /// Parses a manifest from the raw bytes of a `.manifest` file.
    pub fn from_bytes(data: &[u8]) -> Result<Manifest> {
        debug!("parsing manifest, {} bytes", data.len());
        let mut reader = Reader::new(data);
        let header = Header::read(&mut reader)?;
        debug!(
            "header: version {}, compressed {}, encrypted {}",
            header.version,
            header.is_compressed(),
            header.is_encrypted()
        );
        let body = header.decode_body(reader.read_rest())?;
        debug!("body decoded and SHA-1 verified, {} bytes", body.len());

        let mut reader = Reader::new(&body);
        let meta = Meta::read(&mut reader)?;
        let chunks = chunks::read_chunks(&mut reader)?;
        debug!("read {} chunks", chunks.len());

        let files = files::read_files(&mut reader)?;
        debug!("read {} files", files.len());
        
        let custom_fields = custom_fields::read_custom_fields(&mut reader)?;
        info!(
            "parsed {} {}: {} files, {} chunks",
            meta.app_name,
            meta.build_version,
            files.len(),
            chunks.len()
        );

        Ok(Manifest {
            header,
            meta,
            chunks,
            files,
            custom_fields,
        })
    }

    /// Reads a `.manifest` file from disk and parses it.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Manifest> {
        let data = std::fs::read(path)?;
        Manifest::from_bytes(&data)
    }

    /// Size of the installed build: the sum of every file's size.
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|file| file.file_size).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use sha1::{Digest, Sha1};
    use std::io::Write;

    fn fstring(text: &str) -> Vec<u8> {
        if text.is_empty() {
            return 0i32.to_le_bytes().to_vec();
        }
        let mut bytes = ((text.len() + 1) as i32).to_le_bytes().to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes
    }

    fn section(version: u8, content: Vec<u8>) -> Vec<u8> {
        let mut bytes = ((content.len() + 5) as u32).to_le_bytes().to_vec();
        bytes.push(version);
        bytes.extend(content);
        bytes
    }

    fn u32s(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    /// A body with one chunk, two files (one with a single chunk part) and
    /// one custom field.
    fn sample_body() -> Vec<u8> {
        let mut meta = u32s(&[21]);
        meta.push(0);
        meta.extend(u32s(&[42]));
        for text in ["MyGame", "1.0.0", "Game.exe", "-fast"] {
            meta.extend(fstring(text));
        }
        meta.extend(u32s(&[0]));
        for text in ["", "", ""] {
            meta.extend(fstring(text));
        }

        let mut chunks = u32s(&[1, 1, 2, 3, 4]);
        chunks.extend_from_slice(&0xABCDu64.to_le_bytes());
        chunks.extend_from_slice(&[0x11; 20]);
        chunks.push(7);
        chunks.extend(u32s(&[1000]));
        chunks.extend_from_slice(&400i64.to_le_bytes());

        let mut files = u32s(&[2]);
        files.extend(fstring("data/main.pak"));
        files.extend(fstring("empty.txt"));
        files.extend(fstring(""));
        files.extend(fstring(""));
        files.extend_from_slice(&[0x22; 20]);
        files.extend_from_slice(&[0x33; 20]);
        files.extend_from_slice(&[0, 0]);
        files.extend(u32s(&[0, 0]));
        files.extend(u32s(&[1, 28, 1, 2, 3, 4, 16, 600]));
        files.extend(u32s(&[0]));

        let mut custom = u32s(&[1]);
        custom.extend(fstring("BaseUrl"));
        custom.extend(fstring("https://cdn.example"));

        let mut body = section(0, meta);
        body.extend(section(0, chunks));
        body.extend(section(0, files));
        body.extend(section(0, custom));
        body
    }

    fn manifest_bytes(body: &[u8], compress: bool) -> Vec<u8> {
        let stored = if compress {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(body).unwrap();
            encoder.finish().unwrap()
        } else {
            body.to_vec()
        };

        let mut bytes = u32s(&[0x44BE_C00C, 41, body.len() as u32, stored.len() as u32]);
        bytes.extend_from_slice(&Sha1::digest(body));
        bytes.push(u8::from(compress));
        bytes.extend(u32s(&[21]));
        bytes.extend(stored);
        bytes
    }

    fn check_sample(manifest: &Manifest) {
        assert_eq!(manifest.header.version, 21);
        assert_eq!(manifest.meta.app_id, 42);
        assert_eq!(manifest.meta.app_name, "MyGame");
        assert_eq!(manifest.meta.build_version, "1.0.0");
        assert_eq!(manifest.meta.launch_exe, "Game.exe");
        assert_eq!(manifest.meta.launch_command, "-fast");

        assert_eq!(manifest.chunks.len(), 1);
        assert_eq!(manifest.chunks[0].guid, Guid([1, 2, 3, 4]));
        assert_eq!(manifest.chunks[0].hash, 0xABCD);
        assert_eq!(manifest.chunks[0].group_num, 7);
        assert_eq!(manifest.chunks[0].window_size, 1000);
        assert_eq!(manifest.chunks[0].file_size, 400);

        assert_eq!(manifest.files.len(), 2);
        assert_eq!(manifest.files[0].filename, "data/main.pak");
        assert_eq!(manifest.files[0].file_size, 600);
        assert_eq!(manifest.files[0].chunk_parts[0].guid, Guid([1, 2, 3, 4]));
        assert_eq!(manifest.files[1].filename, "empty.txt");
        assert_eq!(manifest.files[1].file_size, 0);
        assert_eq!(manifest.total_size(), 600);

        assert_eq!(manifest.custom_fields["BaseUrl"], "https://cdn.example");
    }

    #[test]
    fn parses_an_uncompressed_manifest() {
        let bytes = manifest_bytes(&sample_body(), false);
        check_sample(&Manifest::from_bytes(&bytes).unwrap());
    }

    #[test]
    fn parses_a_compressed_manifest() {
        let bytes = manifest_bytes(&sample_body(), true);
        check_sample(&Manifest::from_bytes(&bytes).unwrap());
    }

    #[test]
    fn rejects_a_corrupted_body() {
        let mut bytes = manifest_bytes(&sample_body(), false);
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(matches!(
            Manifest::from_bytes(&bytes),
            Err(Error::HashMismatch)
        ));
    }

    #[test]
    fn rejects_data_that_is_not_a_manifest() {
        let result = Manifest::from_bytes(b"this is definitely not a manifest file");
        assert!(matches!(result, Err(Error::BadMagic { .. })));
    }

    #[test]
    fn from_file_reads_a_manifest_from_disk() {
        let path = std::env::temp_dir().join("epic_manifest_from_file_test.manifest");
        std::fs::write(&path, manifest_bytes(&sample_body(), true)).unwrap();
        let manifest = Manifest::from_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        check_sample(&manifest);
    }
}
