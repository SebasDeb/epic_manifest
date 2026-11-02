use crate::error::Result;
use crate::reader::Reader;

/// Build metadata: which app this is and how to launch it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub data_version: u8,
    pub feature_level: u32,
    pub is_file_data: bool,
    pub app_id: u32,
    pub app_name: String,
    pub build_version: String,
    pub launch_exe: String,
    pub launch_command: String,
    pub prereq_ids: Vec<String>,
    pub prereq_name: String,
    pub prereq_path: String,
    pub prereq_args: String,
    /// Empty when `data_version` is 0; the build id is not derived here.
    pub build_id: String,
    pub uninstall_action_path: String,
    pub uninstall_action_args: String,
}

impl Meta {
    pub fn read(reader: &mut Reader) -> Result<Meta> {
        let section = reader.begin_section()?;
        let data_version = section.version;

        let feature_level = reader.read_u32()?;
        let is_file_data = reader.read_u8()? == 1;
        let app_id = reader.read_u32()?;
        let app_name = reader.read_fstring()?;
        let build_version = reader.read_fstring()?;
        let launch_exe = reader.read_fstring()?;
        let launch_command = reader.read_fstring()?;
        let prereq_ids = read_prereq_ids(reader)?;
        let prereq_name = reader.read_fstring()?;
        let prereq_path = reader.read_fstring()?;
        let prereq_args = reader.read_fstring()?;

        let mut build_id = String::new();
        if data_version >= 1 {
            build_id = reader.read_fstring()?;
        }
        let mut uninstall_action_path = String::new();
        let mut uninstall_action_args = String::new();
        if data_version >= 2 {
            uninstall_action_path = reader.read_fstring()?;
            uninstall_action_args = reader.read_fstring()?;
        }

        reader.end_section(&section)?;

        Ok(Meta {
            data_version,
            feature_level,
            is_file_data,
            app_id,
            app_name,
            build_version,
            launch_exe,
            launch_command,
            prereq_ids,
            prereq_name,
            prereq_path,
            prereq_args,
            build_id,
            uninstall_action_path,
            uninstall_action_args,
        })
    }
}

fn read_prereq_ids(reader: &mut Reader) -> Result<Vec<String>> {
    let count = reader.read_u32()?;
    // No with_capacity: the count comes from the file and may be garbage.
    let mut ids = Vec::new();
    for _ in 0..count {
        ids.push(reader.read_fstring()?);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    fn fstring(text: &str) -> Vec<u8> {
        if text.is_empty() {
            return 0i32.to_le_bytes().to_vec();
        }
        let mut bytes = ((text.len() + 1) as i32).to_le_bytes().to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes
    }

    /// Builds a meta section; `extra` is appended inside the section.
    fn meta_bytes(data_version: u8, prereq_ids: &[&str], extra: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&21u32.to_le_bytes());
        body.push(0);
        body.extend_from_slice(&7u32.to_le_bytes());
        body.extend(fstring("MyGame"));
        body.extend(fstring("1.2.3-build"));
        body.extend(fstring("Game.exe"));
        body.extend(fstring("-windowed"));
        body.extend_from_slice(&(prereq_ids.len() as u32).to_le_bytes());
        for id in prereq_ids {
            body.extend(fstring(id));
        }
        body.extend(fstring("Redist"));
        body.extend(fstring("redist.exe"));
        body.extend(fstring("/quiet"));
        if data_version >= 1 {
            body.extend(fstring("build-id-123"));
        }
        if data_version >= 2 {
            body.extend(fstring("uninstall.exe"));
            body.extend(fstring("/remove"));
        }
        body.extend_from_slice(extra);

        let mut bytes = ((body.len() + 5) as u32).to_le_bytes().to_vec();
        bytes.push(data_version);
        bytes.extend(body);
        bytes
    }

    #[test]
    fn reads_a_version_0_meta() {
        let bytes = meta_bytes(0, &["prereq-a", "prereq-b"], &[]);
        let mut reader = Reader::new(&bytes);
        let meta = Meta::read(&mut reader).unwrap();
        assert_eq!(meta.data_version, 0);
        assert_eq!(meta.feature_level, 21);
        assert!(!meta.is_file_data);
        assert_eq!(meta.app_id, 7);
        assert_eq!(meta.app_name, "MyGame");
        assert_eq!(meta.build_version, "1.2.3-build");
        assert_eq!(meta.launch_exe, "Game.exe");
        assert_eq!(meta.launch_command, "-windowed");
        assert_eq!(meta.prereq_ids, ["prereq-a", "prereq-b"]);
        assert_eq!(meta.prereq_name, "Redist");
        assert_eq!(meta.prereq_path, "redist.exe");
        assert_eq!(meta.prereq_args, "/quiet");
        assert_eq!(meta.build_id, "");
        assert_eq!(meta.uninstall_action_path, "");
        assert_eq!(reader.position(), bytes.len());
    }

    #[test]
    fn version_2_adds_the_uninstall_actions() {
        let bytes = meta_bytes(2, &[], &[]);
        let meta = Meta::read(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(meta.build_id, "build-id-123");
        assert_eq!(meta.uninstall_action_path, "uninstall.exe");
        assert_eq!(meta.uninstall_action_args, "/remove");
    }

    #[test]
    fn skips_fields_added_by_a_newer_version() {
        let mut bytes = meta_bytes(3, &[], &[1, 2, 3, 4, 5, 6]);
        bytes.push(0x42);
        let mut reader = Reader::new(&bytes);
        let meta = Meta::read(&mut reader).unwrap();
        assert_eq!(meta.uninstall_action_args, "/remove");
        assert_eq!(reader.read_u8().unwrap(), 0x42);
    }

    #[test]
    fn rejects_a_section_size_smaller_than_the_fields() {
        let mut bytes = meta_bytes(0, &[], &[]);
        bytes[0..4].copy_from_slice(&20u32.to_le_bytes());
        let result = Meta::read(&mut Reader::new(&bytes));
        assert!(matches!(result, Err(Error::SectionOverrun { .. })));
    }
}
