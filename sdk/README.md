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
| `include/dk/settings/` | `page.h`: a saved settings page from a table of fields (bool, int, enum). | yes |
| `include/dk/sample/` | Sample components: `pool.h` (per-module stereo sample pool), `view.h` (waveform drawing for native pages) and `picker.h` (on-device sample picker for a 16-entry pool). | yes |
| `include/dk/gen/` | C layouts and constants generated from `sdk.toml` (including `imports.h`, the prototypes of the service imports, and `sample.h`, the word layout of a sample pool entry). Included by the headers above, not by modules. | no |
| `src/` | Compiled SDK sources, in the folders of `include/dk/`: `chooser/chooser.c`, `chooser/logic.c`, `core/text.c`, `core/path.c`, `sample/picker.c`, `settings/field.c`, `settings/page.c`. The packer builds them into one SDK library and links the functions a module calls (`--gc-sections` drops the rest); a module does not list them in its `module.toml`. | yes |
| `dsp/` | SHARC+ code by concept (`osc/`, `filter/`, `dynamics/`, `delay/`, `util/`, `sample/`). See [dsp/README.md](dsp/README.md). `dsp/contract/` is on the SHARC+ include path and holds the call convention (`call.inc`), the includes generated from `sdk.toml` (`dk_fx.inc`, `dk_machine.inc`, `dk_sample_pool.inc`) and the hand-written `dk_trace.inc` (see Tracing). | yes |
| `templates/` | Module templates for `mise run module:new`: `machine/`, `fx/`, `tool/`. | yes |
| `tests/` | Host C tests, in the folders of `include/dk/` (`sdk:test-host`). | yes |
| `../runtime/core/vectors/` | Fit and settings expected results shared by the Rust, Python and web tests. They belong to the runtime, not to the SDK. | no |

### Where a new thing goes

Include direction: `core` <- `platform` <- components (`page`, `machine`, `fx`, `chooser`, `sample`, `settings`). A component may include
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

## Entry point, services and builder calls

A module defines one constant, `dk_module` (`<dk/core/module.h>`), and reaches the device through named imports. The prototypes of the imports,
including the builder calls that declare machines and effects, are generated into `include/dk/gen/imports.h` from the `[[import]]` entries of
`sdk.toml`. Each import has a `service`, a `since` (the runtime version that added it) and a `context`; the generated header repeats the last two
in the comment of each prototype.

A service that a service file (`.dks`) provides marks its imports `provided = true`; the file exports exactly those imports, in `sdk.toml` order, and its
`module.toml` has no `[[export]]`. A provided import takes one more optional field, `wrapper = "<module>"`: the module of the service's `dks/src`
(`fw`, `builder`, `glue`) that holds the function written by hand, for a function that converts a callback or a handle. An import without `wrapper` is a pure
wrapper over the service crate's `build::*` function (the name without `dk_` and a leading `module_ctx_`): `patchkit gen` writes it, with the type check against
the import's signature and the export list `gen_exports::exports()`, to `services/<service>/dks/src/gen_exports.rs`. A pure wrapper cannot take a
callback argument. A new builder call is an `[[import]]` and the `build::*` function; the generated file follows.

- [docs/MODULES.md](../docs/MODULES.md) says what a module is and why: the entry point, services and optional providers, the builder calls,
  what `module.toml` fixes, the load path and the versions.
- [docs/MODULE-AUTHORING.md](../docs/MODULE-AUTHORING.md) has the steps and a code sample for a machine, an effect and a tool.
- The naming rules (`dk_<area>_<verb>`, `ctx` and `arg`, `_fn` callback types) are in [docs/plans/sdk.md](../docs/plans/sdk.md) section 10.

The headers of the services are `core/log.h`, `core/mem.h`, `core/time.h`, `platform/host.h`, `settings.h`, `files.h`, `io.h`, `hash32.h`,
`intro.h`, `param.h`, `samples.h`, `shortcuts.h`, `ui.h`, `input.h`, `usb_bulk.h`, `dsp_link.h` and `debug.h`, and `page/page.h` for canvas pages. The logical key names
(`DK_KEY_TRK`, `DK_KEY_TRIG(n)`, type `dk_key`) are in the generated `gen/keys.h`: `patchkit gen` writes them from `[input.keys]` of the device files.

