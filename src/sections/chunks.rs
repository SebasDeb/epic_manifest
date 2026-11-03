use crate::binary::Guid;
use crate::binary::Reader;
use crate::error::Result;

// Versions of the chunk list section, as in Unreal's EChunkDataListVersion.
const SERIALISES_ENCRYPTION_SECRET_ID: u8 = 1;
const SERIALISES_COMPRESSED_DATA_SIZE: u8 = 2;
const SERIALISES_AES_AUTH_TAG: u8 = 3;

/// One downloadable chunk. The last three fields only exist in newer
/// sections and stay zeroed when the section is too old to carry them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkInfo {
    pub guid: Guid,
    pub hash: u64,
    pub sha1: [u8; 20],
    pub group_num: u8,
    /// Size of the chunk once decompressed.
    pub window_size: u32,
    /// Size of the chunk as stored on the CDN.
    pub file_size: i64,
    pub secret_guid: Guid,
    pub window_size_compressed: u32,
    pub auth_tag: [u8; 16],
}

impl ChunkInfo {
    /// Path of this chunk on the CDN, relative to the base URL. The directory
    /// depends on the manifest's feature level.
    pub fn relative_path(&self, feature_level: u32) -> String {
        format!(
            "{}/{:02}/{:016X}_{}.chunk",
            chunk_dir(feature_level),
            self.group_num,
            self.hash,
            self.guid
        )
    }
}

// Feature levels that moved chunks to a new directory.
const DATA_FILE_RENAMES: u32 = 3;
const CHUNK_COMPRESSION_SUPPORT: u32 = 6;
const VARIABLE_SIZE_CHUNKS_WITHOUT_WINDOW_SIZE: u32 = 15;
const CHUNK_ENCRYPTION_SUPPORT: u32 = 22;

fn chunk_dir(feature_level: u32) -> &'static str {
    match feature_level {
        level if level < DATA_FILE_RENAMES => "Chunks",
        level if level < CHUNK_COMPRESSION_SUPPORT => "ChunksV2",
        level if level < VARIABLE_SIZE_CHUNKS_WITHOUT_WINDOW_SIZE => "ChunksV3",
        level if level < CHUNK_ENCRYPTION_SUPPORT => "ChunksV4",
        _ => "ChunksV5",
    }
}

