use anyhow::{bail, Context, Result};
use bose_dfu::device_ids::{identify_device, DeviceCompat, DeviceMode, UsbId};
use bose_dfu::dfu_file::parse as parse_dfu_file;
use bose_dfu::protocol::{download, ensure_idle, enter_dfu, leave_dfu, read_info_field};
use clap::Parser;
use hidapi::{DeviceInfo, HidApi, HidDevice};
use log::{info, warn};
use std::io::Read;
use std::path::Path;
use thiserror::Error;

#[derive(Parser, Debug)]
#[clap(version, about, setting = clap::AppSettings::DeriveDisplayOrder)]
enum Opt {
    /// List all connected Bose HID devices (vendor ID 0x05a7)
    List,

    /// Get information about a specific device not in DFU mode
    Info {
        #[clap(flatten)]
        spec: DeviceSpec,
    },

    /// Put a device into DFU mode
    EnterDfu {
        #[clap(flatten)]
        spec: DeviceSpec,
    },

    /// Take a device out of DFU mode
    LeaveDfu {
        #[clap(flatten)]
        spec: DeviceSpec,
    },

    /// Write firmware to a device in DFU mode
    Download {
        #[clap(flatten)]
        spec: DeviceSpec,

        #[clap(parse(from_os_str))]
        file: std::path::PathBuf,
    },

    /// Print metadata about a firmware file, no device needed
    FileInfo {
        #[clap(parse(from_os_str))]
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

fn parse_pid(src: &str) -> Result<u16, std::num::ParseIntError> {
    u16::from_str_radix(src, 16)
}

#[derive(Parser, Debug)]
struct DeviceSpec {
    /// USB serial number
    #[clap(short)]
    serial: Option<String>,

    /// USB product ID as an unprefixed hex string (only matches vendor ID 05a7)
    #[clap(short, parse(try_from_str = parse_pid))]
    pid: Option<u16>,

    /// Proceed with operation even if device is untested or might be in wrong mode.
    #[clap(short, long)]
    force: bool,

    /// DFU/normal mode (determined using product ID for known devices)
    #[clap(skip)]
    required_mode: Option<DeviceMode>,
}

#[derive(Copy, Clone, Debug)]
struct DeviceRisks {
    /// The device has not been tested, and bose-dfu might brick it.
    untested: bool,
    /// The running command needs a specific mode, and we're not sure the device is in that mode.
    ambiguous_mode: bool,
}

impl DeviceSpec {
    /// If we match the given device, return a [DeviceRisks] with details on the match. Otherwise,
    /// return [None].
    fn match_dev(&self, device: &DeviceInfo) -> Option<DeviceRisks> {
        let dev_id = UsbId {
            vid: device.vendor_id(),
            pid: device.product_id(),
        };

        let (untested, mode) = match identify_device(dev_id) {
            DeviceCompat::Compatible(mode) => (false, mode),
            DeviceCompat::Untested(mode) => (true, mode),
            DeviceCompat::Incompatible => return None,
        };

        let ambiguous_mode = match self.required_mode {
            None => false,
            Some(_) if mode == DeviceMode::Unknown => true,
            Some(req_mode) if mode == req_mode => false,
            _ => return None,
        };

        if let Some(x) = self.pid {
            if dev_id.pid != x {
                return None;
            }
        }

        if let Some(ref x) = self.serial {
            if device.serial_number() != Some(x) {
                return None;
            }
        }

        Some(DeviceRisks {
            untested,
            ambiguous_mode,
        })
    }

    fn get_device<'a>(&self, hidapi: &'a HidApi) -> Result<(HidDevice, &'a DeviceInfo)> {
        let mut candidates = hidapi
            .device_list()
            .filter_map(|d| (self.match_dev(d).map(|r| (d, r))));

        match candidates.next() {
            None => Err(MatchError::NoDevices.into()),
            Some((dev, risks)) => {
                if candidates.next().is_some() {
                    return Err(MatchError::MultipleDevices.into());
                }

                if risks.untested {
                    warn!("Device has NOT BEEN TESTED with bose-dfu; by proceeding, you risk damaging it");
                }

                if risks.ambiguous_mode {
                    warn!(
                        "Cannot determine device's mode; command may damage devices not in {} mode",
                        self.required_mode.unwrap()
                    );
                }

                if (risks.untested || risks.ambiguous_mode) && !self.force {
                    bail!("to use an untested or ambiguous-mode device, you must pass -f");
                }

                dev.open_device(hidapi)
                    .map(|open| (open, dev))
                    .context("failed to open device; do you have permission?")
            }
        }
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::new()
            .filter_or("BOSE_DFU_LOG", "info")
            .write_style("BOSE_DFU_LOG_STYLE"),
    )
    .format_timestamp(None)
    .init();

    let mode = Opt::parse();

    let api = HidApi::new()?;

    match mode {
        Opt::List => list_cmd(&api),
        Opt::Info { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Normal),
                ..spec
            };
            let (dev, info) = spec.get_device(&api)?;

            use bose_dfu::protocol::InfoField::*;
            println!("USB serial: {}", info.serial_number().unwrap_or("INVALID"));
            println!("HW serial: {}", read_info_field(&dev, SerialNumber)?);
            println!("Device model: {}", read_info_field(&dev, DeviceModel)?);
            println!(
                "Current firmware: {}",
                read_info_field(&dev, CurrentFirmware)?
            );
        }
        Opt::EnterDfu { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Normal),
                ..spec
            };
            enter_dfu(&spec.get_device(&api)?.0)?;
            info!("Note that device may take a few seconds to change mode");
        }
        Opt::LeaveDfu { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Dfu),
                ..spec
            };
            leave_dfu(&spec.get_device(&api)?.0)?;
        }
        Opt::Download { spec, file } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Dfu),
                ..spec
            };
            let (dev, info) = spec.get_device(&api)?;
            download_cmd(&dev, info, &file)?
        }
        Opt::FileInfo { file: path } => {
            let mut file = std::fs::File::open(path)?;
            let suffix = parse_dfu_file(&mut file)?;

            println!(
                "For USB ID: {:04x}:{:04x}",
                suffix.vendor_id, suffix.product_id
            );
            match suffix.has_valid_crc() {
                true => println!("CRC: valid ({:#010x})", suffix.expected_crc),
                false => println!(
                    "CRC: INVALID (expected {:#010x}, actual {:#010x}",
                    suffix.expected_crc, suffix.actual_crc
                ),
            }
        }
    };

    Ok(())
}

