Device compatibility
====================
The only Bose device I own is a SoundLink Color II speaker, and so all initial
development and testing took place against that. However, a quick spot check of
the firmware images for other Bose "ced" devices, archived in [this][ced]
excellent repository, indicates that they all use the same image format and so
likely can be updated using the same protocol. bose-dfu will warn you if you
attempt to update a device it doesn't recognize but will not prevent you from
doing so. If you successfully use bose-dfu with a device that's not yet on the
list below, please open a pull request to add it.

Tested devices:
 - SoundLink Color II

**Use this tool at your own risk. I will not take responsibility for any damage
bose-dfu does to your device, even if that device is on the list above.**

[ced]: https://github.com/bosefirmware/ced

Protocol
========
The USB protocol implemented herein was derived entirely from USB captures of
Bose's [official firmware updater][btu]. No binary reversing
techniques were used to specify or implement the protocol.

Bose's DFU protocol is nearly identical to the [USB DFU protocol][dfu-spec],
except it communicates via [USB HID][hid-spec] reports instead of raw USB
transfers. This one main change, presumably made because communicating with an
HID device doesn't require custom drivers on any major OS, seems to imply all
the other notable changes (e.g. an added header for uploads and downloads to
hold fields that would otherwise be part of the Setup Packet).

As such, I have not written a formal protocol description for Bose DFU. The USB
DFU specification, in combination with this tool's source code (and comments
therein), should sufficiently document the protocol.

[btu]: https://btu.bose.com/
[dfu-spec]: https://usb.org/sites/default/files/DFU_1.1.pdf
[hid-spec]: https://www.usb.org/sites/default/files/hid1_11.pdf

Prior work
==========
I am not aware of any other third-party implementations of this protocol.
However, Bose appears to have at least two first-party implementations: the
first is the "Bose Updater" website and associated native application available
at https://btu.bose.com/, which is what I took USB captures from in order to
develop bose-dfu. I did not inspect it in any further detail.

The second is the Electron-based "[Bose USB Link Updater][usb-link-updater]",
which bundles and invokes a patched version of [dfu-util][dfu-util] that
implements this protocol, which Bose seems to call "USB-DFU" based on strings in
the Electron code. It also includes a custom utility called "dfuhid" that puts a
Bose device into DFU mode, same as bose-dfu's `enter-dfu` subcommand.

Notably, I have been unable to find source code for the modified dfu-util.
dfu-util is a GPL application, and so Bose is obligated to provide source upon
request. However, based on their license text, I expect they will honor this
obligation only if you mail them a physical letter and pay for them to ship you
a copy of the source on physical media. This is more work than I want to do,
but I will happily review the source if someone else goes to the trouble of
getting it. It may well contain useful information that can be used to increase
bose-dfu's reliability or device compatibility.

[usb-link-updater]: https://pro.bose.com/en_us/products/software/conferencing_software/bose-usb-link-updater.html
[dfu-util]: http://dfu-util.sourceforge.net/
