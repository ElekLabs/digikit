# Boot splash

ui/boot-splash shows your own 128x64 picture at power-on for 1.5 seconds instead of the stock intro animation. The
default is the owner's art (`art/abigor-128x64.png`) with the inverted polarity: dark screen, the figure lit.

- Devices and firmware: Digitakt II 1.17 only (`ports = ["dt2-1.17"]`).
- What changes: the five stock intro variants all draw the splash, 45 frames at the intro's 30 Hz. Nothing else.
  After the intro the firmware runs as stock (same loading screen, project load and UI).
- Boot time: the app task waits for the intro before it loads the project, so a 45-frame intro makes the boot shorter
  than stock (in the emulator the main UI comes up at about 259 M instructions instead of 831 M).
- Limits: no animation; one static picture. With the console boot flag the intro (and so the splash) is skipped, as
  in stock.

## How it works

The stock intro task picks one of five scene lists at random and calls each list's draw function once per frame with
`(frame, nframes, Bitmap *)`, then flushes the bitmap to the OLED itself. The module only rewrites data: in the
80-byte scene-list block (anchor `cf.intro.scenes`, a `cf_words` rule on the stock frame counts 175, 180, 180, 180,
170) every list's draw-function word points at `splash_draw` and every frame count becomes 45. The intro task, its
timer, the flushes and the semaphore the app task waits on stay stock.

`splash_draw` (`src/cf/splash.s`, in `cf.expansion`) checks that the Bitmap is the stock 128x64 one (width 128,
height 64, 2 words per column, non-NULL storage) and copies the 1024-byte `splash_bits` table into it; otherwise it
returns without writing. No firmware calls. The calling convention and the Bitmap layout are read from the stock
bytes; see the header of `src/cf/splash.s`.

Image layout: column-major, byte `8x + k` holds Bitmap rows `8k .. 8k+7` of column x, MSB first, a set bit lit; on the
glass Bitmap row 0 is the **bottom** row. `tools/patchkit/pk_splash.py` does the flip, so PNG row 0 is the top of the
screen.

## Swapping the art or the polarity

Any 128x64 PNG works (grey levels are thresholded at 128; convert and threshold larger art first). Then:

    uv run python -m tools.patchkit.pk_splash modules/ui/boot-splash/art/MY-ART.png \
        -o modules/ui/boot-splash/src/cf/splash_bits.s --polarity inverted --preview out/splash-preview.png
    mise run patch:build -- modules/ui/boot-splash

- `--polarity inverted` (default): the PNG's dark pixels are lit (for black-on-white art: dark screen, figure lit).
- `--polarity as-drawn`: the PNG's white pixels are lit.
- `--preview` writes what the screen will show (x4).
- `uv run python -m tools.patchkit.pk_splash --check modules/ui/boot-splash/src/cf/splash_bits.s` fails when the
  table is stale. `tests/patchkit/test_boot_splash.py` checks the same and pins the default polarity (update that
  test when the owner switches it).

The duration is the `0x2d` (45) in the five `hex` strings of `module.toml`'s `[[op]]`s (frames at 30 Hz).

## Images

    uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx \
        --modules ui/boot-splash -o out/patch/splash-only-1.17.syx
    uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx \
        --modules ui/boot-splash,machines/plaits-48k -o out/patch/plaits-5models-names-splash-48k-1.17.syx

Owner checks before flashing (as for the PLAITS images): `elektron-firmware-tool -i X` reports `checksums : ok`;
`uv run python -m tools.sharc_dev.sharc_bootcheck --syx X --stock firmware/Digitakt_II_OS1.17.syx` reports 0 errors;
`uv run python -m tools.patchkit.pk_encoding_gate X X.plan.json` reports 0 violations; sections 2, 4, 5 and 8 are
byte-identical to stock (and section 7 too for `splash-only`).

## Owner checklist

1. Flash `splash-only-1.17.syx` first (it changes only the MAIN OS: the scene-list data and the module code).
2. Power on: the splash shows for about 1.5 s (dark screen, figure lit, upright), then the stock loading screen and
   the UI. If it is upside down, mirrored or garbled the boot still completes; report it.
3. Create a new project after flashing (every flash).
4. Then the combined image, `plaits-5models-names-splash-48k-1.17.syx`, with the PLAITS checklist
   (`modules/machines/plaits/HARDWARE-TEST.md`).

Recovery (any image that does not boot): hold FUNC at power-on, press TRIG 4, then send the stock
`firmware/Digitakt_II_OS1.17.syx` with Elektron Transfer's legacy mode over MIDI DIN. The bootloader is never
touched by patchkit.

The art is the owner's own (from their avatar). Keep this file free of firmware bytes and vendor tool names.
