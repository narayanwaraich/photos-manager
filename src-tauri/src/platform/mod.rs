pub mod macos;
pub mod stub;
pub mod windows;

use std::path::Path;

use crate::domain::{RootError, VolumeInfo};

/// Formats a Windows volume serial number as a namespaced volume identity (TASK-1F decision h).
pub fn format_serial_identity(serial: u32) -> String {
    format!("serial:{serial:08X}")
}

/// Returns `true` if `value` is a legacy bare 8-character hexadecimal volume serial.
fn is_legacy_bare_serial(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Returns `true` if `value` is a `serial:` identity or a legacy bare 8-hex volume serial.
pub fn is_serial_identity(value: &str) -> bool {
    value.starts_with("serial:") || is_legacy_bare_serial(value)
}

/// Compares a stored volume `fs_uuid` against a live one (TASK-1F decision h).
///
/// Treats a stored bare 8-hex value (legacy Windows fallback) as equal to `serial:` plus that value.
pub fn same_volume_identity(stored: &str, live: &str) -> bool {
    if stored == live {
        return true;
    }
    if is_legacy_bare_serial(stored) {
        if let Some(live_hex) = live.strip_prefix("serial:") {
            return stored.eq_ignore_ascii_case(live_hex);
        }
    }
    false
}

/// Dispatches volume description to the platform-specific implementation.
pub fn describe_volume(path: &Path) -> Result<VolumeInfo, RootError> {
    #[cfg(target_os = "macos")]
    {
        macos::describe_volume_macos(path)
    }
    #[cfg(target_os = "windows")]
    {
        windows::describe_volume_windows(path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        stub::describe_volume_stub(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_serial_identity_uses_serial_prefix_and_uppercase_hex() {
        assert_eq!(format_serial_identity(0x0A1B_2C3D), "serial:0A1B2C3D");
        assert_eq!(format_serial_identity(0x0000_00FF), "serial:000000FF");
    }

    #[test]
    fn test_same_volume_identity_handles_exact_and_legacy_bare_serial() {
        assert!(same_volume_identity(
            "12345678-1234-1234-1234-1234567890ab",
            "12345678-1234-1234-1234-1234567890ab"
        ));
        assert!(same_volume_identity("serial:0A1B2C3D", "serial:0A1B2C3D"));
        assert!(same_volume_identity("0A1B2C3D", "serial:0A1B2C3D"));
        assert!(same_volume_identity("0a1b2c3d", "serial:0A1B2C3D"));
        assert!(!same_volume_identity("0A1B2C3E", "serial:0A1B2C3D"));
        assert!(!same_volume_identity("serial:0A1B2C3E", "serial:0A1B2C3D"));
        assert!(!same_volume_identity("not8hex", "serial:not8hex"));
        assert!(!same_volume_identity("ZZZZZZZZ", "serial:ZZZZZZZZ"));
        assert!(is_serial_identity("serial:0A1B2C3D"));
        assert!(is_serial_identity("0A1B2C3D"));
        assert!(!is_serial_identity("12345678-1234-1234-1234-1234567890ab"));
    }
}