/// Reads the chunk list. The data is columnar: every GUID first, then every
/// hash, and so on, so each loop below reads one field for all chunks.
pub fn read_chunks(reader: &mut Reader) -> Result<Vec<ChunkInfo>> {
    let section = reader.begin_section()?;
    let count = reader.read_u32()?;

    // The first column creates the elements. Growing the vector as GUIDs are
    // read keeps a bogus count from allocating more than the data can hold.
    let mut chunks = Vec::new();
    for _ in 0..count {
        chunks.push(ChunkInfo {
            guid: reader.read_guid()?,
            ..ChunkInfo::default()
        });
    }
    for chunk in &mut chunks {
        chunk.hash = reader.read_u64()?;
    }
    for chunk in &mut chunks {
        chunk.sha1 = reader.read_array()?;
    }
    for chunk in &mut chunks {
        chunk.group_num = reader.read_u8()?;
    }
    for chunk in &mut chunks {
        chunk.window_size = reader.read_u32()?;
    }
    for chunk in &mut chunks {
        chunk.file_size = reader.read_i64()?;
    }

    if section.version >= SERIALISES_ENCRYPTION_SECRET_ID {
        for chunk in &mut chunks {
            chunk.secret_guid = reader.read_guid()?;
        }
    }
    if section.version >= SERIALISES_COMPRESSED_DATA_SIZE {
        for chunk in &mut chunks {
            chunk.window_size_compressed = reader.read_u32()?;
        }
    }
    if section.version >= SERIALISES_AES_AUTH_TAG {
        for chunk in &mut chunks {
            chunk.auth_tag = reader.read_array()?;
        }
    }

    reader.end_section(&section)?;
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    /// Two chunks laid out column by column, like the real format.
    fn chunk_list_bytes(version: u8, extra: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&2u32.to_le_bytes());
        for word in [1u32, 2, 3, 4, 5, 6, 7, 8] {
            body.extend_from_slice(&word.to_le_bytes());
        }
        for hash in [0x1111u64, 0x2222] {
            body.extend_from_slice(&hash.to_le_bytes());
        }
        body.extend_from_slice(&[0xA1; 20]);
        body.extend_from_slice(&[0xA2; 20]);
        body.extend_from_slice(&[10, 20]);
        for size in [1_000_000u32, 2_000_000] {
            body.extend_from_slice(&size.to_le_bytes());
        }
        for size in [500i64, 900] {
            body.extend_from_slice(&size.to_le_bytes());
        }
        if version >= 1 {
            for word in [9u32, 9, 9, 9, 8, 8, 8, 8] {
                body.extend_from_slice(&word.to_le_bytes());
            }
        }
        if version >= 2 {
            for size in [600u32, 700] {
                body.extend_from_slice(&size.to_le_bytes());
            }
        }
        if version >= 3 {
            body.extend_from_slice(&[0xB1; 16]);
            body.extend_from_slice(&[0xB2; 16]);
        }
        body.extend_from_slice(extra);

        let mut bytes = ((body.len() + 5) as u32).to_le_bytes().to_vec();
        bytes.push(version);
        bytes.extend(body);
        bytes
    }

    #[test]
    fn reads_a_version_0_chunk_list() {
        let bytes = chunk_list_bytes(0, &[]);
        let mut reader = Reader::new(&bytes);
        let chunks = read_chunks(&mut reader).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].guid, Guid([1, 2, 3, 4]));
        assert_eq!(chunks[1].guid, Guid([5, 6, 7, 8]));
        assert_eq!((chunks[0].hash, chunks[1].hash), (0x1111, 0x2222));
        assert_eq!(chunks[0].sha1, [0xA1; 20]);
        assert_eq!(chunks[1].sha1, [0xA2; 20]);
        assert_eq!((chunks[0].group_num, chunks[1].group_num), (10, 20));
        assert_eq!(chunks[0].window_size, 1_000_000);
        assert_eq!(chunks[1].window_size, 2_000_000);
        assert_eq!((chunks[0].file_size, chunks[1].file_size), (500, 900));
        assert_eq!(chunks[0].secret_guid, Guid::default());
        assert_eq!(chunks[0].window_size_compressed, 0);
        assert_eq!(chunks[0].auth_tag, [0; 16]);
        assert_eq!(reader.position(), bytes.len());
    }

    #[test]
    fn each_section_version_adds_its_own_columns() {
        let bytes = chunk_list_bytes(1, &[]);
        let chunks = read_chunks(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(chunks[0].secret_guid, Guid([9, 9, 9, 9]));
        assert_eq!(chunks[1].secret_guid, Guid([8, 8, 8, 8]));
        assert_eq!(chunks[0].window_size_compressed, 0);

        let bytes = chunk_list_bytes(2, &[]);
        let chunks = read_chunks(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(chunks[0].window_size_compressed, 600);
        assert_eq!(chunks[1].window_size_compressed, 700);
        assert_eq!(chunks[0].auth_tag, [0; 16]);

        let bytes = chunk_list_bytes(3, &[]);
        let chunks = read_chunks(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(chunks[0].auth_tag, [0xB1; 16]);
        assert_eq!(chunks[1].auth_tag, [0xB2; 16]);
    }

    #[test]
    fn rejects_a_count_larger_than_the_data() {
        let mut bytes = chunk_list_bytes(0, &[]);
        bytes[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        let result = read_chunks(&mut Reader::new(&bytes));
        assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
    }

    fn sample_chunk() -> ChunkInfo {
        ChunkInfo {
            guid: Guid([0xC74E_A3B4, 0x4F99_43B4, 0x75BB_72A4, 0x1F8E_68DD]),
            hash: 0x2A39_A534_6809_495C,
            group_num: 5,
            ..ChunkInfo::default()
        }
    }

    #[test]
    fn builds_the_chunk_path_with_group_hash_and_guid() {
        let path = sample_chunk().relative_path(15);
        assert_eq!(
            path,
            "ChunksV4/05/2A39A5346809495C_C74EA3B44F9943B475BB72A41F8E68DD.chunk"
        );
    }

    #[test]
    fn picks_the_chunk_directory_from_the_feature_level() {
        let dir = |level| sample_chunk().relative_path(level);
        assert!(dir(2).starts_with("Chunks/"));
        assert!(dir(3).starts_with("ChunksV2/"));
        assert!(dir(6).starts_with("ChunksV3/"));
        assert!(dir(15).starts_with("ChunksV4/"));
        assert!(dir(22).starts_with("ChunksV5/"));
    }
}
