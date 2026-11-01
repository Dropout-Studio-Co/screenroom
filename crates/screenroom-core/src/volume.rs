use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeIdentity {
    pub device_id: u64,

    pub volume_id: Option<String>,
}

impl VolumeIdentity {
    pub fn new(device_id: u64, volume_id: Option<String>) -> Self {
        Self {
            device_id,
            volume_id,
        }
    }

    pub fn probe(path: &Path) -> Option<Self> {
        let device_id = device_id(path)?;
        Some(Self {
            device_id,
            volume_id: platform::volume_id(path),
        })
    }

    pub fn matches(&self, other: &Self) -> bool {
        match (&self.volume_id, &other.volume_id) {
            (Some(left), Some(right)) => left == right,
            _ => self.device_id == other.device_id,
        }
    }

    pub(crate) fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub(crate) fn decode(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }
}

#[cfg(unix)]
fn device_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(path).ok().map(|metadata| metadata.dev())
}

#[cfg(windows)]
fn device_id(path: &Path) -> Option<u64> {
    platform::volume_serial(path).map(u64::from)
}

#[cfg(not(any(unix, windows)))]
fn device_id(_path: &Path) -> Option<u64> {
    None
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{CString, c_char, c_int, c_ulong, c_void};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[repr(C)]
    struct AttributeList {
        bitmap_count: u16,
        reserved: u16,
        common: u32,
        volume: u32,
        directory: u32,
        file: u32,
        fork: u32,
    }

    unsafe extern "C" {
        fn getattrlist(
            path: *const c_char,
            attributes: *mut AttributeList,
            buffer: *mut c_void,
            buffer_size: usize,
            options: c_ulong,
        ) -> c_int;
    }

    const ATTR_VOL_INFO: u32 = 0x8000_0000;
    const ATTR_VOL_UUID: u32 = 0x0004_0000;
    const UUID_BYTES: usize = 16;
    const LENGTH_PREFIX_BYTES: usize = 4;
    const UUID_ATTRIBUTE_BYTES: usize = LENGTH_PREFIX_BYTES + UUID_BYTES;

    pub(super) fn volume_id(path: &Path) -> Option<String> {
        let path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut attributes = AttributeList {
            bitmap_count: 5,
            reserved: 0,
            common: 0,
            volume: ATTR_VOL_INFO | ATTR_VOL_UUID,
            directory: 0,
            file: 0,
            fork: 0,
        };
        let mut buffer = [0_u8; 64];
        let status = unsafe {
            getattrlist(
                path.as_ptr(),
                &mut attributes,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
            )
        };
        if status != 0 {
            return None;
        }
        let length = u32::from_ne_bytes(buffer[..LENGTH_PREFIX_BYTES].try_into().ok()?);
        if length as usize != UUID_ATTRIBUTE_BYTES {
            return None;
        }
        let bytes = &buffer[LENGTH_PREFIX_BYTES..UUID_ATTRIBUTE_BYTES];
        Some(
            bytes
                .iter()
                .enumerate()
                .map(|(index, byte)| {
                    let separator = matches!(index, 4 | 6 | 8 | 10);
                    format!("{}{byte:02x}", if separator { "-" } else { "" })
                })
                .collect(),
        )
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const MAX_PATH: usize = 260;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetVolumePathNameW(
            file_name: *const u16,
            volume_path_name: *mut u16,
            buffer_length: u32,
        ) -> i32;

        fn GetVolumeInformationW(
            root_path_name: *const u16,
            volume_name_buffer: *mut u16,
            volume_name_size: u32,
            volume_serial_number: *mut u32,
            maximum_component_length: *mut u32,
            file_system_flags: *mut u32,
            file_system_name_buffer: *mut u16,
            file_system_name_size: u32,
        ) -> i32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(super) fn volume_serial(path: &Path) -> Option<u32> {
        let path = wide(path);
        let mut root = [0_u16; MAX_PATH + 1];
        let named =
            unsafe { GetVolumePathNameW(path.as_ptr(), root.as_mut_ptr(), root.len() as u32) };
        if named == 0 {
            return None;
        }
        let mut serial = 0_u32;
        let described = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut serial,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        (described != 0).then_some(serial)
    }

    pub(super) fn volume_id(path: &Path) -> Option<String> {
        volume_serial(path).map(|serial| format!("{serial:08X}"))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::path::Path;

    pub(super) fn volume_id(_path: &Path) -> Option<String> {
        None
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use std::path::Path;

    pub(super) fn volume_id(_path: &Path) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_volume_identifier_survives_a_changed_device_number() {
        let stored = VolumeIdentity::new(1, Some("abc".into()));
        let remounted = VolumeIdentity::new(9, Some("abc".into()));
        assert!(stored.matches(&remounted));
    }

    #[test]
    fn a_different_volume_identifier_is_not_the_same_volume() {
        let stored = VolumeIdentity::new(1, Some("abc".into()));
        assert!(!stored.matches(&VolumeIdentity::new(1, Some("def".into()))));
        assert!(!stored.matches(&VolumeIdentity::new(2, Some("def".into()))));
    }

    #[test]
    fn without_identifiers_the_device_number_decides() {
        let stored = VolumeIdentity::new(7, None);
        assert!(stored.matches(&VolumeIdentity::new(7, Some("abc".into()))));
        assert!(!stored.matches(&VolumeIdentity::new(8, None)));
    }

    #[test]
    fn an_identity_round_trips_through_its_stored_form() {
        let identity = VolumeIdentity::new(42, Some("cc40d1f0-c6fb-46c4".into()));
        assert_eq!(
            VolumeIdentity::decode(&identity.encode()),
            Some(identity.clone())
        );
        assert_eq!(VolumeIdentity::decode("not an identity"), None);
        assert_eq!(VolumeIdentity::decode(""), None);
    }

    #[test]
    fn a_real_directory_reports_a_readable_identity() {
        let directory = tempfile::tempdir().unwrap();
        let identity = VolumeIdentity::probe(directory.path()).expect("identity");
        assert!(identity.device_id > 0);
        let again = VolumeIdentity::probe(directory.path()).expect("identity");
        assert!(identity.matches(&again));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_volume_reports_a_stable_identifier() {
        let identity = VolumeIdentity::probe(Path::new("/")).expect("identity");
        assert!(
            identity.volume_id.is_some(),
            "macOS must report a volume UUID for the boot volume"
        );
        assert_eq!(
            identity.volume_id.as_deref().map(str::len),
            Some(36),
            "the identifier is a formatted UUID"
        );
    }
}
