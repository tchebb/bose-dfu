# SoundLink Mini II Special Edition / M3 — Battery Imbalance Diagnostic Example

This document records one real diagnostic case using the CDC/TAP service interface recovered and implemented in this fork.

It is intended as an example of how the service commands can be used to distinguish a genuine battery fault from firmware corruption, charging-board failure, or an apparently dead battery.

This is **one diagnosed unit**, not a claim that all SoundLink Mini II Special Edition / M3 red-battery-LED failures have the same cause.

## Device

* Model family: SoundLink Mini II Special Edition / M3
* Firmware: `Main 1.0.14.6636`
* Normal USB PID: `05A7:40FE`
* TAP/CDC USB PID: `05A7:40FF`

## Initial symptom

The speaker showed a flashing red battery LED and would not charge normally.

The Bose firmware updater recognised the speaker and a forced reinstall of the current firmware completed successfully, but the fault remained.

USB input current was approximately 20 mA, indicating that the logic electronics were powered but the battery was not accepting meaningful charge.

Physical voltage measurements at the four battery-wire solder points on the Bose power board initially showed 0 V across every combination.

At this stage the main possibilities included:

* a completely discharged or failed battery pack;
* internal battery protection/BMS shutdown;
* broken battery interconnects;
* charging/power-board failure;
* firmware or battery-state corruption.

The CDC/TAP service interface allowed these possibilities to be investigated without immediately opening the sealed battery pack.

## Entering the service interface

With the speaker operating normally as USB HID PID `40FE`:

```text
bose-dfu enter-tap
```

The speaker warm-restarted and re-enumerated as:

```text
05A7:40FF
```

Windows exposed the interface as a USB serial COM port.

The TAP console was opened at:

```text
115200 baud
8 data bits
no parity
1 stop bit
```

## Initial live battery readings

The first battery-voltage queries returned:

```text
> vb
vbat: 0

> vb 5
vcell1: 0

> vb 6
vcell2: 0
```

Battery state-of-charge also returned zero:

```text
> ba 4
chargelevel: 0
```

Battery fault status returned:

```text
> ba 6
faultstatus: 0x0000
```

These results initially looked consistent with a dead or electrically disconnected battery.

However, the system-fault log told a different story.

## Reading the stored system fault

The system fault state was queried before clearing anything:

```text
> sf
state: 00000001
```

The least-significant bit was set, indicating stored fatal fault information.

The associated fatal fault was then queried:

```text
> sf 1,0
fault: 1
```

Using the Bose troubleshooting documentation, fatal fault `1` corresponds to:

```text
FAULT_BATTERY_PALLADIUM_CELL_IMBALANCE
```

The temperature and battery voltage recorded when the fault occurred were also still available:

```text
> sf 1,1
C: 29

> sf 1,2
mV: 7610
```

The speaker had therefore seen a battery voltage of approximately **7.61 V at 29 °C when the fatal fault was registered**.

This was important evidence.

The battery had not simply been a completely dead 0 V pack when the failure occurred.

## Clearing the recorded fault for diagnostic purposes

At this point the fault information had been recorded.

Bose service documentation describes clearing the stored fault, resetting the speaker, re-entering TAP mode and reading the cell voltages immediately before the fault is registered again.

`sf 0` destroys the stored fault information and should therefore **not** be used until the existing `sf` and `sf 1,x` data have been recorded.

The fault was cleared:

```text
> sf 0
OK
```

The speaker was then soft-reset:

```text
> sr
```

After the normal HID interface returned, TAP/CDC mode was immediately entered again:

```text
bose-dfu enter-tap
```

## Immediate post-reset battery readings

The battery values were queried immediately after TAP mode became available:

```text
> vb
vbat: 7568

> vb 5
vcell1: 4079

> vb 6
vcell2: 3489
```

These values are internally consistent:

```text
Cell 1:       4079 mV
Cell 2:       3489 mV
              -------
Pack total:   7568 mV
```

The measured cell difference was:

```text
4079 - 3489 = 590 mV
```

The battery was therefore alive, but the two series cells were severely out of balance.

Bose troubleshooting documentation treats cell differences above approximately 50 mV as abnormal and identifies approximately 300 mV as the threshold for the fatal cell-imbalance condition.

This pack showed approximately:

```text
590 mV imbalance
```

or nearly twice the documented fatal threshold.

## Fault re-latching

Shortly after the measurements were taken, the red battery LED returned.

The system fault state was queried again:

```text
> sf
state: 00000001
```

The fatal fault had re-latched.

The complete observed sequence was therefore:

```text
Stored fatal CELL_IMBALANCE fault
        |
        v
Battery communication/readings unavailable
vb = 0
cell1 = 0
cell2 = 0
        |
        v
Record stored fault information
        |
        v
sf 0
clear stored fault
        |
        v
sr
soft reset
        |
        v
enter-tap
        |
        v
Battery temporarily readable
        |
        +--> pack  = 7568 mV
        +--> cell1 = 4079 mV
        +--> cell2 = 3489 mV
        |
        v
590 mV cell imbalance detected
        |
        v
Fatal fault re-latches
        |
        v
Red battery LED returns
```

## Conclusion

The original assumption that the battery was simply at 0 V was incorrect.

The zero readings were a consequence of the latched battery/system fault state preventing normal battery communication or measurement.

Once the stored fault was cleared and the unit reset, the pack became readable long enough to reveal the actual failure:

**Cell 1: 4.079 V**

**Cell 2: 3.489 V**

**Difference: 590 mV**

The stored `CELL_IMBALANCE` fault was therefore consistent with the live measurements and immediately reappeared once the firmware evaluated the battery again.

For this unit, the evidence pointed to a genuine internal battery-pack cell imbalance rather than:

* corrupted main firmware;
* a completely dead battery;
* a broken connection between the battery and Bose power board;
* failure of the main board to measure battery voltage;
* a simple software state that could be permanently cleared.

A replacement battery pack was therefore the appropriate repair path.

## Why this example matters

Without the CDC/TAP service interface, the observable symptoms were misleading.

The external measurements and normal firmware state suggested an electrically dead battery. The stored service data instead showed that the speaker had previously measured a normal 7.61 V pack and had deliberately disabled normal battery operation after detecting a fatal condition.

The recovered service interface made it possible to:

1. preserve and decode the stored fault;
2. retrieve the voltage and temperature recorded when the fault occurred;
3. temporarily restore battery communication using Bose's documented diagnostic procedure;
4. measure both cells independently;
5. confirm the fault against live hardware measurements;
6. observe the same fatal fault re-latch afterward.

This is also a useful example of why fault data should be collected **before** issuing `sf 0`.

## Commands used

Read-only diagnostic commands used in this case:

```text
sf
sf 1,0
sf 1,1
sf 1,2

vb
vb 5
vb 6

ba 4
ba 6
```

State-changing commands used only after the original fault information had been recorded:

```text
sf 0
sr
```

Host command used to enter the CDC/TAP service interface:

```text
bose-dfu enter-tap
```

`sf 0` should not be treated as a routine first troubleshooting step because it erases stored fault evidence.

Other TAP commands may also change device state. In particular, `sh` enters ship mode and should not be used as a diagnostic query.

## Related documentation

See also:

* `../TAP-SERVICE-MODE.md` — how the HID-to-CDC/TAP transition was reconstructed and verified.
* `../SOUNDLINK-MINI-II-DIAGNOSTICS.md` — general diagnostic workflow and command reference.
* `../research/hid-report1-tap-analysis.md` — detailed firmware analysis behind the TAP-mode transition.
