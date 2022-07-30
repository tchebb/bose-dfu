For users
=========
bose-dfu is an open-source, command-line firmware update tool for certain Bose
speakers and headphones. Unlike Bose's [official updater][btu], bose-dfu

 - runs on Windows, macOS, Linux, and any other OS supported by Rust and
   [HIDAPI][hidapi]
 - can downgrade firmware as well as upgrade it
 - doesn't rely on a web service to run

Using this tool, you can enter and leave firmware update ("DFU") mode on
compatible devices connected via USB. After putting a device in DFU mode, you
can write new firmware to it.

See the next section for a list of devices known to be compatible and the one
after that for instructions on how to find firmware images for your device
(which can also help you determine compatibility).

[hidapi]: https://github.com/libusb/hidapi
[btu]: https://btu.bose.com/

Tested devices
--------------
**Use this tool at your own risk. I will not take responsibility for damage
bose-dfu does to your device, even if that device is on the following list:**

 - SoundLink Color II (used for initial development)

It's likely that most Bose devices that take updates in `.dfu` format work with
this tool, but I can't guarantee that. If your device isn't on the list above
and you use this tool, **you are volunteering to potentially brick your
device**.  bose-dfu will warn you before it operates on an untested device. If
you successfully use bose-dfu with such a device, please open a pull request to
add it to the list.

