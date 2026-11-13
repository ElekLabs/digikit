# digikit SDK

The SDK is the stable interface between content modules and the digikit runtime.
It does not depend on the device. Device differences live in the `core/*` adapters
and the device profiles (`devices/*.toml`).

## Layout

| Path | What | Edit |
| --- | --- | --- |
| `sdk.toml` | The contract spec: the runtime version, the imports with their `service`, `since` and `context`, machine and effect layouts, log kinds, protocol numbers, limits, settings. | yes |
| `include/dk/core/` | The module contract: `<dk/core/module.h>` (`dk_module_def`, `dk_module_ctx` and the prototypes of every import), `<dk/core/log.h>`, `<dk/core/mem.h>`, `<dk/core/time.h>` (the runtime services), `<dk/core/trace.h>` (trace marks), `<dk/core/bytes.h>` (big-endian 32-bit access, header only), `<dk/core/text.h>` (bounded text: length, copy, equal, append, concatenate, number text, truncation, size and time text), `<dk/core/path.h>` (join, parent, leaf) and `<dk/core/status.h>` (the `DK_E_*` status codes; generated, do not edit). No device access. | `module.h`, `log.h`, `mem.h`, `time.h`, `trace.h`, `bytes.h`, `text.h`, `path.h` yes; `status.h` no |
| `include/dk/platform/` | What the device provides, one header per service group: `ui.h` (menus, lists and dialogs on the screen), `param.h` (read and write a machine parameter from a `dk_param_on_click` callback), `settings.h`, `files.h` (file listing and writing), `io.h`, `job.h` (background jobs), `hash32.h`, `usb_bulk.h`, `intro.h`, `data.h`. | yes |
| `include/dk/page/` | Pages: `page.h` (the frame and event of a canvas page, the page flags) and `widgets.h` (small page widgets). | yes |
| `include/dk/chooser/` | The slot list chooser (`chooser.h`) and its pure functions of row sort order and pool row text (`logic.h`). | yes |
| `include/dk/sample/` | Sample components: `pool.h` (per-module stereo sample pool), `view.h` (waveform drawing for native pages) and `picker.h` (on-device sample picker for a 16-entry pool). | yes |
| `include/dk/gen/` | C layouts and constants generated from `sdk.toml` (including `imports.h`, the prototypes of the service imports, and `sample.h`, the word layout of a sample pool entry). Included by the headers above, not by modules. | no |
| `src/` | Compiled SDK sources, in the folders of `include/dk/`: `chooser/chooser.c`, `chooser/logic.c`, `core/text.c`, `core/path.c`, `sample/picker.c`. The packer builds them into one SDK library and links the functions a module calls (`--gc-sections` drops the rest); a module does not list them in its `module.toml`. | yes |
| `dsp/` | SHARC+ code by concept (`osc/`, `filter/`, `dynamics/`, `delay/`, `util/`, `sample/`). See [dsp/README.md](dsp/README.md). `dsp/contract/` is on the SHARC+ include path and holds the call convention (`call.inc`), the includes generated from `sdk.toml` (`dk_fx.inc`, `dk_machine.inc`, `dk_sample_pool.inc`) and the hand-written `dk_trace.inc` (see Tracing). | yes |
| `templates/` | Module templates for `mise run module:new`: `machine/`, `fx/`, `tool/`. | yes |
| `tests/` | Host C tests, in the folders of `include/dk/` (`sdk:test-host`). | yes |
| `../modules/core/runtime/vectors/` | Fit and settings expected results shared by the Rust, Python and web tests. They belong to the runtime, not to the SDK. | no |

### Where a new thing goes

Include direction: `core` <- `platform` <- components (`page`, `machine`, `fx`, `chooser`, `sample`). A component may include
another component, without cycles.

- `core/` is the module contract: entry point, status, log, small helpers. No device access.
- `platform/` is the only layer that declares imports. A component never declares an import: it calls `platform/`.
- A new concept is a new component folder in `include/dk/`, with `src/<concept>/` for compiled code, `tests/<concept>/` for its host
  test and `dsp/<concept>/` for SHARC+ code. If it needs the device, its imports go in `platform/<name>.h`.
- Generated files go in `include/dk/gen/`, `dsp/contract/` and `include/dk/core/status.h` only.
- Names: functions `dk_<area>_<verb>`, types `dk_<thing>`, constants `DK_<AREA>_<NAME>`.

`patchkit gen` writes every generated file. `patchkit gen --check` fails when one is stale
(`mise run gen:modules -- --check`).

`pnpm nx:pilot run sdk:test-host` compiles the machine and effect fixture
declarations with the host `cc` compiler and checks their public-header records:
machine parameter units, effect parameter values, DSP symbols and routing.
It needs no firmware or DSP toolchain. Each shipped module still owns its device
conformance checks; this fast SDK check does not replace them.

