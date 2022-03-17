Purpose
=======
bose-dfu is an open-source implementation of Bose's USB update protocol for
their ced line of speakers and headphones. I do not know what "ced" stands for,
but there is a list of such devices in [this][ced] repository (which I am not
affiliated with), so check for yours!

Using this tool, you can enter and leave firmware update mode ("DFU mode") on
any supported device. When in DFU mode, you can read and write a device's
firmware, including to downgrade it to an earlier version.

Device compatibility
====================
The only Bose device I own is a SoundLink Color II speaker, and so all initial
development and testing took place against that. However, a quick spot check of
the firmware images for other ced devices indicates that they all use the same
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
==================
No firmware images are included with this tool, so you'll have to obtain those
yourself. Firmware images end in the extension `.dfu`, and this tool does some
basic verification of images you attempt to write to ensure they are for the
right device and have not mistakenly become corrupt. There are two ways to get
official firmware images that I'm aware of: directly from Bose, and via the
unofficial archive linked above.

Directly from Bose
------------------
Bose hosts the latest firmware (and possibly earlier ones, too) for each device
at https://downloads.bose.com/.  Although directory listings are not enabled on
that server, you can find the latest firmware for your device by accessing
`index.xml` under the subdirectory matching your device's codename. For my
SoundLink Color II (codename "foreman"), for example, I can fetch
https://downloads.bose.com/ced/foreman/index.xml and look for the latest
firmware filename in its `<IMAGE>` element. I can then download that file,
which lives in the same directory as `index.xml`.

Via unofficial archive
----------------------
The [bosefirmware/ced][ced] GitHub repository also contains an easily-browsable
archive of old firmwares for every ced device. Once again, I am not affiliated
with this repository and do not guarantee the authenticity or accuracy of the
files it contains.

[ced]: https://github.com/bosefirmware/ced

Protocol
========
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
==========
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
