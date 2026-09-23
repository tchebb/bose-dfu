# Bose HID report-ID-1 / TAP transition analysis

> **Publication note:** This document records the reverse-engineering stage of
> the work. The handler, selector, boot-mode, and USB-registration conclusions
> below came from static firmware analysis; they should not be read as hardware
> observations. The separately labelled **Subsequent validation** section
> records later USBPcap/Wireshark and physical-device results. The original Bose
> `hidtool.exe` executable was not recovered or disassembled.

## Scope and result

This analysis is limited to the normal USB HID control path, boot-mode selection,
and the resulting HID/CDC registration. No hardware was accessed and no firmware
binary was modified.

The principal result is:

> `hidtool.exe tap` can be reconstructed with high confidence as HID feature
> report ID 1 carrying selector `B0 5E`. The complete report buffer is
> `01 B0 5E`. It requests **boot mode 6**, not boot mode 3. In both M3 builds,
> boot modes 3 and 6 select the same CDC registration path, so mode 6 produces
> the desired TAP/CDC re-enumeration.

## Inputs

| Image | `vm.app` size | SHA-256 |
|---|---:|---|
| M3 1.0.14.6636 captured | 345,922 | `E61B21709838E795B38E2D7E6FBDDC19CA637D0EA1107290A917C92CE0A05113` |
| M3 1.0.14.6631 extracted | 345,292 | `6AAFBFD6F2F4A69A6394698262AF58DE2E3348FC192553243698648A2F6636BA` |
| KCup 1.1.4.3558 extracted | 338,996 | `B7AD5E663E825DBD243A618121B0F1969A31FF5B14EEF90B92FFC46244C8634F` |

The two CED images were extracted with `extract_csr_dfu.py`; the source DFUs
were not changed.

## M3 report-ID-1 protocol

### USB transaction

The normal HID descriptor declares report ID 1 as a two-byte Feature report.
The report buffer seen by the firmware therefore contains three bytes:

```text
byte 0       byte 1       byte 2
report ID    selector MSB selector LSB
01           xx           xx
```

The corresponding USB class request is:

```text
bmRequestType = 0x21        (host-to-device, class, interface)
bRequest      = 0x09        (SET_REPORT)
wValue        = 0x0301      (Feature report, report ID 1)
wIndex        = active HID interface number
wLength       = 3
data          = 01 <selector-MSB> <selector-LSB>
```

On Windows this is the semantic equivalent of a three-byte
`HidD_SetFeature()` buffer. KCup checks `wValue == 0x0301` explicitly. M3
dispatches the SET_REPORT by the low report-ID byte; `0x0301` is nevertheless
the standards-correct value dictated by its descriptor.

### M3 1.0.14.6636 handler

The report-ID-1 routine is at byte address `0x1D066` (`vm.app` XAP word address
`0xE833`). It:

1. Reads `report[1] << 8 | report[2]`.
2. Searches seven selector records at DM `0x9EC2` (file offset `0x4FE50`).
3. Extracts the requested mode from the low nibble of the record's high byte.
4. Calls the common boot-transition routine at byte `0x5BBC` with its secondary
   flag clear.
5. Marks the request context handled (`context[6] = 1`) if the transition
   routine returns either "already in mode" or "scheduled". It leaves the flag
   unchanged on an invalid/failed request.

Every accepted selector is below. Values not in this table are ignored.

| Selector / full report | Requested effect | Persistent/pre-reset detail |
|---|---|---|
| `B0 07` / `01 B0 07` | Boot mode 0 (DFU) | Queues dedicated event `0x60FF` |
| `B0 5E` / `01 B0 5E` | Boot mode 6 (CDC/TAP) | Queues generic boot event `0x6092` with payload `6` |
| `DF 00` / `01 DF 00` | Boot mode 7 | Queues dedicated event `0x6046` |
| `FF FF` / `01 FF FF` | Configured initial mode | Reads one word from PS key `0x03CD`; the retrieved value becomes the requested mode (the captured stage-1 value is `1`) |
| `EF 00` / `01 EF 00` | Boot mode 2 | First ensures PS key `0x0040`, bits 11:8, equals `2`; then queues `0x6092` with payload `2` |
| `B0 5C` / `01 B0 5C` | Boot mode 4 | Queues `0x6092` with payload `4` |
| `19 05` / `01 19 05` | Boot mode 5 | First ensures PS key `0x0040`, bits 11:8, equals `0`, and bits 3:0 equals `1`; then queues `0x6092` with payload `5` |

The transition routine rejects a resolved mode greater than 7. If the resolved
mode already equals `BootGetMode()`, it acknowledges the request without
resetting. Otherwise it performs the mode-2/mode-5 PS-key preparation above,
does common transition housekeeping, and schedules the event after `0x898`
milliseconds (2200 ms). Report ID 1 always passes the flag selecting this
shorter delay; the routine's alternate delay is `0xBB8` (3000 ms).

Its return values are `0` for already in the requested mode, `1` for a queued
transition, and `2` for rejection/failure (including an out-of-range resolved
mode or allocation failure). The report handler treats `0` and `1` as handled.

The generic `0x6092` event consumes its one-word payload and calls
`BootSetMode(payload)` at byte `0x4ADE`. The mode-7 path calls
`BootSetMode(7)` at byte `0x4546`; the mode-0 path is the dedicated DFU event.
`BootSetMode` performs the warm restart. There is no direct USB-reset or
on-the-fly CDC-registration call in the report handler itself.

### M3 CDC selection after restart

M3's boot-dependent USB table is at DM `0xA3FA` (file offset `0x508C0`). The
startup code extracts a USB-class flag for the current boot mode. The relevant
entries are:

| Boot mode | USB-class flag | Result |
|---:|---:|---|
| 1 | `0x01` | HID initializer |
| 2 | `0x01` | HID initializer |
| 3 | `0x02` | CDC initializer |
| 6 | `0x02` | CDC initializer |

Flag `0x01` calls the HID initializer at byte `0x1CDDC`. Flag `0x02` calls the
CDC initializer at byte `0x1BFF0`. The CDC class handler at byte `0x1C1DC`
implements requests `0x20`, `0x21`, `0x22`, and `0x23` (SET/GET_LINE_CODING,
SET_CONTROL_LINE_STATE, and SEND_BREAK).

Consequently, report ID 1 has **no fixed selector for boot mode 3**, but `B0 5E`
does request CDC: it enters mode 6, which uses the same CDC configuration as
mode 3. The `FF FF` indirection could resolve to mode 3 if PS key `0x03CD` were
configured to `3`; in the captured firmware data that key is `1`. USB
disconnect/re-enumeration is a consequence of the `BootSetMode(6)` warm
restart, not a separate `UsbReset` operation in the HID handler.

## M3 1.0.14.6631 comparison

The corresponding report-ID-1 handler is at byte `0x1CFAC`; the common
transition routine remains at `0x5BBC`. Its seven-record selector table is at
file offset `0x4FC2A` and is byte-for-byte identical to 1.0.14.6636, including
the `B0 5E -> mode 6` and `19 05 -> mode 5` records. The boot-dependent USB
configuration table is also identical and maps both modes 3 and 6 to CDC.

Thus there is no protocol or semantic change between 6631 and 6636 relevant to
report ID 1, TAP entry, CDC registration, or the reset mechanism. Differences
in handler address are layout/relocation changes.

## KCup 1.1.4.3558 transition

KCup's HID callback starts at byte `0x15338`; its report-ID-1 SET_REPORT branch
is at `0x15414`. KCup accepts four selectors:

| Selector | KCup effect |
|---|---|
| `B0 07` | After a guard check, queue `0x60FF` for mode 0 / DFU |
| `B0 5E` | Allocate payload `6` and queue `0x6092` |
| `DF 00` | Queue `0x6046` for mode 7 |
| `19 05` | Send application event `0x6436` (`EventAltActivate`); this is not a boot-mode or USB-class change |

KCup does not accept M3's `FF FF`, `EF 00`, or `B0 5C` selectors on report ID 1.
Its `19 05` meaning is also different from M3.

KCup's startup switch at byte `0x59A2` uses the jump table at DM `0x7C96`:

```text
mode:      0    1    2    3    4    5    6
offset:   00   02   14   17   1A   1D   17
```

Mode 2 calls the HID initializer at byte `0x15200`. Modes 3 and 6 share offset
`0x17` and call the CDC initializer at byte `0x14E36`. Therefore KCup's normal
HID-to-TAP transition is also:

```text
01 B0 5E -> queue mode 6 -> BootSetMode(6) -> restart -> CDC
```

KCup additionally contains a report-ID-2/PolyComm boot-mode parser, but it
permits modes 0, 1, 2, 4, 6, and 7 while deliberately routing modes 3 and 5 to
an error. That behavior independently reinforces that mode 6 is the supported
command-driven service/CDC mode even though mode 3 reaches the same initializer
at startup. No literal `hidtool` or PolyComm `tap` command string was found in
the three applications or the locally available Bose updater executables.

## Comparison and reconstruction confidence

The decisive common chain is identical in KCup and both M3 builds:

```text
HID SET_REPORT, Feature ID 1, payload B0 5E
                  |
                  v
             request mode 6
                  |
                  v
        deferred BootSetMode(6)
                  |
                  v
             warm restart
                  |
                  v
       boot-mode-6 USB path -> CDC
```

This also explains why documentation can describe TAP activation without
mentioning boot mode 6: the user-visible result is the TAP CDC/COM interface,
while the internal selector is a boot-mode transition.

The host transaction can therefore be reconstructed with **high confidence**
as the control transfer shown above, or equivalently the feature-report buffer
`01 B0 5E`. What has not been independently recovered is the original
`hidtool.exe` implementation itself (argument parsing, device enumeration, and
retry/wait behavior). Those host-side details do not affect the on-wire request
accepted by the firmware.

This result matches the two relevant service workflows. The
[Tera Term setup instructions](https://documents.cdn.ifixit.com/sXIuOwuGwUJmIht5.pdf)
say to run `hidtool.exe tap` before selecting the Bose TAP Interface COM port.
The [Mini II service manual](https://documents.cdn.ifixit.com/PqsaQ4VEqKIsZH1m.pdf)
instead presents PolyComm's **TAP Mode** button while the device is connected as
HID. Neither document gives the HID bytes; the shared `B0 5E` firmware path
supplies that missing link.

## Subsequent validation

After the static analysis was complete, a USBPcap capture examined in
Wireshark confirmed that the implemented host request is a Feature
`SET_REPORT`, report ID 1, `wValue = 0x0301`, length 3, with payload
`01 B0 5E`. The capture also showed the subsequent USB re-enumeration.

Physical tests then confirmed the resulting `05A7:40FE` HID to `05A7:40FF`
CDC transition on two speakers:

- a healthy original SoundLink Mini II Cup/KCup-family unit running
  1.1.4.3558, where Windows exposed a USB Serial Device COM port and the
  service console returned data; and
- a SoundLink Mini II Special Edition/M3 running Main 1.0.14.6636, where the
  same CDC console exposed working battery and system diagnostics.

These observations validated the firmware-derived prediction on both firmware
families. The original service-kit `hidtool.exe` binary and its surrounding
host-side implementation remain unavailable; the accepted protocol and its
observable result were independently reproduced.
