use std::fmt;

/// An Unreal GUID: four little-endian `u32` words, not a UUID.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Guid(pub [u32; 4]);

impl fmt::Display for Guid {
    /// Prints the 32 uppercase hex digits Unreal uses when it names chunk files.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d] = self.0;
        write!(f, "{a:08X}{b:08X}{c:08X}{d:08X}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_four_words_as_32_uppercase_hex_digits() {
        let guid = Guid([0x1, 0xABCD_EF01, 0, 0xFFFF_FFFF]);
        assert_eq!(guid.to_string(), "00000001ABCDEF0100000000FFFFFFFF");
    }
}
