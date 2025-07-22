use byteorder::{ByteOrder, LE, ReadBytesExt, WriteBytesExt};
use hidapi::{HidDevice, HidError};
use log::{info, trace};
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;
use std::io::{Read, Write};
use std::thread::sleep;
use std::time::Duration;
use thiserror::Error;

const XFER_HEADER_SIZE: usize = 5;
// Gathered from USB captures. Probably corresponds to a 1024-byte internal buffer in the firmware.
const XFER_DATA_SIZE: usize = 1017;

/// Download (i.e. write firmware to) the device. `device` must be in DFU mode. `file` should
/// contain only the firmware payload to be written, with any DFU header stripped off.
pub fn download(device: &HidDevice, file: &mut impl Read) -> Result<(), Error> {
    let mut report = vec![];

    let mut block_num = 0u16;
    let mut prev_delay = Duration::from_millis(0);
    loop {
        report.clear();
        // Reserve 1 byte report ID + header to be filled later.
        report.resize(1 + XFER_HEADER_SIZE, 0u8);

        // Fill the rest with data from the file.
        let data_size = file.take(XFER_DATA_SIZE as _).read_to_end(&mut report)?;

        // Construct header
        let mut cursor = std::io::Cursor::new(&mut report);
        cursor.write_u8(DfuReportId::UploadDownload as _).unwrap();
        cursor.write_u8(DfuRequest::DFU_DNLOAD as _).unwrap();
        cursor.write_u16::<LE>(block_num).unwrap();
        cursor.write_u16::<LE>(data_size as u16).unwrap();
        assert!(cursor.position() == (1 + XFER_HEADER_SIZE) as _); // Add 1 for report ID

        device
            .send_feature_report(&report)
            .map_err(|e| Error::DeviceIoError {
                source: e,
                action: "sending firmware data chunk",
            })?;

        // This emulates the behavior of the official updater, as far as I can tell, but is not
        // compliant with the DFU spec. If the device needs more time, it's supposed to respond
        // to a status request here with a status of dfuDNLOAD_BUSY or dfuMANIFEST with
        // bwPollTimeout set to the number of milliseconds it needs. However, my speaker (SoundLink
        // Color II) appears to stop responding to requests immediately after receiving the last
        // (empty) block without waiting for a status request. Instead, it communicates how long
        // it needs in its *previous* status response (that is, its response to the last non-empty
        // block). That's why we have to persist prev_delay across loop iterations.
        //
        // Notably, although the device does also set bwPollTimeout for non-final blocks, the
        // official updater seems to completely ignore those values and instead just rely on the
        // device to bake the necessary delay into its GET_STATUS response latency. We do the same.
        if data_size == 0 {
            info!("Waiting {prev_delay:?}, as requested by device, for firmware to manifest");
            sleep(prev_delay);
        }

        let status = DfuStatusResult::read_from_device(device)?;
        status.ensure_ok()?;

        prev_delay = Duration::from_millis(status.poll_timeout as _);

        trace!("Successfully downloaded block {block_num:#06x} ({data_size} bytes)");

        if data_size == 0 {
            // Empty read means we're done, device should now be idle.
            status.ensure_state(DfuState::dfuIDLE)?;
            break;
        } else {
            status.ensure_state(DfuState::dfuDNLOAD_IDLE)?;
        }

        block_num = match block_num.checked_add(1) {
            Some(i) => i,
            None => return Err(ProtocolError::FileTooLarge.into()),
        };
    }

    Ok(())
}