fn list_cmd(hidapi: &HidApi) {
    for dev in hidapi.device_list() {
        let dev_id = UsbId {
            vid: dev.vendor_id(),
            pid: dev.product_id(),
        };

        let state = identify_device(dev_id);
        if let DeviceCompat::Incompatible = state {
            continue;
        }

        println!(
            "{} {} {} [{}]",
            dev_id,
            dev.serial_number().unwrap_or("INVALID"),
            dev.product_string().unwrap_or("INVALID"),
            state,
        );
    }
}

fn download_cmd(dev: &HidDevice, info: &DeviceInfo, path: &Path) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let suffix = parse_dfu_file(&mut file)?;
    suffix.ensure_valid_crc()?;

    let (vid, pid) = (info.vendor_id(), info.product_id());
    if !suffix.vendor_id.matches(vid) || !suffix.product_id.matches(pid) {
        bail!(
            "this file is not for the selected device: file for {:04x}:{:04x}, device is {:04x}:{:04x}",
            suffix.vendor_id, suffix.product_id, vid, pid
        );
    }

    if suffix.vendor_id.0.is_none() || suffix.product_id.0.is_none() {
        warn!(
            "DFU file's USB ID ({:04x}:{:04x}) is incomplete; can't guarantee it's for this device",
            suffix.vendor_id, suffix.product_id
        );
        // TODO: Require a "force" flag to proceed?
    }

    info!("Update verified to be for selected device");

    ensure_idle(dev)?;

    info!("Beginning firmware download; it may take several minutes; do not unplug device");
    download(dev, &mut file.by_ref().take(suffix.payload_length))?;

    Ok(())
}