## The entry point and the services

A module of the new contract defines one constant, `dk_module`, and calls the device through named imports grouped by service. There is no
Api table and no `dkm_*` symbol: the old entry (`dkm_init(api)`, the slots, `dkm_menu`) is gone (docs/plans/sdk.md section 5). `include/dk/core/module.h` has `dk_module_def` and `dk_module_ctx`; the prototypes of the
imports are generated into `include/dk/gen/imports.h` from the `[[import]]` entries of `sdk.toml` that have a `service`.

```c
#include <dk/core/module.h>
#include <dk/core/log.h>
#include <dk/platform/settings.h>

static dk_settings *settings;

static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_settings_open(ctx, &settings);   /* a handle per service, opened in init */
    if (r != DK_OK)
        return r;
    dk_log(DK_LOG_INFO, "ready", 0);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};  /* init, fini, tick, menu: all optional */
```

- `tick(ctx)` runs once per UI pass for every loaded module. `menu(ctx, menu)` adds the rows of the module's SETTINGS... menu.
- The type is `dk_module_def` and the constant is `dk_module` (the packer looks the symbol up). `ctx` is only the module context; the user-data pointer of a callback is `arg`, and callback types end in `_fn` (`dk_io_step_fn`). The naming rules are in docs/plans/sdk.md section 10.
- Each import has a `service` (`runtime`, `samples`, `machines`, `fx`, `ui`, `input`, `usb_bulk`, `dsp_link`), a `since` (the runtime version
  that added it, "0.7.0") and a `context` (`init`, `ui`, `io-step`, `any`, `isr`). The generated header repeats the last two in the comment of each
  prototype. `patchkit pack` writes the newest `since` a module links as the runtime version it needs and prints the services it needs
  (`services <file>: ...`).
- A service that another module provides (`ui`, `input`, `usb_bulk`, `dsp_link`) may be absent: its `_open` returns `DK_E_MISSING`, the other
  calls return the same code or do nothing, and the module still loads. Without `dk_ui_open` returning `DK_OK` a module does not call
  the functions that need the `dk_ui` handle (`dk_ui_menu_new`, `dk_ui_confirm`, `dk_ui_choice`, `dk_ui_toast`, `dk_ui_tick`).
- The headers of the services are `core/log.h`, `core/mem.h`, `core/time.h`, `platform/host.h`, `settings.h`, `files.h` (listing and writing),
  `io.h`, `hash32.h`, `intro.h`, `param.h`, `samples.h`, `ui.h`, `input.h`, `usb_bulk.h`, `dsp_link.h` and `debug.h`.

A `.dkm` that needs another runtime major or minor, or a higher patch, than the runtime is refused with `E_CONTRACT`; the packer refuses an object that exports `dkm_init`, `dkm_fini`, `dkm_menu` or `dkm_slot_<n>`.

## Include paths

The build passes these search paths, in this order:

- ColdFire C and assembly: the module's `build/gen/`, the module's `src/`, `modules/core/fw/inc`, then `sdk/include`.
- SHARC+ assembly: the module's `build/gen/`, the module's `src/`, `sdk/dsp/contract`, `sdk` (for `#include "dsp/<concept>/<name>.inc"`), then the directories of the module's sources.

A header in the module's `src/` is shared by C and SHARC+ sources. Use only decimal `#define`s and `//` comments on their own line: the SHARC+ assembler keeps a trailing comment in the macro text.

## Declaring an effect

An effect is built in `init` with the builder calls of `<dk/gen/imports.h>` (`dk_module_ctx_add_fx`, `dk_fx_routes`, `dk_fx_add_page`, `dk_page_add_fx_param`).
The effect id goes in `module.toml` as `[[fx]]`; `patchkit gen` writes `build/gen/module_ids.h` with `DK_FX_ID_<ID>`, the index of the id in the `[[fx]]` list (the packer computes the keys from the packed name). This is `modules/fx/freqshift/src/module.c`:

```c
#include <dk/core/module.h>
#include <dk/gen/page_consts.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_fx *fx = dk_module_ctx_add_fx(ctx, DK_FX_ID_FSHIFT, "Freq Shifter", "FSH");
    dk_fx_routes(fx, DK_ROUTE_SEND | DK_ROUTE_MASTER, DK_ROUTE_SEND);
    dk_page *p = dk_fx_add_page(fx, "Freq Shifter", "FSH");
    dk_page_add_fx_param(p, "Freq Shift", "SHIFT", DK_BIPOLAR, -64.0f, 63.0f, 0.0f);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
```

List the file as an `[[object]]` in `module.toml`. The guide ([docs/MODULE-AUTHORING.md](../docs/MODULE-AUTHORING.md)) is rewritten for the builders in S4.