/// Upload (i.e. read firmware from) the device. `device` must be in DFU mode. No processing is
/// done on the data written to `file` (for example, a DFU suffix is not added).
pub fn upload(device: &HidDevice, file: &mut impl Write) -> Result<(), Error> {
    // 1 byte report ID + header + data
    let mut report = [0u8; 1 + XFER_HEADER_SIZE + XFER_DATA_SIZE];

    loop {
        // Zero out the report each time through to protect against hidapi bugs.
        report.fill(0u8);

        report[0] = DfuReportId::UploadDownload as u8;
        let report_size = map_gfr(
            device.get_feature_report(&mut report),
            1 + XFER_HEADER_SIZE,
            "reading firmware data chunk",
        )?;

        let status = DfuStatusResult::read_from_device(device)?;
        status.ensure_ok()?;

        let data_size = LE::read_u16(&report[1..3]) as usize;
        let data_start = 1 + XFER_HEADER_SIZE;

        if report_size < data_start + data_size {
            return Err(ProtocolError::ReportTooShort {
                expected: data_start + data_size,
                actual: report_size,
            }
            .into());
        }

        trace!("Successfully uploaded block ({data_size} bytes)");

        file.write_all(&report[data_start..data_start + data_size])?;

        if data_size != XFER_DATA_SIZE {
            // Short read means we're done, device should now be idle.
            status.ensure_state(DfuState::dfuIDLE)?;
            break;
        } else {
            status.ensure_state(DfuState::dfuUPLOAD_IDLE)?;
        }
    }

    Ok(())
}

/// Pieces of information that Bose's normal firmware exposes.
#[derive(Debug)]
#[non_exhaustive]
pub enum InfoField {
    DeviceModel,
    SerialNumber,
    CurrentFirmware,
}

/// Read an information field (as listed in [InfoField]) from the normal firmware. `device` must
/// NOT be in DFU mode.
pub fn read_info_field(device: &HidDevice, field: InfoField) -> Result<String, Error> {
    const INFO_REPORT_ID: u8 = 2;
    const INFO_REPORT_LEN: usize = 126;

    use InfoField::*;

    // 1 byte report ID + 2 bytes field ID + 1 byte NUL
    let mut request_report = [0u8; 1 + 2 + 1];

    // Packet captures indicate that "lc" is also a valid field type for some devices, but on mine
    // it always returns a bus error (both when I send it and when the official updater does).
    request_report[0] = INFO_REPORT_ID;
    request_report[1..3].copy_from_slice(match field {
        DeviceModel => b"pl",
        SerialNumber => b"sn",
        CurrentFirmware => b"vr",
    });

    device
        .send_feature_report(&request_report)
        .map_err(|e| Error::DeviceIoError {
            source: e,
            action: "requesting info field",
        })?;

    let mut response_report = [0u8; 1 + INFO_REPORT_LEN];
    response_report[0] = INFO_REPORT_ID;
    map_gfr(
        device.get_feature_report(&mut response_report),
        1,
        "reading info field",
    )?;

    trace!("Raw {field:?} info field: {response_report:02x?}");

    // Result is all the bytes after the report ID and before the first NUL.
    let result = response_report[1..].split(|&x| x == 0).next().unwrap();

    Ok(std::str::from_utf8(result)
        .map_err(|e| Error::ProtocolError(e.into()))?
        .to_owned())
}

pub fn run_tap_commands(device: &HidDevice) -> Result<(), Error> {
    const TAP_REPORT_ID: u8 = 2;
    const TAP_REPORT_LEN: usize = 2048;

    loop {
        print!("> ");
        std::io::stdout().flush().unwrap();
        let mut tap_command = String::new();
        std::io::stdin()
            .read_line(&mut tap_command)
            .expect("Failed to read line");

        tap_command = tap_command.trim().to_string();
        if tap_command.len() == 0 {
            continue;
        } else if tap_command == "." {
            break;
        }
        let tap_bytes = tap_command.as_bytes();

        // 1 byte report ID + 2 bytes field ID + 1 byte NUL
        let mut request_report = vec![0u8; 1 + tap_bytes.len() + 1].into_boxed_slice();

        // Packet captures indicate that "lc" is also a valid field type for some devices, but on mine
        // it always returns a bus error (both when I send it and when the official updater does).
        request_report[0] = TAP_REPORT_ID;
        request_report[1..tap_bytes.len()+1].copy_from_slice(tap_bytes);

        device
            .send_feature_report(&request_report)
            .map_err(|e| Error::DeviceIoError {
                source: e,
                action: "running TAP command",
            })?;

        let mut response_report = [0u8; 1 + TAP_REPORT_LEN];
        response_report[0] = TAP_REPORT_ID;
        map_gfr(
            device.get_feature_report(&mut response_report),
            1,
            "reading TAP command response",
        )?;

        trace!("Raw {:?} TAP command response: {:02x?}", tap_command, response_report);

        // Result is all the bytes after the report ID and before the first NUL.
        let result = response_report[1..].split(|&x| x == 0).next().unwrap();
        println!("{:?}", std::str::from_utf8(result));
    }

    Ok(())
}

