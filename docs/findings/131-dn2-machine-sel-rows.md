# MACHINE SEL on Digitone II 1.12: dividers, selection and page changes

**[D]** Static read of the DN2 1.12 MAIN OS (`0x40000400`) using the ColdFire disassembler, reference scan and
Ghidra. The observations below distinguish the stock chooser from the current digikit hooks. For the SYN and FLTR
row tables and the commit path, see [finding 02](02-machines-and-parameters.md#the-selector-d) and
[finding 33](33-runtime-machines-dn2.md). The [DN2 mk2 mod](https://de-pattern.github.io/dn2mk2/) identified the
same chooser functions on 1.11. The addresses below were read on 1.12.

## Stock behavior

- **Dividers.** The row builder at `0x4005b2c0` skips inputs with type `-1` and calls the group function at
  `0x40059274` for each remaining type. Stock types `0..3` (synths) are group 1, type `4` (MIDI) is group 2,
  and other types are group 0. The builder inserts a value `-1` row when the group changes and the *previous*
  group is nonzero (`0x4005b372`–`0x4005b39c`). The first group therefore has no divider before it. The text
  callback at `0x4005a874` returns an empty string for the divider.
- **Cursor movement.** UP/DOWN in `0x4005b9e8` repeat their step while the row value returned by `0x4005b162`
  is `-1` (`0x4005bb0e`–`0x4005bb3a`). Movement is clamped to the list and does not wrap.
- **YES only commits a different type.** On SYN, the handler compares the highlighted row value with the
  marker at view `+0x19c` (`0x4005bb86`). Equality takes the close path (`0x4005bbee`) without committing.
  Otherwise the handler updates the marker and calls the machine commit function (`0x40031880`). NO closes
  without committing. A module cannot treat reselecting the current machine as a signal to open another view.
- **Page changes can move the cursor away from the marker.** LEFT/RIGHT switch between SYN and FLTR (view
  `+0x1a8`). The handler puts the new page's marked *type value* (`+0x19c` on SYN, `+0x1a0` on FLTR) into
  the cursor's *row index* at `+0x1a4` (`0x4005baa8` and `0x4005bab6`). The stock row order is not the type
  order. For example, FM DRUM has type 2 but is row 1. Returning to SYN with FM DRUM marked highlights
  WAVETONE in row 2 instead.

## Emulator observation

On 2026-11-14 the author ran DN2 1.12 with stock machines in `mise run dk:dev -- emulator --device dn2`. UP/DOWN
skipped the divider between SWARMER and MIDI. YES on the marked machine closed the view without changing type.
YES on a different machine marked it and left the view open. With FM DRUM marked, returning from FLTR to SYN
highlighted WAVETONE. A separate agent checked the static reading on 2026-11-15. These claims remain **[D]**
pending the independent human check required by [CONTRIBUTING.md](../../CONTRIBUTING.md).

## Implications for digikit

The stock group-0 fallback is not what an added machine gets in current digikit. The
[DN2 machines service hook](../../services/machines/dks/src/hooks-dn2.s#L52) maps added types to synth group 1.
The view caches its list, but the [service rebuilds it after reload](../../services/machines/src/machines.rs#L304).
Grouping added machines into separate families would therefore change the group hook. The stock divider has no
label, and the stock chooser has no observed row kind that opens a sub-list.

**[O]** Whether returning to SYN with MIDI marked can put the cursor on the divider, and what YES would do
in that state, has not been checked in the emulator. A third group has not been exercised either.
