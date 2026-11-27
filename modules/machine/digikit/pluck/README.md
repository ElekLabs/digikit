# Pluck

**Category:** machines  **Devices:** Digitakt II 1.17; Digitone II 1.12  **Status:** experimental (emulator only)  **Kind:** run-time module (`.dkm`)

A new machine in the SRC machine list: a plucked string (Karplus-Strong). On the Digitakt II every track with the
PLUCK machine is one string tuned to the note you play, so it follows the keyboard, the sequencer's notes and the
TUNE knob; the sample is not used. On the Digitone II PLUCK is a machine next to FM TONE, WAVETONE, FM DRUM and
SWARMER: every note (up to the 16 voices) is its own string.

On the **Digitakt II** the SRC page has these knobs (UI 0 to 127):

- **A, TUNE:** fine tuning in the usual SAMPLE way. Default 64 (no offset).
- **B, DECAY:** how long the string rings: about 0.25 s at 32, 1 s at 64, longer above. Default 64.
- **C, PICK:** where the string is plucked: 0 is the end (bright), 63 is the middle (hollow, even harmonics soften). Default 32.
- **E, BRIGHT:** damping of the string: low is a dull thump, high rings with all its harmonics. Default 80.
- **F, COLOR:** softness of the excitation: 0 is a sharp click, high is a soft finger. Default 40.
- **H, LEV:** track level. Default 100.

D and G are empty. The track's amp envelope, filter, FX sends and reverb work on it as on any machine.

On the **Digitone II** PLUCK has one SYN page of its own (the SYN key shows it; there is no FM algorithm and no
operator page). Knobs (UI 0 to 127), same meaning and defaults as above:

- **A, DECAY:** default 64.
- **B, PICK:** default 32.
- **C, BRIGHT:** default 80.
- **D, COLOR:** default 40.

The note pitch follows the keyboard and the sequencer (there is no TUNE knob); the last knob is the track level. The
voice's amplitude envelope (AMP page), filter, overdrive, FX sends and reverb work on it as on the other machines:
hold the key, or set a long amp release, to hear a long DECAY. Each note starts a new string; the same key pressed
again starts another one. FLTR, AMP, FX and MOD pages are the usual ones.

A run-time module: it is not patched into the firmware. The digikit runtime loads the `.dkm` from the card and sends the DSP
code to the DSP. Load it with `mise run dk:run -- digikit/pluck --emu` (or `--port Digitakt`), or pack it with
`mise run module:pack -- digikit/pluck --device dt2` (`dn2` for the Digitone II) and upload `out/patch/pluck-<device>.dkm`. The
image needs the digikit runtime (see docs/PATCHING.md, "Test a run-time module in the emulator"). The machine adds ColdFire
rows for itself and its four parameters (on the Digitone II also its own parameter page) and DSP code.

What a saved project does with a PLUCK track when the module is not loaded: docs/findings/52-sticky-machine-ids.md.

MIDI CC and NRPN do not reach the machine's knobs yet (stock tracks take them through a table the patch does not
extend), and parameter locks were not tried in the emulator. Not tested on hardware.
