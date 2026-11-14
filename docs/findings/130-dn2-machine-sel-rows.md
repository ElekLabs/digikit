# MACHINE SEL on the Digitone II 1.12: rows, dividers and the commit rule

Static read of DN2 OS 1.12 (MAIN OS section, load address `0x40000400`): `tools/coldfire/disasm.py`, `tools/coldfire/refscan.py`
(96.6% byte coverage) and Ghidra 12.2 (`tools/coldfire/ghidra.sh`, `Decompile.java`), marked [D]; the visible behaviour was then
run in the emulator (section "Emulator run"). Written for the planned rework of the
machine list (more run-time machines, groups or sub-lists).

Prior art: the DN2 mk2 mod read the same functions on DN2 1.11 (de-pattern.github.io/dn2mk2, `docs/machine-list.md`, AGPL-3.0; facts
cited, no code copied). **Every address below is the same on 1.12**, checked here instruction by instruction.

## Result

**[D] The rows.** The row builder `0x4005b2c0` belongs to the view the firmware names `Machine Selection` (its constructor
`0x4005b56e` passes that string; the builder's one caller is `0x4005b6de` in it). The view is built at boot and keeps its rows
([digikit-registry](../design/digikit-registry.md)). The builder gets the machine list from the getter `0x4004d8b6`
(the call at `0x4005b33c`; core/dynreg hooks the getter's copy, so run-time machines are in this list). For each entry it:

1. skips an entry of `-1` (`0x4005b35a`-`0x4005b360`);
2. calls the group function `0x4005b364: jsr 0x40059274` with the type;
3. when the group differs from the previous entry's and the previous group is not 0, adds a row of value `-1` first
   (`0x4005b372`-`0x4005b39c`). That row is the dotted divider. It has no text.

The rows go into a vector of 8-byte entries at view `+0x184`. The previous group starts at 0 and follows every change, so no
divider is drawn before the first group.

**[D] The group function** `0x40059274` (34 bytes): type < 0 -> 0; 0..3 -> 1 (the synths); 4 -> 2 (MIDI); anything else -> 0.
Callers: `0x4005b364` (the row builder) and `0x40059cdc`, in the constructor `0x40059ab0` of a second view the firmware names
`MachineListView`, which builds its list the same way (the same skip of `-1`, the same group compare). No direct call of
`0x40059ab0` was found (a table or a computed call; not traced). Where that view is used is **[O]**. The groups are only compared for equality, so a new group needs no other change to draw its
own divider. A run-time machine (type 16 and up) is in group 0 today, so the stock list draws a divider between MIDI and the
first run-time machine and none between run-time machines.

**[D] The cursor skips dividers.** The cursor movement in `0x4005b9e8` repeats its step while `0x4005b162` (the value of the row
under the cursor) is `-1`, so UP and DOWN never stop on a divider.

**[D] The marker and the commit rule.** The view keeps the type it marks, and puts the cursor on it, at `+0x19c` (412). Four
stores set it, each right after a call of `0x4003134e(model, track)` ("which machine does track t have"): `0x4005b6c8` (the
constructor) and `0x4005a5cc`, `0x4005a638`, `0x4005a9ce` (the mk2 mod reads these as after a commit and on model changes; not re-read here). `0x4003134e` ends in a tail
`jmp 0x4004b7f2` at `0x40031394` (the track's machine type, `sound+0xDE`). On YES (`0x4005bb86`):

```
4005bb80  jsr     0x4005b162(pc)     ; the type under the cursor
4005bb86  cmp.l   $19c(a2), d0
4005bb8a  beq.b   0x4005bbee         ; the same type: nothing is committed
4005bb8c  move.l  d0, $19c(a2)       ; else mark it and commit
```

In C (Ghidra, `0x4005b9e8`): with the view's mode word `+0x1a8` = 0, the value under the cursor (`0x4005b162`) is compared with
`+0x19c`; when it differs, it is stored there and committed with `0x40031880(model, track, type, 1, 1)`, and `+0x1a0` is set from
`0x400313a6(model, track)`. When it is the same, the view calls its vtable entry `+0x28` (read as: the view closes; not run).
So choosing the machine a track already has commits nothing. A module cannot use "the user picked my machine again" as a signal
to open its own chooser.

## Keys

**[D]** The view's key handler is `0x4005b9e8`. By key code (`0x401162f8(event)`): `0x0c` NO closes the view (vtable `+0x28`);
`0x0b` and `0x0e` are UP and DOWN; `0x0d` and `0x0f` are LEFT and RIGHT; `0x0a` and `0x31` run the YES path above; codes 1 to 6
(the page keys) close the view. Everything else goes to the base handler `0x4011c954`. NO has no other use in this view, so a
sub-list can take "NO goes up one level" without displacing anything.

## Two pages: SYN and FLTR

**[D] static, [V] run.** The view has two lists and a page word at `+0x1a8`: 0 is the machine list (rows at `+0x184`, the page
drawn as `< SYN >`), 1 is the filter type list (rows at `+0x190`, drawn as `< FLTR >`: MULTI-MODE, LOWPASS 4, LEGACY LP/HP, COMB-,
COMB+, EQUALIZER). LEFT and RIGHT switch the page (`0x4005ba8a`-`0x4005ba9c`). Every function of the view branches on the word:
the row count (`0x4005a92c`), the value under the cursor (`0x4005b162`), the draw (`0x4005b7d4`: the page's bitmap, its label,
its list and two page dots, the current one larger), and YES, which on page 1 compares with `+0x1a0` and commits with
`0x40031a56(model, track, value, 1, 1)`. On a page change the cursor row `+0x1a4` is set from the new page's marker *value*
(`+0x19c` or `+0x1a0`, at `0x4005baa8` and `0x4005bab6`), used as a row number. That is the marked row only where value and row
agree: with FM DRUM marked (type 2, second row), LEFT from FLTR puts the cursor on the third row (WAVETONE).

So the stock view already shows several lists as pages of one view. A group of run-time machines could be a third page
(its own label and dot), with the machine page's code path: the smallest "folder" this view can show.

## Emulator run

**[V] (run here; the byte check by a second agent is pending).** `mise run dk:dev -- emulator --device dn2` with a key script
(FUNC+SRC, DOWN and UP steps, YES), DN2 1.12, stock machines, 2026-11-14. Screenshots in `out/runs/devreload/dn2/` (not committed):

- FUNC+SRC opens `MACHINE SEL > TRACK 1` with the cursor on the track's machine (FM TONE, marked by the filled dot). The rows are
  FM TONE, FM DRUM, WAVETONE, SWARMER, a dotted divider, MIDI: the stock order, one divider where the group changes from 1 to 2.
- DOWN from SWARMER lands on MIDI, and UP from MIDI on SWARMER: the cursor never stops on the divider.
- YES on the marked machine closes the view; the track keeps FM TONE.
- After DOWN to FM DRUM, YES commits it: the dot moves to FM DRUM and the view stays open. A second YES closes it, and the
  track shows FM DRUM's page.
- RIGHT shows `< FLTR >` with the six filter types and the second dot larger; LEFT returns to `< SYN >`. With FM DRUM
  marked, the cursor came back on WAVETONE (the marker value 2 used as a row number, above).

## What it means for more run-time machines

- **Groups:** a divider per family (for example one per module, or per collection) is a change to the group function only; a
  type range per group fits in its 34 bytes or in a hook. The divider has no label, so a family needs its name in the
  machines' names (or a labelled divider row, which the builder does not have).
- **Sub-lists:** the builder makes one flat list of rows; there is no row kind that opens another list **[O]**. A sub-list
  needs either a new row kind and key handling in this view, or a replacement view.
- **Re-selection:** YES on the current machine is a no-op (above). A flow that chooses a variant after the machine (for
  example "pick MD, then which MD machine") has to be reached some other way when the track already has that machine.
- **Built once:** the rows are built once at boot, so a module loaded later (hot reload) is not listed until the view is
  rebuilt (the archived `dynreg_view_hook`, [digikit-hot-reload](../plans/digikit-hot-reload.md)).

## Open

- Where `MachineListView` (`0x40059ab0`) is used **[O]**.
- A run with a third group (a changed group function) to show its divider **[O]**.
- The second agent's check of the addresses against the image bytes (pending).