/// Put a device running the normal firmware into DFU mode. `device` must NOT be in DFU mode.
pub fn enter_dfu(device: &HidDevice) -> Result<(), Error> {
    const ENTER_DFU_REPORT_ID: u8 = 1;

    device
        .send_feature_report(&[ENTER_DFU_REPORT_ID, 0xb0, 0x07]) // Magic
        .map_err(|e| Error::DeviceIoError {
            source: e,
            action: "entering DFU mode",
        })
}

/// Switch back to the normal firmware. `device` must be in DFU mode.
pub fn leave_dfu(device: &HidDevice) -> Result<(), Error> {
    device
        .send_feature_report(&[DfuReportId::StateCmd as u8, DfuRequest::BOSE_EXIT_DFU as u8])
        .map_err(|e| Error::DeviceIoError {
            source: e,
            action: "leaving DFU mode",
        })
}

/// Attempt to transition the device to the [dfuIDLE](DfuState::dfuIDLE) state. If we can't or
/// don't know how to, return an error. `device` must be in DFU mode.
pub fn ensure_idle(device: &HidDevice) -> Result<(), Error> {
    use DfuState::*;

    let status = DfuStatusResult::read_from_device(device)?;
    match status.state {
        dfuIDLE => return Ok(()),
        dfuDNLOAD_SYNC | dfuDNLOAD_IDLE | dfuMANIFEST_SYNC | dfuUPLOAD_IDLE => {
            info!(
                "Device not idle, state = {:?}; sending DFU_ABORT",
                status.state
            );

            device
                .send_feature_report(&[DfuReportId::StateCmd as u8, DfuRequest::DFU_ABORT as u8])
                .map_err(|e| Error::DeviceIoError {
                    source: e,
                    action: "sending DFU_ABORT",
                })?;
        }
        dfuERROR => {
            info!(
                "Device in error state, status = {:?} ({}); sending DFU_CLRSTATUS",
                status.status,
                status.status.error_str()
            );

            device
                .send_feature_report(&[
                    DfuReportId::StateCmd as u8,
                    DfuRequest::DFU_CLRSTATUS as u8,
                ])
                .map_err(|e| Error::DeviceIoError {
                    source: e,
                    action: "sending DFU_CLRSTATUS",
                })?;
        }
        _ => return Err(ProtocolError::BadInitialState(status.state).into()),
    };

    // If we had to send a request, ensure it succeeded and we're now idle.
    let status = DfuStatusResult::read_from_device(device)?;
    status.ensure_ok()?;
    status.ensure_state(dfuIDLE).map_err(Into::into)
}

#[repr(u8)]
enum DfuReportId {
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
    // possibly others too.
    StateCmd = 3,
}

/// Status codes a DFU device can return, taken from the USB DFU 1.1 spec.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive)]
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
            errNOTDONE => {
                "Received DFU_DNLOAD with wLength = 0, but device does not think it has all of the data yet."
            }
            errFIRMWARE => {
                "Device's firmware is corrupt. It cannot return to run-time (non-DFU) operations."
            }
            errVENDOR => "iString indicates a vendor-specific error.",
            errUSBR => "Device detected unexpected USB reset signaling.",
            errPOR => "Device detected unexpected power on reset.",
            errUNKNOWN => "Something went wrong, but the device does not know what it was.",
            errSTALLEDPKT => "Device stalled an unexpected request.",
        }
    }
}

