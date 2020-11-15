use anyhow::{Context, Result};
use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;
use std::io::prelude::*;
use structopt::StructOpt;
use thiserror::Error;

const BOSE_VID: u16 = 0x05a7;

const SUPPORTED_NONDFU_PIDS: &[u16] = &[
    0x40fe, // Bose Color II SoundLink
];

const SUPPORTED_DFU_PIDS: &[u16] = &[
    0x400d, // Bose Color II SoundLink
];

fn get_mode(pid: u16) -> Option<DeviceMode> {
    match pid {
        v if SUPPORTED_NONDFU_PIDS.contains(&v) => Some(DeviceMode::Normal),
        v if SUPPORTED_DFU_PIDS.contains(&v) => Some(DeviceMode::Dfu),
        _ => None,
    }
}

#[derive(StructOpt, Debug)]
#[structopt(name = "bose-dfu")]
enum Opt {
    /// List all connected Bose HID devices (vendor ID 0x05a7)
    List,

    /// Put a device into DFU mode
    EnterDfu {
        #[structopt(flatten)]
        spec: DeviceSpec,
    },

    /// Take a device out of DFU mode
    LeaveDfu {
        #[structopt(flatten)]
        spec: DeviceSpec,
    },

    /// Read the firmware of a device in DFU mode
    Upload {
        #[structopt(flatten)]
        spec: DeviceSpec,

        #[structopt(parse(from_os_str))]
        file: std::path::PathBuf,
    },
}

#[derive(Error, Debug)]
enum MatchError {
    #[error("no devices match specification")]
    NoDevices,

    #[error("multiple devices match specification")]
    MultipleDevices,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum DeviceMode {
    Normal,
    Dfu,
}

#[derive(StructOpt, Debug)]
struct DeviceSpec {
    /// Serial number
    #[structopt(short)]
    serial: Option<String>,

    /// Product ID (vendor ID is always matched against Bose's, 0x05a7)
    #[structopt(short)]
    pid: Option<u16>,

    /// DFU/normal mode (determined using product ID for known devices)
    #[structopt(skip)]
    required_mode: Option<DeviceMode>,
}

impl DeviceSpec {
    fn matches(&self, device: &hidapi::DeviceInfo) -> bool {
        if device.vendor_id() != BOSE_VID {
            return false;
        }

        if let Some(ref x) = self.serial {
            if device.serial_number() != Some(&x) {
                return false;
            }
        }

        if let Some(x) = self.pid {
            if device.product_id() != x {
                return false;
            }
        }

        if let Some(mode) = self.required_mode {
            // TODO: Handle unknown devices
            if get_mode(device.product_id()) != Some(mode) {
                return false;
            }
        }

        return true;
    }

    fn get_device(&self, hidapi: &hidapi::HidApi) -> Result<hidapi::HidDevice> {
        let mut candidates = hidapi.device_list().filter(|d| self.matches(d));

        match candidates.next() {
            None => Err(MatchError::NoDevices.into()),
            Some(dev) => {
                if candidates.next().is_some() {
                    Err(MatchError::MultipleDevices.into())
                } else {
                    dev.open_device(hidapi).map_err(Into::into)
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let mode = Opt::from_args();

    let api = hidapi::HidApi::new()?;

    match mode {
        Opt::List => list(&api),
        Opt::EnterDfu { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Normal),
                ..spec
            };
            enter_dfu(&spec.get_device(&api)?)?;
        }
        Opt::LeaveDfu { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Dfu),
                ..spec
            };
            leave_dfu(&spec.get_device(&api)?)?;
        }
        Opt::Upload { spec, file: path } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Dfu),
                ..spec
            };

            let device = &spec.get_device(&api)?;

            let state = DfuState::read_from_device(device)?;
            state
                .ensure(DfuState::dfuIDLE)
                .context("device not idle, please re-enter DFU mode")?;

            let mut file = std::fs::File::create(path)?;
            upload(device, &mut file)?;
        }
    };

    Ok(())
}

fn list(hidapi: &hidapi::HidApi) {
    let all_spec = DeviceSpec {
        serial: None,
        pid: None,
        required_mode: None,
    };
    for dev in hidapi.device_list().filter(|d| all_spec.matches(d)) {
        let support_status = match get_mode(dev.product_id()) {
            Some(DeviceMode::Normal) => "not in DFU mode, known device",
            Some(DeviceMode::Dfu) => "in DFU mode, known device",
            None => "unknown device, proceed at your own risk",
        };

        println!(
            "{} {} [{}]",
            dev.serial_number().unwrap_or("INVALID"),
            dev.product_string().unwrap_or("INVALID"),
            support_status,
        );
    }
}

#[repr(u8)]
enum DfuReportType {
    UploadDownload = 1,
    GetStatus = 2,
    State = 3,
}

fn enter_dfu(device: &hidapi::HidDevice) -> Result<()> {
    device
        .send_feature_report(&[1, 0xb0, 0x07])
        .map_err(Into::into)
}

fn leave_dfu(device: &hidapi::HidDevice) -> Result<()> {
    device
        .send_feature_report(&[DfuReportType::State as u8, 0xff])
        .map_err(Into::into)
}

#[derive(Error, Debug)]
enum ProtocolError {
    #[error("device reported an error: {0:?} ({})", .0.error_str())]
    ErrorStatus(DfuStatus),

    #[error("device entered unexpected state: expected {expected:?}, got {actual:?}")]
    UnexpectedState {
        expected: DfuState,
        actual: DfuState,
    },
}