Incompatible devices
--------------------
The following devices are known not to work with bose-dfu because they use a
substantially different update protocol:

 - Noise Cancelling Headphones 700 (tchebb/bose-dfu#1)

Obtaining firmware
------------------
No firmware images are included with this tool, so you'll have to obtain those
yourself. Firmware images end in the extension `.dfu`, and this tool does some
basic verification of images you attempt to write to ensure they are for the
right device and have not mistakenly become corrupt. There are two ways to get
official firmware images that I'm aware of: directly from Bose, and via the
unofficial archive linked above.

### Directly from Bose
Bose hosts the latest firmware (and possibly earlier ones, too) for each device
at https://downloads.bose.com/. Although directory listings aren't enabled,
https://downloads.bose.com/lookup.xml lists all devices.

Each `<PRODUCT>` element in `lookup.xml` holds both the USB product ID of that
device when in DFU mode and the URL of an `index.xml` file for the device.
`index.xml` lives in a directory named for the device's codename and holds the
filename(s) of its latest firmware image in one or more `<IMAGE>` elements.
Firmware files live alongside the `index.xml` file that refers to them.

To find firmware for your device, you can run `bose-dfu info` and match the
"Device model" field against directory names on Bose's server. Alternatively,
you can put your device in DFU mode using `bose-dfu enter-dfu`, get its USB ID
using `bose-dfu list`, and match its USB PID (the part of the ID after the
colon) against a `<PRODUCT>` element in `lookup.xml`.

### Via unofficial archive
The [bosefirmware][unofficial-user] GitHub user maintains repositories
archiving old firmwares for various lines of Bose devices. Several of these
repositories, most notably [ced][unofficial-repo], contain `.dfu` files.

I am not affiliated with this user and do not guarantee the authenticity or
accuracy of the files their repositories contain.

[unofficial-user]: https://github.com/bosefirmware
[unofficial-repo]: https://github.com/bosefirmware/ced

Installation
------------
[![Crates.io](https://img.shields.io/crates/v/bose-dfu)](https://crates.io/crates/bose-dfu)

If you already have a Rust toolchain installed on your computer, installing
bose-dfu is as simple as running `cargo install bose-dfu`. To get a Rust
toolchain, you can use [rustup](https://rustup.rs/) or install `rust` using
your system's package manager.

Alternatively, you can find prebuilt binaries for Linux, Windows, and macOS on
the [releases](https://github.com/tchebb/bose-dfu/releases) page.

If you use Linux and encounter permission errors or see `INVALID` in the output
of `bose-dfu list`, you likely need to give your user permission to access Bose
HID devices. You can do this by copying `70-bose-dfu.rules` into
`/etc/udev/rules.d/` and reconnecting the device (no reboot needed). If your
device is untested, it won't have an entry in that file so you'll need to add
one yourself.

Usage
-----
bose-dfu has several subcommands, which are summarized in its help text:

```
bose-dfu 1.0.0
Firmware updater for various Bose devices

USAGE:
    bose-dfu <SUBCOMMAND>

OPTIONS:
    -h, --help       Print help information
    -V, --version    Print version information

SUBCOMMANDS:
    list         List all connected Bose HID devices (vendor ID 0x05a7)
    info         Get information about a specific device not in DFU mode
    enter-dfu    Put a device into DFU mode
    leave-dfu    Take a device out of DFU mode
    download     Write firmware to a device in DFU mode
    file-info    Print metadata about a firmware file, no device needed
    help         Print this message or the help of the given subcommand(s)
```

To update a device, you'll need to run at least `bose-dfu enter-dfu`, `bose-dfu
download`, and `bose-dfu leave-dfu`, in that order. The other subcommands help
you inspect the current state of devices and firmware files. Notable is `info`,
which tells you the current firmware version a device is running.

Subcommands that perform an operation on a device support arguments for
selecting which device to talk to.  You can use `-p` to select by USB product
ID, `-s` to select by USB serial number, or both together. Additionally, the
same subcommands support the `-f`/`--force` flag, which has no effect for
tested devices but is required to perform operations on untested ones.

FAQ
---
### Can updating my device's firmware brick it?
Quite possibly; there have been reports online of even the official Bose
updater bricking headphones. That being said, my SoundLink Color II falls back
to DFU mode when its firmware is corrupt, allowing for easy recovery. I have
not yet managed to brick it while developing this tool, and my attempts have
included intentionally disconnecting its USB cable in the middle of a firmware
download.

### Can bose-dfu read firmware as well as writing it?
Not out of the box. Although USB DFU supports an upload operation, which is
supposed to read back the exact firmware that was last downloaded, Bose's
implementation of it returns an image that's not identical and which can't be
successfully re-downloaded. As such, I've intentionally omitted an `upload`
subcommand to prevent confusion. There is an `upload()` function in
`src/protocol.rs`, though: if you want to use it, adding a corresponding
subcommand is up to you.

For developers
==============
[![docs.rs](https://img.shields.io/docsrs/bose-dfu)](https://docs.rs/bose-dfu/latest/bose_dfu/)

Protocol
--------
The USB protocol implemented herein was derived entirely from USB captures of
Bose's [official firmware updater][btu]. No binary reversing techniques were
used to ascertain or implement the protocol.

Bose's DFU protocol is nearly identical to the [USB DFU protocol][dfu-spec],
except it communicates via [USB HID][hid-spec] reports instead of raw USB
transfers. This one main change, presumably made because communicating with an
HID device doesn't require custom drivers on any major OS, seems to imply all
the other notable changes (e.g. an added header for uploads and downloads to
hold fields that would otherwise be part of the Setup Packet).

As such, I have not written a formal protocol description for Bose DFU. The USB
DFU specification, in combination with this tool's source code and comments
therein, should sufficiently document the protocol.

[dfu-spec]: https://usb.org/sites/default/files/DFU_1.1.pdf
[hid-spec]: https://www.usb.org/sites/default/files/hid1_11.pdf

Prior work
----------
I am not aware of any other third-party implementations of this protocol.
However, Bose appears to have at least two first-party implementations: the
first is the "Bose Updater" website and associated native application available
at https://btu.bose.com/, which is what I took USB captures from in order to
develop bose-dfu. I did not inspect it in any further detail.

The second is the Electron-based "[Bose USB Link Updater][usb-link-updater]",
which bundles and invokes a patched version of [dfu-util][dfu-util] that
implements this protocol (which Bose seems to call "USB-DFU" based on strings
in the Electron code). It also includes a custom utility called "dfuhid" that
puts a device into DFU mode, serving the same purpose as `bose-dfu enter-dfu`.

Notably, I have been unable to find source code for this modified dfu-util.
dfu-util is a GPL application, so Bose is obligated to provide source upon
request. However, based on their license text, I expect they will honor this
obligation only if you mail them a physical letter and pay for them to ship you
the source on physical media. This is more work than I want to do, but I will
happily review the source if someone else goes to the trouble of getting it. It
may well contain useful information that can be used to increase bose-dfu's
reliability or device compatibility.

[usb-link-updater]: https://pro.bose.com/en_us/products/software/conferencing_software/bose-usb-link-updater.html
[dfu-util]: http://dfu-util.sourceforge.net/
