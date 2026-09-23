# SoundLink Mini II CDC/TAP diagnostics

This guide is intended for technically competent repairers working on a Bose
SoundLink Mini II. It describes a conservative, observation-first workflow. It
does not cover forcing charge current, bypassing protection, modifying battery
packs, or other unsafe battery manipulation.

TAP commands are firmware-specific. Even commands that look like simple
queries may change state if used with different arguments. Record the original
output before making any change.

## Enter the CDC/TAP interface

1. Connect the speaker by USB while it is in its normal HID mode. Verified
   Mini II units enumerate as `05A7:40FE` in this state.
2. Run:

   ```text
   bose-dfu enter-tap
   ```

   If multiple devices match, select the intended unit with `-s` or `-p`.
   `--force` retains its normal bose-dfu meaning for an untested or
   ambiguous-mode device.
3. The request schedules a warm restart after approximately 2.2 seconds. Wait
   for Windows to finish removing the HID device and enumerating `05A7:40FF`.
4. Open **Device Manager**, expand **Ports (COM & LPT)**, and identify the new
   **USB Serial Device (COMn)**. Note its COM-port number.
5. Open that COM port in a serial terminal with:

   ```text
   Baud:         115200
   Data bits:    8
   Parity:       None
   Stop bits:    1
   Flow control: None
   ```

Press Enter if necessary to obtain the TAP prompt.

`enter-tap` only sends the mode request. It does not discover or open the COM
port and does not send a TAP command.

## Record faults before clearing anything

> **Important:** Record the complete output of `sf` and the applicable
> `sf 1,x` queries before ever using `sf 0`. The command `sf 0` clears stored
> system-fault evidence. Once cleared, information needed to understand the
> original failure may be lost.

`sf` is the system-fault bitfield, not a locale command. On M3 1.0.14.6636,
`lc` returned the locale `en-us`; on the Cup/KCup 1.1.4.3558 CDC/TAP console,
the same command returned `?ERR`. Do not generalize command availability or
semantics across device families, and do not describe or use `lc` as an
error-clearing command.

Fatal battery faults can also cause `vb` and cell-voltage readings to return
zero while the fault is latched. Zero readings in that state do not by
themselves prove that the pack or every cell is at zero volts. Preserve the
fault evidence first, then interpret voltage readings in the context of the
fault state and the applicable service procedure.

## Useful read-only diagnostic queries

The following commands were used successfully on the verified Mini II
firmware. The descriptions intentionally stay close to observed behavior;
field availability and formatting may differ by firmware.

| Command | Diagnostic use |
|---|---|
| `sn 0` | Read the unit's serial-number information. |
| `sf` | Read the current/stored system-fault bitfield summary. Save the complete output. |
| `sf 1,x` | Read the stored fatal-fault details; the second argument selects the field. In the verified M3 workflow, `0` returned the fault code, `1` the recorded temperature, and `2` the recorded battery voltage. Save these fields before clearing faults. |
| `vb` | Read the battery-voltage summary. A fatal latched battery fault may suppress the reading to zero. |
| `vb 5` | Read the observed cell-1 voltage field on the verified firmware. |
| `vb 6` | Read the observed cell-2 voltage field on the verified firmware. |
| `ba 4` | Read battery state of charge. This meaning is documented in Bose troubleshooting material and was confirmed on hardware. |
| `ba 6` | Read battery fault status. This meaning is documented in Bose troubleshooting material and was confirmed on hardware. |

These examples are queries as shown. Do not generalize that every TAP command
or every alternate argument is harmless. In particular, plain `sh` enters ship
mode and is state-changing.

## Diagnostic example: one M3 unit with cell imbalance

On one diagnosed SoundLink Mini II Special Edition/M3 unit, a latched fatal
fault made `vb`, `vb 5`, and `vb 6` all report 0 mV. The stored evidence instead
recorded `CELL_IMBALANCE`, 29 C, and 7610 mV. After that evidence was captured
and the documented clear/reset/re-enter-TAP sequence was performed, the live
readings showed 7568 mV total, 4079 mV for cell 1, and 3489 mV for cell 2: a
590 mV difference. The fault then re-latched and the red battery LED returned.

This result is limited to the diagnosed unit. It illustrates why stored faults
should be recorded before `sf 0`, and why zero live voltage readings can be
misleading while a fatal fault is latched.

**Full command transcript and diagnostic evidence:** [M3 Battery Imbalance Diagnostic](examples/M3-BATTERY-IMBALANCE-DIAGNOSTIC.md)

Do not use this example as authorization to force charging, bypass battery
protection, balance cells manually, or continue operating a suspect pack.
Follow appropriate battery-safety and product-service procedures.

## Return to normal mode

On the hardware-tested Cup/KCup 1.1.4.3558 unit, the verified return from
CDC/TAP mode was to issue `sh`, wait for `OK`, disconnect USB power, and then
reconnect USB power. The speaker then returned to its normal `05A7:40FE` HID
interface. The use of `sh` here is intentional, but it remains a state-changing
command that enters ship mode. This procedure is supported by the Bose Mini II
service manual and has not yet been hardware-verified on M3. A direct
software-only return from boot mode 6 has not yet been established.

See [Leaving TAP service mode](TAP-SERVICE-MODE.md#leaving-tap-service-mode)
for the complete verified sequence.

## Related documentation

- [HID-to-CDC TAP service mode](TAP-SERVICE-MODE.md)
- [Firmware report-ID-1 analysis](research/hid-report1-tap-analysis.md)