#[repr(u8)]
#[derive(Debug, Eq, PartialEq, TryFromPrimitive, Copy, Clone)]
#[allow(non_camel_case_types)] // Names from DFU spec
enum DfuStatus {
    OK = 0x00,
    errTARGET = 0x01,
    errFILE = 0x02,
    errWRITE = 0x03,
    errERASE = 0x04,
    errCHECK_ERASED = 0x05,
    errPROG = 0x06,
    errVERIFY = 0x07,
    errADDRESS = 0x08,
    errNOTDONE = 0x09,
    errFIRMWARE = 0x0a,
    errVENDOR = 0x0b,
    errUSBR = 0x0c,
    errPOR = 0x0d,
    errUNKNOWN = 0x0e,
    errSTALLEDPKT = 0x0f,
}

impl DfuStatus {
    fn error_str(&self) -> &'static str {
        use DfuStatus::*;
        match self {
            OK => "No error condition is present.",
            errTARGET => "File is not targeted for use by this device.",
            errFILE => "File is for this device but fails some vendor-specific verification test.",
            errWRITE => "Device is unable to write memory.",
            errERASE => "Memory erase function failed.",
            errCHECK_ERASED => "Memory erase check failed.",
            errPROG => "Program memory function failed.",
            errVERIFY => "Programmed memory failed verification.",
            errADDRESS => "Cannot program memory due to received address that is out of range.",
            errNOTDONE => "Received DFU_DNLOAD with wLength = 0, but device does not think it has all of the data yet.",
            errFIRMWARE => "Device’s firmware is corrupt. It cannot return to run-time (non-DFU operations.",
            errVENDOR => "iString indicates a vendor-specific error.",
            errUSBR => "Device detected unexpected USB reset signaling.",
            errPOR => "Device detected unexpected power on reset.",
            errUNKNOWN => "Something went wrong, but the device does not know what it was.",
            errSTALLEDPKT => "Device stalled an unexpected request.",
        }
    }
}

#[repr(u8)]
#[derive(Debug, Eq, PartialEq, TryFromPrimitive, Copy, Clone)]
#[allow(non_camel_case_types)] // Names from DFU spec
enum DfuState {
    appIDLE = 0,
    appDETACH = 1,
    dfuIDLE = 2,
    dfuDNLOAD_SYNC = 3,
    dfuDNBUSY = 4,
    dfuDNLOAD_IDLE = 5,
    dfuMANIFEST_SYNC = 6,
    dfuMANIFEST = 7,
    dfuMANIFEST_WAIT_RESET = 8,
    dfuUPLOAD_IDLE = 9,
    dfuERROR = 10,
}

impl DfuState {
    fn read_from_device(device: &hidapi::HidDevice) -> Result<Self> {
        let mut report = [0u8; 2];
        report[0] = DfuReportType::State as u8;
        device
            .get_feature_report(&mut report)
            .context("failed to read state")?;

        Self::try_from(report[1]).map_err(Into::into)
    }

    fn ensure(self, expected: Self) -> Result<()> {
        if self != expected {
            Err(ProtocolError::UnexpectedState {
                expected,
                actual: self,
            }
            .into())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct DfuStatusResult {
    status: DfuStatus,
    state: DfuState,
    poll_timeout: u32,
}

impl DfuStatusResult {
    fn read_from_device(device: &hidapi::HidDevice) -> Result<Self> {
        let mut report = [0u8; 6];
        report[0] = DfuReportType::GetStatus as u8;
        device
            .get_feature_report(&mut report)
            .context("failed to read status")?;

        let mut cursor = std::io::Cursor::new(report);
        cursor.set_position(1); // Skip report number

        let status = DfuStatus::try_from(cursor.read_u8().unwrap())?;
        let poll_timeout = cursor.read_u24::<LittleEndian>().unwrap();
        let state = DfuState::try_from(cursor.read_u8().unwrap())?;

        Ok(Self {
            status,
            poll_timeout,
            state,
        })
    }

    fn ensure_ok(&self) -> Result<()> {
        if self.status != DfuStatus::OK {
            Err(ProtocolError::ErrorStatus(self.status).into())
        } else {
            Ok(())
        }
    }

    fn ensure_state(&self, expected: DfuState) -> Result<()> {
        self.state.ensure(expected)
    }
}

const FW_TRANSFER_SIZE: usize = 1022;
const UPLOAD_HEADER_SIZE: usize = 5;

fn upload(device: &hidapi::HidDevice, file: &mut std::fs::File) -> Result<()> {
    let mut report = [0u8; FW_TRANSFER_SIZE + 1];

    loop {
        report[0] = DfuReportType::UploadDownload as u8;
        device
            .get_feature_report(&mut report)
            .context("failed to read firmware data chunk")?;
        let status = DfuStatusResult::read_from_device(device)?;
        status.ensure_ok()?;

        let data_size = LittleEndian::read_u16(&report[1..3]) as usize;
        let data_start = UPLOAD_HEADER_SIZE + 1;
        file.write(&report[data_start..data_start + data_size])?;

        if data_size != FW_TRANSFER_SIZE - UPLOAD_HEADER_SIZE {
            //  Short read means we're done, device should now be idle.
            status.ensure_state(DfuState::dfuIDLE)?;
            break;
        } else {
            status.ensure_state(DfuState::dfuUPLOAD_IDLE)?;
        }
    }

    Ok(())
}
