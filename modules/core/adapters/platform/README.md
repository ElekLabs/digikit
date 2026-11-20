# core/platform

The L1 adapter under the runtime's `Platform` interface (docs/ARCHITECTURE.md, R3). It owns the firmware hooks that the digikit runtime used to install
itself, and the exception hook install:

- `cf.view.hook.ui_loop` (`dkrt_loop_hook`): the UI loop's event pop runs the runtime's `dkrt_tick` once per pass.
- `cf.rtos.hook.main_os_task` (`dkrt_boot_hook`): the entry of the main-OS task runs `dkrt_boot`, which loads the selected modules before the task builds its
  singletons (finding 37; finding 128: it must exist before the main OS starts, so it stays patched in the image).
- `cf.midi.hook.rx` (`dkrt_midi_hook`): a gathered USB SysEx message goes to `dkrt_midi_rx` first (finding 43).
- `dkplat_exc_install`: points the firmware's exception hook word at `dkrt_exc_hook`, which saves the registers and calls `dkrt_exc` (finding 46). The runtime
  calls it every UI pass through `Platform::install_exception_hook`.

The runtime requires this module and the two link by name; the trampolines are the code that was in `core/runtime`'s `tramp.s`, unchanged. The machine page
hooks stay in the runtime (finding 128). Tests: the runtime's host tests (`FakePlatform`) and `modules/core/runtime/tests/platform-contract.device.test.ts`.
