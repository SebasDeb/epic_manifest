use crate::binary::Guid;
use crate::binary::Reader;
use crate::error::{Error, Result};

// Versions of the file list section, as in Unreal's EFileManifestListVersion.
const HAS_MD5_AND_MIME_TYPE: u8 = 1;
const HAS_SHA256: u8 = 2;

/// A slice of a chunk that makes up part of a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkPart {
    pub guid: Guid,
    /// Where the part starts inside the decompressed chunk.
    pub offset: u32,
    pub size: u32,
    /// Where the part lands inside the final file. Not stored; it is the sum
    /// of the sizes of the parts before this one.
    pub file_offset: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileManifest {
    pub filename: String,
    pub symlink_target: String,
    pub sha1: [u8; 20],
    /// Bit 0 read-only, bit 1 compressed, bit 2 unix executable.
    pub flags: u8,
    pub install_tags: Vec<String>,
    pub chunk_parts: Vec<ChunkPart>,
    pub md5: Option<[u8; 16]>,
    pub mime_type: String,
    pub sha256: Option<[u8; 32]>,
    /// Not stored; it is the sum of the sizes of the chunk parts.
    pub file_size: u64,
}

/// Reads the file list, which is columnar like the chunk list.
pub fn read_files(reader: &mut Reader) -> Result<Vec<FileManifest>> {
    let section = reader.begin_section()?;
    let count = reader.read_u32()?;

    // Each filename takes at least four bytes, so the vector cannot grow
    // beyond what the data holds even if the count is bogus.
    let mut files = Vec::new();
    for _ in 0..count {
        files.push(FileManifest {
            filename: reader.read_fstring()?,
            ..FileManifest::default()
        });
    }
    for file in &mut files {
        file.symlink_target = reader.read_fstring()?;
    }
    for file in &mut files {
        file.sha1 = reader.read_array()?;
    }
    for file in &mut files {
        file.flags = reader.read_u8()?;
    }
    for file in &mut files {
        file.install_tags = read_install_tags(reader)?;
    }
    for file in &mut files {
        file.chunk_parts = read_chunk_parts(reader)?;
        file.file_size = file
            .chunk_parts
            .iter()
            .map(|part| u64::from(part.size))
            .sum();
    }

    if section.version >= HAS_MD5_AND_MIME_TYPE {
        for file in &mut files {
            file.md5 = read_optional_md5(reader)?;
        }
        for file in &mut files {
            file.mime_type = reader.read_fstring()?;
        }
    }
    if section.version >= HAS_SHA256 {
        for file in &mut files {
            file.sha256 = Some(reader.read_array()?);
        }
    }

    reader.end_section(&section)?;
    Ok(files)
}

fn read_install_tags(reader: &mut Reader) -> Result<Vec<String>> {
    let count = reader.read_u32()?;
    let mut tags = Vec::new();
    for _ in 0..count {
        tags.push(reader.read_fstring()?);
    }
    Ok(tags)
}

fn read_chunk_parts(reader: &mut Reader) -> Result<Vec<ChunkPart>> {
    let count = reader.read_u32()?;
    let mut parts = Vec::new();
    let mut file_offset = 0u64;
    for _ in 0..count {
        let part = read_chunk_part(reader, file_offset)?;
        file_offset += u64::from(part.size);
        parts.push(part);
    }
    Ok(parts)
}

/// Each part starts with its own size (28 today), so a newer writer can add
/// fields to it without breaking readers that skip to the end.
fn read_chunk_part(reader: &mut Reader, file_offset: u64) -> Result<ChunkPart> {
    let start = reader.position();
    let part_size = reader.read_u32()? as usize;
    let part = ChunkPart {
        guid: reader.read_guid()?,
        offset: reader.read_u32()?,
        size: reader.read_u32()?,
        file_offset,
    };

    let end = start.saturating_add(part_size);
    if reader.position() > end {
        return Err(Error::SectionOverrun {
            end,
            position: reader.position(),
        });
    }
    reader.seek(end)?;
    Ok(part)
}

