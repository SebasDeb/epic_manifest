use thiserror::Error;

/// Everything that can go wrong while reading a manifest.
#[derive(Debug, Error)]
pub enum Error {
    #[error("unexpected end of data at offset {offset}: needed {needed} more bytes")]
    UnexpectedEof { offset: usize, needed: usize },

    #[error("invalid string in manifest data")]
    InvalidString,

    #[error("bad manifest magic: found {found:#010x}")]
    BadMagic { found: u32 },

    #[error("header size {declared} is smaller than the {read} bytes already read")]
    BadHeaderSize { declared: u32, read: usize },

    #[error("manifest body does not match the SHA-1 in the header")]
    HashMismatch,

    #[error("manifest is encrypted, which is not supported")]
    Encrypted,

    #[error("could not decompress manifest body")]
    Decompress(#[source] std::io::Error),

    #[error("section ends at offset {end} but parsing reached offset {position}")]
    SectionOverrun { end: usize, position: usize },

    #[error("could not read manifest file")]
    Io(#[from] std::io::Error),

    #[error("seek to offset {offset} is past the end of the data ({len} bytes)")]
    SeekOutOfBounds { offset: usize, len: usize },
}

pub type Result<T> = std::result::Result<T, Error>;
