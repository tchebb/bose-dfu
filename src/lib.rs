/// Load and validate firmware update files containing suffixes as defined the DFU spec.
pub mod dfu_file;

/// Perform firmware-related operations on a connected Bose USB device using HID reports.
pub mod protocol;
