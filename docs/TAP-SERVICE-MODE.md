# Bose HID-to-CDC TAP service mode

## Purpose

The original SoundLink Mini II Cup/KCup family and the SoundLink Mini II
Special Edition/M3 family verified in this work normally enumerate as a
vendor-specific HID device. The `enter-tap` command asks that normal firmware
to restart in boot mode 6,
where it exposes the more capable CDC/TAP serial service interface.

This is distinct from `bose-dfu tap`:

- `tap` keeps the device in normal HID mode and exchanges TAP commands through
  HID report ID 2.
- `enter-tap` sends one report-ID-1 mode-switch request and exits. The device
  subsequently restarts and exposes a serial interface; the command does not
  wait for, open, or communicate with that interface.

## Motivation

The immediate motivating case was a SoundLink Mini II Special Edition/M3 with
a red battery fault. The upstream HID `tap` command could communicate with the
speaker's limited report-ID-2 command interface, but important service commands
such as `sf`, `vb`, `ba`, and related battery diagnostics were not accessible
through that M3 path.

Bose service and troubleshooting material described a different repair
workflow: PolyComm's **TAP Mode** button, or the later `hidtool.exe tap`, made a
serial-style Bose TAP interface available. The purpose of this work was to
understand that transition, reproduce it with open-source tooling, and restore
access to the richer service diagnostics. Although a battery repair prompted
the investigation, the result is a general HID-to-CDC service-mode operation
for the verified Mini II families.

## Investigation method and evidence

The mechanism was established by cross-validating independent sources rather
than relying on a single firmware observation:

| Evidence source | What it established |
|---|---|
| Bose SoundLink Mini II service manuals and troubleshooting material | Authorised repair workflows used a distinct TAP Mode and serial/TAP console, separate from normal HID commands. Later instructions specifically invoked `hidtool.exe tap`. |
| SoundLink Mini II SE/M3 schematic | Provided hardware context while tracing boot/service paths and interpreting battery and charging diagnostics; it was a cross-reference, not the source of the HID request bytes. |
| Firmware analysis | Captured M3 1.0.14.6636 was compared with M3 1.0.14.6631 and KCup 1.1.4.3558. The common report-ID-1 selector `B0 5E` was found to request boot mode 6, schedule a roughly 2.2-second deferred `BootSetMode(6)`, and select CDC after the warm restart. |
| USBPcap/Wireshark capture | Confirmed the actual outgoing Feature `SET_REPORT`: report ID 1, `wValue = 0x0301`, length 3, data `01 B0 5E`, followed by USB re-enumeration. |
| Two physical speakers | Confirmed the predicted `05A7:40FE` HID to `05A7:40FF` CDC transition and a working service console on both Cup/KCup and M3 hardware. |

In other words, firmware analysis predicted the mechanism; Bose documentation
explained the intended service workflow; USB capture confirmed the exact host
transaction; and two real speakers confirmed the resulting behavior.

## Usage

With a supported device connected in normal HID mode:

```text
bose-dfu enter-tap
```

The normal `-s`, `-p`, and `--force` device-selection options are available.
For example, when more than one compatible device is present:

```text
bose-dfu enter-tap -s SERIAL_NUMBER
```

On successful submission, bose-dfu prints:

```text
TAP mode request sent. Device should re-enumerate as a CDC/TAP interface.
```

This confirms only that the HID feature report was submitted successfully. It
does not confirm that the restart or CDC enumeration completed.

## Protocol

In normal mode, the verified SoundLink Mini II devices use USB ID
`05A7:40FE`. `enter-tap` submits exactly one logical HID Feature report:

```text
01 B0 5E
```

The request has the following USB HID meaning:

| Field | Value |
|---|---|
| Transfer | HID `SET_REPORT` |
| Report type | Feature |
| Report ID | `1` |
| `wValue` | `0x0301` |
| Logical report length | 3 bytes |
| Data | `01 B0 5E` |

The first byte remains the HID report ID. bose-dfu passes the three-byte
logical buffer to hidapi and leaves any platform-specific Feature Report
length handling to hidapi.

Firmware interprets selector `B0 5E` as a request for boot mode 6. It schedules
the transition after approximately 2.2 seconds and calls `BootSetMode(6)`,
which performs a warm restart. On the verified firmware, boot mode 6 registers
the CDC/TAP interface. The devices then re-enumerate as USB ID `05A7:40FF`.

On Windows, the CDC interface typically appears under **Ports (COM & LPT)** as
a **USB Serial Device (COMn)**. A console connection using 115200 baud, 8 data
bits, no parity, and 1 stop bit (115200 8N1; no flow control) was used
successfully during verification.

## Hardware verification

| Device family | Firmware | Observed result |
|---|---|---|
| Original SoundLink Mini II, Cup/KCup family | 1.1.4.3558 | `05A7:40FE` HID changed to `05A7:40FF`; Windows attached a USB Serial Device COM port; the CDC console responded at 115200 8N1 |
| SoundLink Mini II Special Edition, M3 | Main 1.0.14.6636 | Same `40FE` to `40FF` transition; the CDC console and diagnostic commands worked |

A USBPcap capture examined in Wireshark on the Cup/KCup-family unit confirmed a Feature
`SET_REPORT`, report ID 1, `wValue = 0x0301`, length 3, with payload
`01 B0 5E`.

## Leaving TAP service mode

Entering boot mode 6 is not undone by simply pressing the normal power button.
On the hardware-tested original SoundLink Mini II Cup/KCup unit running
firmware 1.1.4.3558, the verified return path is:

1. At the CDC/TAP console, enter:

   ```text
   sh
   ```

2. Wait for the device to respond:

   ```text
   OK
   ```

3. Disconnect USB power.
4. Reconnect USB power. The device wakes from ship mode and returns to the
   normal HID interface at `05A7:40FE`, where bose-dfu identifies it as a
   compatible device in normal mode.

The `sh` command is state-changing: its intentional purpose here is to put the
speaker into ship mode. This sequence matches the Bose Mini II service manual,
which documents `sh` as entering ship mode and reconnecting power as waking the
unit for normal operation, and it was verified on Cup/KCup hardware.

This exit sequence has not yet been hardware-verified on M3. A cleaner direct
software-only return from boot mode 6 has also not yet been established.

## Limitations and safety

The original Bose `hidtool.exe` executable was not recovered or disassembled.
The firmware-accepted request and its resulting transition were independently
reconstructed, compared across KCup 1.1.4.3558 and M3 1.0.14.6631/6636, and
then hardware-verified.

Entering service mode changes the active boot mode and deliberately restarts
the device. Console commands are product- and firmware-specific, and some are
state-changing. Do not assume that an unfamiliar TAP command is read-only.
For a conservative Mini II diagnostic workflow, see
[SoundLink Mini II diagnostics](SOUNDLINK-MINI-II-DIAGNOSTICS.md). The static
firmware analysis is preserved in
[HID report-ID-1 TAP analysis](research/hid-report1-tap-analysis.md).
