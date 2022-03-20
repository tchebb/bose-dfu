For users
=========
bose-dfu is an open-source implementation of Bose's USB update protocol for
certain Bose devices that take updates in `.dfu` format. Using this tool, you
can enter and leave firmware update ("DFU") mode on compatible devices. When in
DFU mode, you can write a device's firmware, including to downgrade it to an
earlier version.

See the next section for a list of devices known to be compatible and the one
after that for instructions on how to find firmware images for your device,
which can tell you if an untested device uses the `.dfu` format.

Device compatibility
--------------------
The only Bose device I own is a SoundLink Color II speaker, and so all initial
development and testing took place against that. However, a quick spot check of
the firmware images for other devices indicates that many of them use the same
image format and so likely speak the same protocol.

I cannot guarantee this, however, so **be aware that you are volunteering to
potentially brick your device if you use this tool on one that has not yet been
tested**. bose-dfu will warn you before proceeding in this case so that you are
aware of the risk. If you successfully use bose-dfu with a device that's not
yet on the list below, please open a pull request to add it to the list.

Tested devices:
 - SoundLink Color II

**Use this tool at your own risk. I will not take responsibility for any damage
bose-dfu does to your device, even if that device is on the list above.**

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
at https://downloads.bose.com/.  Although directory listings are not enabled on
that server, you can get a listing of all supported devices by fetching
https://downloads.bose.com/lookup.xml.

Each `<PRODUCT>` element in that file holds the USB PID of the corresponding
device when it's in DFU mode as well as the URL of an `index.xml` file in a
subdirectory named for the device's codename. Each `index.xml` holds the
filename of its device's latest firmware image(s) in one or more `<IMAGE>`
elements. Those firmware images live alongside the `index.xml` file that refers
to them.

To find firmware for your device, you can run `bose-dfu info` and match its
codename in the "Device model" field against a directory on Bose's server.
Alternatively, you can put it into DFU mode using `bose-dfu enter-dfu`, get its
USB ID using `bose-dfu list`, and match its USB PID (the part of the ID after
the colon) against the elements in `lookup.xml`.

### Via unofficial archive
The [bosefirmware](https://github.com/bosefirmware) GitHub user maintains
repositories archiving old firmwares for various lines of Bose devices. Several
of these repositories contain `.dfu` files, although the [ced][ced] repository
seems to hold the majority of them.

I am not affiliated with this user and do not guarantee the authenticity or
accuracy of the files their repositories contain.

[ced]: https://github.com/bosefirmware/ced

FAQ
---
### Can updating my device's firmware brick it?
Quite possibly. There have been reports online of even the official Bose
updater bricking headphones. That being said, my SoundLink Color II appears to
fall back to DFU mode when its firmware is corrupt, allowing recovery even if
something goes wrong. I have intentionally disconnected its USB cable in the
middle of a firmware download, and I was able to perform a second, successful
download just fine after reconnecting it.

### Can bose-dfu read firmware as well as writing it?
Not out of the box. Although DFU includes an upload request, which is supposed
to read back the exact firmware that was last downloaded, Bose's implementation
of this request on my device returns an image that is not identical and that
cannot successfully be written back to the device. As such, I've intentionally
omitted an `upload` subcommand to prevent confusion. I have left a function
that can perform uploads in `src/protocol.rs`, though: if you want to try it
out, adding the corresponding subcommand is left to you as an exercise.

For developers
==============

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

[btu]: https://btu.bose.com/
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
implements this protocol, which Bose seems to call "USB-DFU" based on strings
in the Electron code. It also includes a custom utility called "dfuhid" that
puts a Bose device into DFU mode, same as bose-dfu's `enter-dfu` subcommand.

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