/// The MD5 is preceded by a `u32` that says whether it is present at all.
fn read_optional_md5(reader: &mut Reader) -> Result<Option<[u8; 16]>> {
    if reader.read_u32()? == 0 {
        return Ok(None);
    }
    Ok(Some(reader.read_array()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fstring(text: &str) -> Vec<u8> {
        if text.is_empty() {
            return 0i32.to_le_bytes().to_vec();
        }
        let mut bytes = ((text.len() + 1) as i32).to_le_bytes().to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes
    }

    fn chunk_part(guid: [u32; 4], offset: u32, size: u32, extra: &[u8]) -> Vec<u8> {
        let mut bytes = (28 + extra.len() as u32).to_le_bytes().to_vec();
        for word in guid {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(extra);
        bytes
    }

    /// Two files: "a.txt" with two chunk parts and two tags, and "b/c.bin"
    /// with no parts and no tags.
    fn file_list_bytes(version: u8, first_part_extra: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&2u32.to_le_bytes());
        body.extend(fstring("a.txt"));
        body.extend(fstring("b/c.bin"));
        body.extend(fstring(""));
        body.extend(fstring("link-target"));
        body.extend_from_slice(&[0xC1; 20]);
        body.extend_from_slice(&[0xC2; 20]);
        body.extend_from_slice(&[0x04, 0x01]);
        body.extend_from_slice(&2u32.to_le_bytes());
        body.extend(fstring("tag-one"));
        body.extend(fstring("tag-two"));
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&2u32.to_le_bytes());
        body.extend(chunk_part([1, 1, 1, 1], 100, 50, first_part_extra));
        body.extend(chunk_part([2, 2, 2, 2], 0, 70, &[]));
        body.extend_from_slice(&0u32.to_le_bytes());
        if version >= 1 {
            body.extend_from_slice(&1u32.to_le_bytes());
            body.extend_from_slice(&[0xD1; 16]);
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend(fstring("text/plain"));
            body.extend(fstring(""));
        }
        if version >= 2 {
            body.extend_from_slice(&[0xE1; 32]);
            body.extend_from_slice(&[0xE2; 32]);
        }

        let mut bytes = ((body.len() + 5) as u32).to_le_bytes().to_vec();
        bytes.push(version);
        bytes.extend(body);
        bytes
    }

    #[test]
    fn reads_a_version_0_file_list() {
        let bytes = file_list_bytes(0, &[]);
        let mut reader = Reader::new(&bytes);
        let files = read_files(&mut reader).unwrap();
        assert_eq!(files.len(), 2);

        let first = &files[0];
        assert_eq!(first.filename, "a.txt");
        assert_eq!(first.symlink_target, "");
        assert_eq!(first.sha1, [0xC1; 20]);
        assert_eq!(first.flags, 0x04);
        assert_eq!(first.install_tags, ["tag-one", "tag-two"]);
        assert_eq!(first.chunk_parts.len(), 2);
        assert_eq!(first.chunk_parts[0].guid, Guid([1, 1, 1, 1]));
        assert_eq!(first.chunk_parts[0].offset, 100);
        assert_eq!(first.chunk_parts[0].size, 50);
        assert_eq!(first.chunk_parts[0].file_offset, 0);
        assert_eq!(first.chunk_parts[1].file_offset, 50);
        assert_eq!(first.file_size, 120);
        assert_eq!(first.md5, None);
        assert_eq!(first.sha256, None);

        let second = &files[1];
        assert_eq!(second.filename, "b/c.bin");
        assert_eq!(second.symlink_target, "link-target");
        assert!(second.install_tags.is_empty());
        assert!(second.chunk_parts.is_empty());
        assert_eq!(second.file_size, 0);
        assert_eq!(reader.position(), bytes.len());
    }

    #[test]
    fn newer_versions_add_md5_mime_type_and_sha256() {
        let bytes = file_list_bytes(2, &[]);
        let files = read_files(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(files[0].md5, Some([0xD1; 16]));
        assert_eq!(files[0].mime_type, "text/plain");
        assert_eq!(files[1].md5, None);
        assert_eq!(files[0].sha256, Some([0xE1; 32]));
        assert_eq!(files[1].sha256, Some([0xE2; 32]));
    }

    #[test]
    fn any_nonzero_md5_flag_means_the_hash_is_present() {
        let mut bytes = file_list_bytes(1, &[]);
        let flag_offset = bytes.len() - (4 + 11 + 4) - (4 + 16 + 4) - 4 + 4;
        bytes[flag_offset..flag_offset + 4].copy_from_slice(&7u32.to_le_bytes());
        let files = read_files(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(files[0].md5, Some([0xD1; 16]));
    }

    #[test]
    fn skips_extra_bytes_inside_a_chunk_part() {
        let bytes = file_list_bytes(0, &[9, 9, 9, 9]);
        let mut reader = Reader::new(&bytes);
        let files = read_files(&mut reader).unwrap();
        assert_eq!(files[0].chunk_parts[0].size, 50);
        assert_eq!(files[0].chunk_parts[1].guid, Guid([2, 2, 2, 2]));
        assert_eq!(files[0].chunk_parts[1].file_offset, 50);
        assert_eq!(reader.position(), bytes.len());
    }

    #[test]
    fn rejects_a_count_larger_than_the_data() {
        let mut bytes = file_list_bytes(0, &[]);
        bytes[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        let result = read_files(&mut Reader::new(&bytes));
        assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
    }
}
