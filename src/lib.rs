use anyhow::{Context, Result};
use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;
use std::io::Write;
use thiserror::Error;

#[repr(u8)]
#[derive(Debug, Eq, PartialEq, TryFromPrimitive, Copy, Clone)]
#[allow(non_camel_case_types)] // Names from DFU spec
pub enum DfuStatus {
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
    pub fn error_str(&self) -> &'static str {
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
pub enum DfuState {
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
    pub fn read_from_device(device: &hidapi::HidDevice) -> Result<Self> {
        let mut report = [0u8; 2];
        report[0] = DfuReportType::StateCmd as u8;
        device
            .get_feature_report(&mut report)
            .context("failed to read state")?;

        Self::try_from(report[1]).map_err(Into::into)
    }

    pub fn ensure(self, expected: Self) -> Result<()> {
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
pub struct DfuStatusResult {
    pub status: DfuStatus,
    pub state: DfuState,
    pub poll_timeout: u32,
}

impl DfuStatusResult {
    pub fn read_from_device(device: &hidapi::HidDevice) -> Result<Self> {
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

    pub fn ensure_ok(&self) -> Result<()> {
        if self.status != DfuStatus::OK {
            Err(ProtocolError::ErrorStatus(self.status).into())
        } else {
            Ok(())
        }
    }

    pub fn ensure_state(&self, expected: DfuState) -> Result<()> {
        self.state.ensure(expected)
    }
}

#[repr(u8)]
enum DfuReportType {
    // Getting this descriptor executes DFU_UPLOAD, returning its payload
    // appended to a five-byte header containing the 16-bit, little-endian
    // payload length followed by three unknown bytes ([0x00, 0x00, 0x5d] in
    // my tests).
    // Setting it executes DFU_DNLOAD, taking request data consisting of the
    // payload appended to a five-byte header containing (in order) the
    // constant byte 0x01 (= DFU_DNLOAD); the 16-bit, little-endian block
    // number; and the 16-bit, little-endian payload length.
    UploadDownload = 1,

    // Getting this descriptor executes DFU_GETSTATUS and returns its payload.
    // Setting it appears to always fail.
    GetStatus = 2,

    // Getting this descriptor executes DFU_GETSTATE and returns its payload.
    // Setting it executes a DFU request identified by the first byte of the
    // request data. DFU_CLRSTATUS and DFU_ABORT can be executed this way, and
    // possibly others.
    StateCmd = 3,
}

#[repr(u8)]
#[allow(non_camel_case_types)] // Names from DFU spec
#[allow(dead_code)] // All entries from spec included for completeness
enum DfuRequest {
    DFU_DETACH = 0,
    DFU_DNLOAD = 1,
    DFU_UPLOAD = 2,
    DFU_GETSTATUS = 3,
    DFU_CLRSTATUS = 4,
    DFU_GETSTATE = 5,
    DFU_ABORT = 6,
    BOSE_EXIT_DFU = 0xff, // Custom, not from DFU spec
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

pub fn enter_dfu(device: &hidapi::HidDevice) -> Result<()> {
    device
        .send_feature_report(&[1, 0xb0, 0x07])
        .map_err(Into::into)
}

pub fn leave_dfu(device: &hidapi::HidDevice) -> Result<()> {
    device
        .send_feature_report(&[
            DfuReportType::StateCmd as u8,
            DfuRequest::BOSE_EXIT_DFU as u8,
        ])
        .map_err(Into::into)
}

const FW_TRANSFER_SIZE: usize = 1022;

pub fn upload(device: &hidapi::HidDevice, file: &mut impl Write) -> Result<()> {
    const UPLOAD_HEADER_SIZE: usize = 5;

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
            // Short read means we're done, device should now be idle.
            status.ensure_state(DfuState::dfuIDLE)?;
            break;
        } else {
            status.ensure_state(DfuState::dfuUPLOAD_IDLE)?;
        }
    }

    Ok(())
}
