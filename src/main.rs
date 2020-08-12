use anyhow::Result;
use structopt::StructOpt;
use thiserror::Error;

const BOSE_VID: u16 = 0x05a7;

const SUPPORTED_NONDFU_PIDS: &[u16] = &[
    0x40fe, // Bose Color II SoundLink
];

const SUPPORTED_DFU_PIDS: &[u16] = &[
    0x400d, // Bose Color II SoundLink
];

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
}

#[derive(Error, Debug)]
enum MatchError {
    #[error("no devices match specification")]
    NoDevices,

    #[error("multiple devices match specification")]
    MultipleDevices,
}

#[derive(StructOpt, Debug)]
struct DeviceSpec {
    /// Serial number
    #[structopt(short)]
    serial: Option<String>,

    /// Product ID (vendor ID is always matched against Bose's, 0x05a7)
    #[structopt(short)]
    pid: Option<u16>,
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
                    Ok(dev.open_device(hidapi)?)
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let mode = Opt::from_args();

    let api = hidapi::HidApi::new().expect("couldn't open HIDAPI");

    match mode {
        Opt::List => list(&api),
        Opt::EnterDfu { ref spec } => enter_dfu(&spec.get_device(&api)?)?,
        Opt::LeaveDfu { ref spec } => leave_dfu(&spec.get_device(&api)?)?,
    };

    Ok(())
}

fn list(hidapi: &hidapi::HidApi) {
    let all_spec = DeviceSpec {
        serial: None,
        pid: None,
    };
    for dev in hidapi.device_list().filter(|d| all_spec.matches(d)) {
        let support_status = match dev.product_id() {
            v if SUPPORTED_NONDFU_PIDS.contains(&v) => "not in DFU mode, supported device",
            v if SUPPORTED_DFU_PIDS.contains(&v) => "in DFU mode, supported device",
            _ => "unsupported device, proceed at your own risk",
        };

        println!(
            "{} {} [{}]",
            dev.serial_number().unwrap_or("INVALID"),
            dev.product_string().unwrap_or("INVALID"),
            support_status,
        );
    }
}

fn enter_dfu(device: &hidapi::HidDevice) -> Result<()> {
    device
        .send_feature_report(&[1, 0xb0, 0x07])
        .map_err(Into::into)
}

fn leave_dfu(device: &hidapi::HidDevice) -> Result<()> {
    device.send_feature_report(&[3, 0xff]).map_err(Into::into)
}