## Include paths

The build passes these search paths, in this order:

- ColdFire C and assembly: the module's `build/gen/`, the module's `src/`, `ports/bindings/inc`, then `sdk/include`.
- SHARC+ assembly: the module's `build/gen/`, the module's `src/`, `sdk/dsp/contract`, `sdk` (for `#include "dsp/<concept>/<name>.inc"`), then the directories of the module's sources.

A header in the module's `src/` is shared by C and SHARC+ sources. Use only decimal `#define`s and `//` comments on their own line: the SHARC+ assembler keeps a trailing comment in the macro text.

## Tracing

Start with the [DSP profiling workflow](dsp/README.md#profile-a-module-and-compare-two-builds).
A normal module build supports dispatch costs and emulator or rig hotspots without any author-inserted marks.
Add the marks below when a log or fault record needs an explicit checkpoint inside your code.

| Side | Include | Macro | What it does when on |
| --- | --- | --- | --- |
| ColdFire C | `<dk/core/trace.h>` | `DK_TRACE_LOG("msg", value);` | Calls `dk_log(DK_LOG_TRACE, msg, value)`: the message (24 bytes kept) and a word go to the runtime's log ring (`dk:log`). |
| SHARC+ | `#include "dk_trace.inc"` | `DK_STEP(3, R0);` | Loads the step number 3 into the scratch register `R0` and stores it in the word `_dk_step`. Pick a register that is free at that point: SHARC+ cannot store an immediate to memory. |
| SHARC+ (a service's dispatch code only) | `#include "dk_trace.inc"`, `#include "dk_span.inc"` | `DK_SPAN_BEGIN(skip, _dv_diag + 124, R3, I1);` | The span of a module call for the DSP stats window (docs/plans/profiling.md 13.1): with stats off a load, a compare and a jump. R3 and I1 must be free at the site; `dk_span.inc` holds the routines and takes `DK_SPAN_PTR_WORD` and `DK_SPAN_ROW_CODE` from the including file. Content modules do not use it (`check:layers`, rule spans). |

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
(`packages/patchkit/tests/trace.rs`). A `DK_STEP` costs two instructions when on. Use stable step numbers at
function boundaries and state transitions, using a scratch register that is dead there. The fault record
reports the last completed block's step. Use the plain build for an instruction comparison.

## Modules never touch hardware

A content module (everything outside `core/*`) reaches the device only through the runtime table and the services. It does not address a
peripheral, the flash, the FPGA or the peer CPU. Each device file lists the windows that only the firmware and `core/*` drive
(`[protect] windows` in `devices/*.toml`, evidence in [finding 141](../docs/findings/141-syntakt-analog-and-nor-paths.md)). `patchkit pack`
refuses a module whose objects hold an address in such a window of any device it is built for: the addend of a relocation, the value of
an absolute symbol, a word of a data section, or a 32-bit operand of an instruction (absolute long or immediate `.l` of `move`, `lea`, `pea`,
`jsr`, `jmp`, `ori` to `cmpi`, the bit instructions). The error names the window, the device, and the section and offset (or the relocation's
section, offset and symbol). A content module imports only from the runtime table, so there is no firmware routine name to check.

A window below 0x10000 (the Syntakt's FlexBus CS1 at address 0) is checked against absolute-symbol relocations only. A literal below 64 KiB is an
ordinary offset or constant, so those are not checked: a module that builds a low address in a register is not caught.

Accepted residual risk: a constant table can hold a word inside a window by chance (a 32-bit value such as `0xFC05C034` in a lookup table or a
`.l` immediate). The pack refuses it. The module author changes the table layout (another scale, an offset, 16-bit entries). There is no override.

## Writing modules

See [docs/MODULE-AUTHORING.md](../docs/MODULE-AUTHORING.md). The plan for the SDK API is [docs/plans/sdk-api.md](../docs/plans/sdk-api.md).