/// States a DFU device can be in, taken from the USB DFU 1.1 spec.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive)]
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
    #[allow(dead_code)]
    fn read_from_device(device: &HidDevice) -> Result<Self, Error> {
        let mut report = [0u8; 1 + 1]; // 1 byte report ID + 1 byte state
        report[0] = DfuReportId::StateCmd as u8;
        map_gfr(
            device.get_feature_report(&mut report),
            report.len(),
            "querying state",
        )?;

        Self::try_from(report[1]).map_err(|e| ProtocolError::UnknownState(e.number).into())
    }

    fn ensure(self, expected: Self) -> Result<(), ProtocolError> {
        if self != expected {
            Err(ProtocolError::UnexpectedState {
                expected,
                actual: self,
            })
        } else {
            Ok(())
        }
    }
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

#[derive(Copy, Clone, Debug)]
struct DfuStatusResult {
    pub status: DfuStatus,
    pub state: DfuState,
    pub poll_timeout: u32,
}

impl DfuStatusResult {
    fn read_from_device(device: &HidDevice) -> Result<Self, Error> {
        let mut report = [0u8; 1 + 6]; // 1 byte report ID + 6 bytes status
        report[0] = DfuReportId::GetStatus as u8;
        map_gfr(
            device.get_feature_report(&mut report),
            report.len(),
            "querying status",
        )?;

        let mut cursor = std::io::Cursor::new(report);
        cursor.set_position(1); // Skip report number

        let status = DfuStatus::try_from(cursor.read_u8().unwrap())
            .map_err(|e| ProtocolError::UnknownState(e.number))?;
        let poll_timeout = cursor.read_u24::<LE>().unwrap();
        let state = DfuState::try_from(cursor.read_u8().unwrap())
            .map_err(|e| ProtocolError::UnknownStatus(e.number))?;

        Ok(Self {
            status,
            poll_timeout,
            state,
        })
    }

    fn ensure_ok(&self) -> Result<(), ProtocolError> {
        if self.status != DfuStatus::OK {
            Err(ProtocolError::ErrorStatus(self.status))
        } else {
            Ok(())
        }
    }

    fn ensure_state(&self, expected: DfuState) -> Result<(), ProtocolError> {
        self.state.ensure(expected)
    }
}

/// Map the result of get_feature_report() into an appropriate error if it failed or was too short.
fn map_gfr(
    r: Result<usize, HidError>,
    min_size: usize,
    action: &'static str,
) -> Result<usize, Error> {
    match r {
        Err(e) => Err(Error::DeviceIoError { source: e, action }),
        Ok(s) if s < min_size => Err(ProtocolError::ReportTooShort {
            expected: min_size,
            actual: s,
        }
        .into()),
        Ok(s) => Ok(s),
    }
}

/// All errors (protocol and I/O) that can happen during a DFU operation.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("DFU protocol error")]
    ProtocolError(#[from] ProtocolError),

    #[error("USB transaction error while {action}")]
    DeviceIoError {
        source: HidError,
        action: &'static str,
    },

    #[error("file I/O error")]
    FileIoError(#[from] std::io::Error),
}

/// Failure modes that can happen even when all I/O succeeds.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ProtocolError {
    #[error("device reported state ({0}) that is not in the DFU spec")]
    UnknownState(u8),

    #[error("device reported status ({0}) that is not in the DFU spec")]
    UnknownStatus(u8),

    #[error("device reported an error: {0:?} ({err})", err = .0.error_str())]
    ErrorStatus(DfuStatus),

    #[error("device entered unexpected state: expected {expected:?}, got {actual:?}")]
    UnexpectedState {
        expected: DfuState,
        actual: DfuState,
    },

    #[error("don't know how to safely leave initial state {0:?}; please re-enter DFU mode")]
    BadInitialState(DfuState),

    #[error("file too large: overflowed 16-bit block number while sending")]
    FileTooLarge,

    #[error("device returned invalid UTF-8 string")]
    InvalidString(#[from] std::str::Utf8Error),

    #[error("feature report from device was {actual} bytes, expected at least {expected}")]
    ReportTooShort { expected: usize, actual: usize },
}