## Declaring a machine

A machine is built the same way (`dk_module_ctx_add_machine`, `dk_machine_add_page`, `dk_page_add_param`, `dk_param_range`, `dk_param_glyph`, ...), with `[[machine]]` in
`module.toml` and `DK_MACHINE_ID_<ID>` (the index of the id in the `[[machine]]` list) from `module_ids.h`. This is `modules/machines/analog/src/module.c`:

```c
#include <dk/core/module.h>
#include "module_ids.h"

static int32_t init(dk_module_ctx *ctx)
{
    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_ANALOG, "Analog", "ANA");
    dk_page *p = dk_machine_add_page(m, "Analog", "ANALOG");
    dk_page_add_param(p, "Detune", "DET", 40);
    dk_page_add_param(p, "Wave", "WAVE", 0);
    dk_page_add_param(p, "Sub", "SUB", 0);
    dk_page_add_param(p, "Fold", "FOLD", 0);
    return DK_OK;
}

const dk_module_def dk_module = {.init = init};
```

Parameter values are knob units 0 to 127 (`dk_param_range` changes the range). The DSP symbols (`_<id>_render`, `_<id>_note_on`, optional `_<id>_desc`) are found by name: a machine has no
data-only form. `[uses.<device>]` in `module.toml` (written by `module:check`) says how many machine, parameter and page rows `init` takes.

`dk_param_glyph(param, GLYPH)` sets the knob look of a parameter (the constants are in `<dk/gen/page_consts.h>`). Without it the glyph is `DK_GLYPH_ENCODER`:

| Glyph | Range |
| --- | --- |
| `DK_GLYPH_ENCODER` (default) | 0 .. 127 |
| `DK_GLYPH_FADER` | 0 .. 127 |
| `DK_GLYPH_PAN` | 0 .. 127 (centre 64, shows L/R) |
| `DK_GLYPH_ONOFF` | 0 .. 1 |
| `DK_GLYPH_TUNE` | 4 .. 124 (centre 64, one unit = one semitone) |
| `DK_GLYPH_PLAY` | 0 .. 3 (REV, REV.L, FWD.L, FWD; the Digitone II shows 0 .. 3) |

## Tracing

Trace marks tell you where a module was when it faulted or hung. They are opt-in and cost nothing when off.

| Side | Include | Macro | What it does when on |
| --- | --- | --- | --- |
| ColdFire C | `<dk/core/trace.h>` | `DK_TRACE_LOG("msg", value);` | Calls `dk_log(DK_LOG_TRACE, msg, value)`: the message (24 bytes kept) and a word go to the runtime's log ring (`dk:log`). |
| SHARC+ | `#include "dk_trace.inc"` | `DK_STEP(3, R0);` | Loads the step number 3 into the scratch register `R0` and stores it in the word `_dk_step`. Pick a register that is free at that point: SHARC+ cannot store an immediate to memory. |

The C macro is `DK_TRACE_LOG` because the build define is `DK_TRACE` and a function-like macro of that name would clash with it.

Build with `--trace` to switch them on: `mise run module:pack -- ID --device dt2 --trace`, `mise run module:check -- ID --trace`
(size lines), `mise run dk:dev -- ID --store ID --trace`. `--trace` defines `DK_TRACE` for the C sources (`-DDK_TRACE`) and the SHARC+
sources (`#ifdef DK_TRACE`), and sets bit 31 of the header word at 0x30 of the `.dkm` (`patchkit dkm-info` prints `trace yes`; a runtime
that does not know the bit ignores it). Traced and plain objects are cached apart (`out/cache/dko/<id>+DK_TRACE/`).

What you see:

- DIGIKIT, ABOUT: the module's result row ends in ` TRACE` (for example `- OK TRACE`).
- The fault record: the core clears `_dk_step` before each call into a module, the block tail copies it into the block's reply record, and the fault record prints the step of the last block that ended, for example `FX SEND STEP 3`. A step stored in the call that stops the DSP is lost with the core: put the mark before the call that you suspect on an earlier block, or make it a state that holds from block to block.
  A traced module imports `_dk_step` from `core/dyndsp`; a runtime without that export refuses it with `DSP IMPORT`.
- The log: the `DK_TRACE_LOG` messages.

Off means off: without `--trace` both macros expand to nothing (the arguments of `DK_TRACE_LOG` are not evaluated), the objects have no extra
instruction, data or import, and the `.dkm` is byte-identical to one built from sources without the marks
(`packages/patchkit/tests/trace.rs`). A `DK_STEP` costs two instructions when on, so keep it out of the innermost loop.

## Writing modules

See [docs/MODULE-AUTHORING.md](../docs/MODULE-AUTHORING.md). The plan for the SDK API is
[docs/plans/sdk-api.md](../docs/plans/sdk-api.md).
