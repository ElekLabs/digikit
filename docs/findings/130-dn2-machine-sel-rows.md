# MACHINE SEL on the Digitone II 1.12: rows, dividers and the commit rule

Static read of DN2 OS 1.12 (MAIN OS section, load address `0x40000400`): `tools/coldfire/disasm.py`, `tools/coldfire/refscan.py`
(96.6% byte coverage) and Ghidra 12.2 (`tools/coldfire/ghidra.sh`, `Decompile.java`). Nothing was run, so the marks are [D]. Written for the planned rework of the
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

- **A second mode of the view [O].** With `+0x1a8` = 1, YES compares the cursor's value with `+0x1a0` instead and commits it with
  `0x40031a56(model, track, value, 1, 1)`. What that mode lists is not known. A second selection the stock view already makes
  is worth reading before a sub-list is designed.

- Where `MachineListView` (`0x40059ab0`) is used **[O]**.
- An emulator run with a fourth group, as the mk2 mod did on 1.11, would raise the marks to [V] (after a second check
  against the image bytes).
