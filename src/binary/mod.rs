//! Low-level binary primitives shared by every section of the format.

pub(crate) mod guid;
pub(crate) mod reader;

pub use guid::Guid;
pub(crate) use reader::Reader;
