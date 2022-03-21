use std::fmt::Display;

const BOSE_VID: u16 = 0x05a7;

const COMPATIBLE_DEVICES: &[DeviceIds] = &[
    // Bose Color II SoundLink
    bose_dev(0x40fe, 0x400d),
];

const fn bose_dev(normal_pid: u16, dfu_pid: u16) -> DeviceIds {
    DeviceIds {
        normal_mode: UsbId {
            vid: BOSE_VID,
            pid: normal_pid,
        },
        dfu_mode: UsbId {
            vid: BOSE_VID,
            pid: dfu_pid,
        },
    }
}

/// A USB vendor ID and product ID pair.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct UsbId {
    pub vid: u16,
    pub pid: u16,
}

impl Display for UsbId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vid, self.pid)
    }
}

#[derive(Copy, Clone, Debug)]
struct DeviceIds {
    normal_mode: UsbId,
    dfu_mode: UsbId,
}

impl DeviceIds {
    /// If one of our modes uses with the given ID, return it. Otherwise, return [None].
    fn match_id(&self, id: UsbId) -> Option<DeviceMode> {
        if id == self.normal_mode {
            Some(DeviceMode::Normal)
        } else if id == self.dfu_mode {
            Some(DeviceMode::Dfu)
        } else {
            None
        }
    }
}

/// Modes a device can be in. Can be unknown.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DeviceMode {
    Normal,
    Dfu,
    Unknown,
}

impl Display for DeviceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DeviceMode::Normal => write!(f, "normal"),
            DeviceMode::Dfu => write!(f, "DFU"),
            DeviceMode::Unknown => write!(f, "unknown"),
        }
    }
}

/// Compatibility of a device, with detected mode if applicable.
pub enum DeviceCompat {
    /// Known to speak the Bose DFU protocol. Usable by default.
    Compatible(DeviceMode),
    /// May speak the Bose DFU protocol but has not been tested. Usable with "force" flag. Mode
    /// currently always [DeviceMode::Unknown], but that may change if we find a non-PID way to
    /// identify different modes (e.g. parsing the HID descriptor).
    Untested(DeviceMode),
    /// Definitely does not speak the Bose DFU protocol. Treated as if it doesn't exist.
    Incompatible,
}

impl Display for DeviceCompat {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DeviceCompat::Compatible(mode) => write!(f, "compatible device in {} mode", mode),
            DeviceCompat::Untested(mode) => write!(f, "UNTESTED device in {} mode", mode),
            DeviceCompat::Incompatible => write!(f, "incompatible device"),
        }
    }
}

/// Find a device's compatibility and mode based on its USB ID.
pub fn identify_device(id: UsbId) -> DeviceCompat {
    // First, see if the device is known to us.
    for candidate in COMPATIBLE_DEVICES {
        if let Some(mode) = candidate.match_id(id) {
            return DeviceCompat::Compatible(mode);
        }
    }

    // If not, mark it as untested if it has Bose's VID.
    if id.vid == BOSE_VID {
        DeviceCompat::Untested(DeviceMode::Unknown)
    } else {
        DeviceCompat::Incompatible
    }
}
