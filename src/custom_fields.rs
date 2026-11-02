use std::collections::HashMap;

use crate::error::Result;
use crate::reader::Reader;

/// Reads the key/value pairs the build tooling attaches to a manifest. Keys
/// are stored first, then all values. A repeated key keeps its last value.
pub fn read_custom_fields(reader: &mut Reader) -> Result<HashMap<String, String>> {
    let section = reader.begin_section()?;
    let count = reader.read_u32()?;

    // Each key takes at least four bytes, so a bogus count cannot make the
    // vector grow past what the data holds.
    let mut keys = Vec::new();
    for _ in 0..count {
        keys.push(reader.read_fstring()?);
    }
    let mut fields = HashMap::new();
    for key in keys {
        fields.insert(key, reader.read_fstring()?);
    }

    reader.end_section(&section)?;
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    fn fstring(text: &str) -> Vec<u8> {
        let mut bytes = ((text.len() + 1) as i32).to_le_bytes().to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes
    }

    fn custom_fields_bytes(pairs: &[(&str, &str)], extra: &[u8]) -> Vec<u8> {
        let mut body = (pairs.len() as u32).to_le_bytes().to_vec();
        for (key, _) in pairs {
            body.extend(fstring(key));
        }
        for (_, value) in pairs {
            body.extend(fstring(value));
        }
        body.extend_from_slice(extra);

        let mut bytes = ((body.len() + 5) as u32).to_le_bytes().to_vec();
        bytes.push(0);
        bytes.extend(body);
        bytes
    }

    #[test]
    fn reads_keys_and_values() {
        let bytes =
            custom_fields_bytes(&[("BaseUrl", "https://cdn.example"), ("Tag", "beta")], &[]);
        let mut reader = Reader::new(&bytes);
        let fields = read_custom_fields(&mut reader).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields["BaseUrl"], "https://cdn.example");
        assert_eq!(fields["Tag"], "beta");
        assert_eq!(reader.position(), bytes.len());
    }

    #[test]
    fn rejects_a_count_larger_than_the_data() {
        let mut bytes = custom_fields_bytes(&[("k", "v")], &[]);
        bytes[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        let result = read_custom_fields(&mut Reader::new(&bytes));
        assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
    }
}
