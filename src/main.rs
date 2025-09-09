use anyhow::{Context, Result, bail};
use clap::Parser;
use hidapi::{DeviceInfo, HidApi, HidDevice};
use log::{info, warn};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

use bose_dfu::device_ids::{DeviceCompat, DeviceMode, UsbId, identify_device};
use bose_dfu::dfu_file::parse as parse_dfu_file;
use bose_dfu::protocol::{
    download, ensure_idle, enter_dfu, leave_dfu, read_info_field, run_tap_command,
};

#[derive(Parser, Debug)]
#[command(version, about)]
enum Opt {
    /// List all connected Bose HID devices (vendor ID 0x05a7)
    List,

    /// Get information about a specific device not in DFU mode
    Info {
        #[command(flatten)]
        spec: DeviceSpec,
    },

    /// Run TAP commands on a specific device not in DFU mode
    Tap {
        #[command(flatten)]
        spec: DeviceSpec,
    },

    /// Put a device into DFU mode
    EnterDfu {
        #[command(flatten)]
        spec: DeviceSpec,
    },

    /// Take a device out of DFU mode
    LeaveDfu {
        #[command(flatten)]
        spec: DeviceSpec,
    },

    /// Write firmware to a device in DFU mode
    Download {
        #[command(flatten)]
        spec: DeviceSpec,

        file: std::path::PathBuf,

        #[arg(short, long)]
        wildcard_fw: bool,
    },

    /// Print metadata about a firmware file, no device needed
    FileInfo { file: std::path::PathBuf },
}

#[derive(Parser, Debug)]
struct DeviceSpec {
    /// USB serial number
    #[arg(short)]
    serial: Option<String>,

    /// USB product ID as an unprefixed hex string (only matches vendor ID 05a7)
    #[arg(short, value_parser = parse_pid)]
    pid: Option<u16>,

    /// Proceed with operation even if device is untested or might be in wrong mode
    #[arg(short, long)]
    force: bool,

    /// Required device mode (derived automatically from chosen subcommand)
    #[arg(skip)]
    required_mode: Option<DeviceMode>,
}

fn parse_pid(src: &str) -> Result<u16, std::num::ParseIntError> {
    u16::from_str_radix(src, 16)
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
        Opt::Tap { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Normal),
                ..spec
            };
            tap_command_loop(&spec.get_device(&api)?.0)?;
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
            let (dev, _) = spec.get_device(&api)?;
            ensure_idle(&dev)?;
            leave_dfu(&dev)?;
        }
        Opt::Download {
            spec,
            file,
            wildcard_fw,
        } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Dfu),
                ..spec
            };
            let (dev, info) = spec.get_device(&api)?;
            download_cmd(&dev, info, &file, wildcard_fw)?
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
                    "CRC: INVALID (expected {:#010x}, actual {:#010x})",
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

        let state = identify_device(dev_id, dev.usage_page());
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

fn tap_command_loop(device: &HidDevice) -> Result<()> {
    let mut rl = DefaultEditor::new()?;

    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                } else if line == "." {
                    break;
                }
                rl.add_history_entry(line.as_str())?;

                let result = run_tap_command(device, line.as_bytes());
                println!("{result:?}");
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {err:?}");
                break;
            }
        }
    }

    Ok(())
}

fn download_cmd(dev: &HidDevice, info: &DeviceInfo, path: &Path, wildcard_fw: bool) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let suffix = parse_dfu_file(&mut file)?;
    suffix.ensure_valid_crc()?;

    let dev_id = UsbId {
        vid: info.vendor_id(),
        pid: info.product_id(),
    };

    if !suffix.vendor_id.matches(dev_id.vid) || !suffix.product_id.matches(dev_id.pid) {
        bail!(
            "this file is not for the selected device: file for {:04x}:{:04x}, device is {}",
            suffix.vendor_id,
            suffix.product_id,
            dev_id
        );
    }

    if suffix.vendor_id.0.is_none() || suffix.product_id.0.is_none() {
        warn!(
            "Update's USB ID ({:04x}:{:04x}) is incomplete; can't guarantee it's for this device",
            suffix.vendor_id, suffix.product_id,
        );

        if !wildcard_fw {
            bail!("to write firmware with an incomplete USB ID, you must pass -w");
        }
    } else {
        info!("Update verified to be for selected device");
    }

    ensure_idle(dev)?;

    info!("Beginning firmware download; it may take several minutes; do not unplug device");
    download(dev, &mut file.by_ref().take(suffix.payload_length))?;

    Ok(())
}

impl DeviceSpec {
    /// If we match the given device, return a [DeviceRisks] with details on the match. Otherwise,
    /// return [None].
    fn match_dev(&self, device: &DeviceInfo) -> Option<DeviceRisks> {
        let dev_id = UsbId {
            vid: device.vendor_id(),
            pid: device.product_id(),
        };

        let (untested, mode) = match identify_device(dev_id, device.usage_page()) {
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

        if let Some(x) = self.pid
            && dev_id.pid != x
        {
            return None;
        }

        if let Some(ref x) = self.serial
            && device.serial_number() != Some(x)
        {
            return None;
        }

        Some(DeviceRisks {
            untested,
            ambiguous_mode,
        })
    }

    fn get_device<'a>(&self, hidapi: &'a HidApi) -> Result<(HidDevice, &'a DeviceInfo)> {
        let mut candidates = hidapi
            .device_list()
            .filter_map(|d| self.match_dev(d).map(|r| (d, r)));

        match candidates.next() {
            None => Err(MatchError::NoDevices.into()),
            Some((dev, risks)) => {
                if candidates.next().is_some() {
                    return Err(MatchError::MultipleDevices.into());
                }

                if risks.untested {
                    warn!(
                        "Device has not been tested with bose-dfu; by proceeding, you risk damaging it"
                    );
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

#[derive(Copy, Clone, Debug)]
struct DeviceRisks {
    /// The device has not been tested, and bose-dfu might brick it.
    untested: bool,
    /// The running command needs a specific mode, and we're not sure the device is in that mode.
    ambiguous_mode: bool,
}

#[derive(Error, Debug)]
enum MatchError {
    #[error("no devices match specification")]
    NoDevices,

    #[error("multiple devices match specification")]
    MultipleDevices,
}
