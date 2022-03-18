use byteorder::{ByteOrder, BE};
use log::warn;
use std::fmt::{Display, LowerHex, Write};
use std::io::{Read, Seek, SeekFrom};
use thiserror::Error;

/// A 16-bit ID that may be unset. Has functions for pretty-printing and wildcard matching.
#[derive(Debug)]
pub struct OptionalId(pub Option<u16>);

impl OptionalId {
    pub fn matches(&self, cmp: u16) -> bool {
        match self.0 {
            None => true,
            Some(id) => id == cmp,
        }
    }

    fn fmt_helper<F>(&self, f: &mut std::fmt::Formatter, delegate: F) -> std::fmt::Result
    where
        F: FnOnce(&u16, &mut std::fmt::Formatter) -> std::fmt::Result,
    {
        match self.0 {
            Some(id) => delegate(&id, f),
            None => {
                for _ in 0..f.width().unwrap_or(3) {
                    f.write_char('?')?
                }
                Ok(())
            }
        }
    }
}

impl Display for OptionalId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.fmt_helper(f, Display::fmt)
    }
}

impl LowerHex for OptionalId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.fmt_helper(f, LowerHex::fmt)
    }
}

/// Convert from an ID field in a DFU suffix.
impl From<u16> for OptionalId {
    fn from(val: u16) -> Self {
        OptionalId(match val {
            0xffff => None,
            i => Some(i),
        })
    }
}

/// Metadata about a file containing a DFU suffix.
#[derive(Debug)]
pub struct SuffixInfo {
    pub vendor_id: OptionalId,
    pub product_id: OptionalId,
    pub release_number: OptionalId,
    pub expected_crc: u32,
    pub actual_crc: u32,
    pub payload_length: u64,
}

impl SuffixInfo {
    pub fn has_valid_crc(&self) -> bool {
        self.actual_crc == self.expected_crc
    }

    pub fn ensure_valid_crc(&self) -> Result<(), SuffixError> {
        match self.has_valid_crc() {
            true => Ok(()),
            false => Err(SuffixError::BadCRC {
                expected: self.expected_crc,
                actual: self.actual_crc,
            }),
        }
    }
}

/// Parse errors for a DFU suffix.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SuffixError {
    #[error("DFU signature is not present; are you sure this is a DFU file?")]
    BadSignature,

    #[error(
        "DFU specification version is too old: expected at least {}.{}, got {}.{}",
        .minimum >> 8, .minimum & 0xff,
        .actual >> 8, .actual & 0xff,
    )]
    TooOld { minimum: u16, actual: u16 },

    #[error("file is shorter than DFU suffix: expected at least {minimum} bytes")]
    FileTooShort { minimum: u64 },

    #[error("DFU suffix is shorter than allowed: expected at least {minimum} bytes, got {actual}")]
    SuffixTooShort { minimum: u8, actual: u8 },

    #[error("DFU suffix is longer than file: suffix is {suffix_len} bytes, file is {file_len}")]
    SuffixTooLong { suffix_len: u8, file_len: u64 },

    #[error("bad CRC32 checksum: expected {expected:#010x}, got {actual:#010x}")]
    BadCRC { expected: u32, actual: u32 },
}

/// All errors (parse and I/O) that can happen while reading a DFU file.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid firmware file")]
    SuffixError(#[from] SuffixError),

    #[error("I/O error")]
    IoError(#[from] std::io::Error),
}

/// Compute the CRC used by USB DFU 1.1 over all bytes in the given file. Does not strip CRC field
/// from suffix automatically.
fn compute_crc(file: &mut impl Read) -> std::io::Result<u32> {
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = [0u8; 4096];
    loop {
        let len = file.read(&mut buf)?;
        if len == 0 {
            break;
        }
        hasher.update(&buf[0..len]);
    }
    Ok(!hasher.finalize()) // DFU's CRC algorithm is a bitwise NOT of IEEE's.
}

/// Parse the suffix of a DFU file and calculate the data's real checksum, storing the results in a
/// [SuffixInfo] struct. When this returns, `file`'s cursor is at the beginning of the payload.
pub fn parse(file: &mut (impl Read + Seek)) -> Result<SuffixInfo, Error> {
    const MIN_SUFFIX_LEN: u8 = 0x10;
    const MIN_DFU_BCD: u16 = 0x0100;

    let file_len = file.seek(SeekFrom::End(0))?;
    if file_len < MIN_SUFFIX_LEN as _ {
        return Err(SuffixError::FileTooShort {
            minimum: MIN_SUFFIX_LEN as u64,
        }
        .into());
    }

    let mut suffix = [0u8; MIN_SUFFIX_LEN as usize];
    file.seek(SeekFrom::End(-(MIN_SUFFIX_LEN as i64)))?;
    file.read_exact(&mut suffix)?;
    suffix.reverse(); // Entire suffix is byte-swapped

    if &suffix[5..=7] != b"DFU" {
        return Err(SuffixError::BadSignature.into());
    }

    let suffix_len = suffix[4];
    #[allow(clippy::comparison_chain)]
    if suffix_len < MIN_SUFFIX_LEN {
        return Err(SuffixError::SuffixTooShort {
            minimum: MIN_SUFFIX_LEN as _,
            actual: suffix_len,
        }
        .into());
    } else if suffix_len > MIN_SUFFIX_LEN {
        warn!(
            "Got {} extra bytes in DFU suffix; continuing",
            suffix_len - MIN_SUFFIX_LEN
        );
    }

    let payload_length = match file_len.checked_sub(suffix_len as _) {
        Some(i) => i,
        None => {
            return Err(SuffixError::SuffixTooLong {
                suffix_len,
                file_len,
            }
            .into())
        }
    };

    let bcd_dfu = BE::read_u16(&suffix[8..10]);
    if bcd_dfu < MIN_DFU_BCD {
        return Err(SuffixError::TooOld {
            minimum: MIN_DFU_BCD,
            actual: bcd_dfu,
        }
        .into());
    }

    // CRC is over all but the last 4 bytes, which hold the expected CRC.
    file.seek(SeekFrom::Start(0))?;
    let actual_crc = compute_crc(&mut file.take(file_len - 4))?;
    let expected_crc = BE::read_u32(&suffix[0..4]);

    // Reset cursor so caller can read the file's data.
    file.seek(SeekFrom::Start(0))?;

    Ok(SuffixInfo {
        vendor_id: BE::read_u16(&suffix[10..12]).into(),
        product_id: BE::read_u16(&suffix[12..14]).into(),
        release_number: BE::read_u16(&suffix[14..16]).into(),
        expected_crc,
        actual_crc,
        payload_length,
    })
}
