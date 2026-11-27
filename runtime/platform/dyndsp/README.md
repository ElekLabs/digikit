# Run-time DSP machines (core/dyndsp)

Reserves the DSP side of machines that the digikit runtime loads at run time: eight free voice rows in the render hook's registry
and a table of DSP addresses (the area, the rows, the helper routines a run-time DSP object may call). It changes nothing by itself:
a free row has a type no machine has, so the DSP renders every track as before. Needs `core/runtime`, `core/dsp-link` and
`core/dsp-voice` (added automatically). Digitakt II 1.17 and Digitone II 1.12, emulator only.

How it works and its limits: `docs/design/digikit-dkm.md` ("DSP section"), `docs/findings/32-runtime-dsp-machines.md`. Test modules:
`digikit/pluck` (Digitakt II), `test/sine` (both devices).
