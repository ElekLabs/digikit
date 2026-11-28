# The browser builds a `.dkm` from a `.dkmod`: byte-identical to `patchkit pack`

Plan: [module distribution](../plans/module-distribution.md) (the deferred "compile C in the browser" item), follow-up of
[finding 74](74-gcc-wasm-spike.md). Marks per AGENTS.md. Everything here is **[D]** (measured on macOS arm64, headless
Chromium 149, one session); nothing is **[V]**. Spike time: about 1.5 hours of work, most of it waiting for builds.

## Question

Can the browser turn the contents of a C module's `.dkmod` into a `.dkm` that is byte-identical to what native
`patchkit pack` writes (and so to what `patchkit build` + `pack` make)?

## Answer

**Yes.** In headless Chromium, a web worker with `@bjorn3/browser_wasi_shim` 0.4.2 runs `cc1`, `as`, `ld`, `nm` and
`objcopy` (all gcc 16.2.0 / binutils 2.47, m68k-elf, as wasm32-wasip1) and `patchkit.wasm`, and returns the `.dkm`.

| set | result |
| --- | --- |
| `test/c-demo` (needs 64-bit helpers from libgcc, the SDK library and `ld -r`) | sha256 `eaeb7dbe...` equal to native, 5396 bytes; also at another guest root path |
| all 49 fixture modules in `tests/fixtures/modules` without Rust and without `workbench.toml`, device dt2 | **46 of 46** that native `pack` writes are identical; 3 are not comparable (below) |
| the same set, device dn2 | **45 of 45** identical |

The three that do not compare: `cf-demo` and `registry-demo` are patch-only (no `dk_module`; native `pack` refuses them
too), `svc-test` is a service (native `pack` needs a `.dks` name). One more (`dyn-dsp-hang`) is not built for dn2.
The set covers machines, effects with a SHARC+ object (`sine`, `dyn-fx-gain*`: DSP assembled by Selache inside
`patchkit.wasm`), raw C modules, SDK pages (`settings-page`, `ui-views`, `ui-demo`) and library-helper users. The `.dko.json` of
`c-demo` also matches native in sections, symbols, relocations and the ISA-check digest; only the `toolchain` version lines
and `source_sha256` differ (the spike does not compute them; the `.dkm` carries neither).

## The native pipeline, step by step

`patchkit pack ID` builds a stale object first (`packages/patchkit/src/build/cf.rs`), then packs. For a ColdFire C object of a
content module (`proto_of_sources`):

| step | tool | in the browser |
| --- | --- | --- |
| `gcc -c` each source (CFLAGS + `-I` module `build/gen`, `src`, five repository include dirs) | gcc driver = `cc1` + `as` | `cc1` and `as` called directly (finding 74), `-isystem` for gcc's 108 KB of freestanding headers |
| SDK library: the same for every `sdk/src/**/*.c`, then `ld -r` | gcc, `ld` | same; `sdk.o` is rebuilt per run (about 1 s) |
| `ld -r` when a module has several sources | `ld` | `ld.wasm` |
| `nm -u`, `nm -g --defined-only` (which SDK members and libgcc helpers are needed) | `nm` | `nm.wasm` |
| `ld -r --gc-sections -u SYM... sdk.o`, then `ld -r module.o sdk-part.o` | `ld` | `ld.wasm` |
| libgcc helpers: `ld -r obj libgcc-cf54418.a`, then `objcopy --remove-section=.eh_frame --localize-symbol=...` | `ld`, `nm`, `objcopy` | `ld.wasm`, `nm.wasm`, `objcopy.wasm` (explicit output file: see below) |
| ELF to prototype, link at the placeholder base, ISA re-decode, `.dko.json` | patchkit, in process | `patchkit.wasm`, new export `pk_cf_object` |
| SHARC+ `.s` to `.dko.json` | patchkit (Selache as a library) | `pk_sharc_object` (already existed) |
| generated `build/gen/module_ids.h` | patchkit (`gen`) | new export `pk_module_ids` |
| pack the objects as a `.dkm` | patchkit | new export `pk_pack` (the `Library` of `wasm.rs`, one module, no ports) |

## Link step

**patchkit cannot link the module objects itself, and needs binutils `ld`.** Its own linker (`cf::link`, used for the
runtime in the browser) places an already merged prototype at an address and applies relocations; it does not merge
relocatable objects (`ld -r`), prune the SDK library (`--gc-sections` from the called symbols) or pull archive members
(libgcc helpers). `pk build` also uses `nm` and `objcopy`. All three build with the finding-74 recipe, with `--disable-ld` and
`--disable-binutils` dropped (`tools/patchkit/gcc_wasm_build.sh` now builds them). Two pitfalls:

- `ld` does not compile: `ldmain.c` reads `ru_maxrss`, which wasi-libc's `struct rusage` lacks. `HAVE_GETRUSAGE` is turned
  off in `ld/config.h` after `configure-ld` (the script does it).
- `objcopy` in place fails (`bfd_fdopenr`: its temporary file needs `mkstemp`). Writing to a second file gives the same
  bytes; the driver does that.

`ld -r` of an object with libgcc was checked byte for byte against the native `ld` under node as well.

## Browser run

`tools/patchkit/browser_build/` (driver, worker, pipeline; usage below). The worker fetches the tools, compiles them
(`WebAssembly.compile`), unpacks one input bundle (the 5 include dirs, `sdk/src`, `sdk/dsp`, gcc headers and `libgcc-cf54418.a`: 146
files) into an in-memory tree served as one WASI preopen, unzips the `.dkmod` (own 40-line zip reader with
`DecompressionStream`) and runs the steps above, each tool as a fresh instance. Guest paths equal the host paths in the
main run (gcc embeds source paths); a run at a different guest root gave the same `.dkm`.

Download (brotli q9, localhost; times here are not network times):

| file | raw | brotli |
| --- | --- | --- |
| `cc1.wasm` | 20.4 MB | 5.5 MB |
| `as.wasm`, `ld.wasm`, `nm.wasm`, `objcopy.wasm` | 1.2, 1.6, 0.9, 1.0 MB | 0.40, 0.42, 0.28, 0.32 MB |
| `patchkit.wasm` (wasm-opt -Oz, with the three new exports; the site loads it anyway) | 3.6 MB | 0.96 MB |
| include and SDK bundle | 1.09 MB | 0.24 MB |
| total beyond `patchkit.wasm` | | about 7.2 MB |

Build times in the page, after the downloads (three modules in one worker, tools already compiled; the first run of a
page pays JIT warm-up). Per module: 9 `cc1` runs (1 module file and the 8 SDK files) are 85 to 90 % of the time.

| module | total | cc1 | as | ld+nm+objcopy | patchkit |
| --- | --- | --- | --- | --- | --- |
| `c-demo` (first, cold) | 2.4 s | 2.2 s | 0.07 s | 0.08 s | 0.07 s |
| `sine` (with SHARC+) | 1.3 s | 1.1 s | 0.04 s | 0.02 s | 0.07 s |
| `ui-views` | 1.1 s | 1.0 s | 0.04 s | 0.01 s | 0.02 s |

Warm runs of the 49-module sweep were 0.5 to 0.9 s each. Native `pack` of `c-demo`: 0.5 s with a warm object cache.
`cc1.wasm` fetch plus compile: 3.3 s on localhost (compile 23 ms: Chromium streams and caches). Memory: each tool's
linear memory stays at 34 to 41 MB for these modules (the largest module sources of finding 74 reached 130 MB in
wasmtime); the whole Chromium process tree peaked at 1.0 to 1.3 GB RSS (a page that does nothing is about 390 MB).

## What matched, what did not, what is open

- Matched: 46 of 46 (dt2) and 45 of 45 (dn2) comparable modules; the `.dko.json` content of `c-demo`; with a different guest path.
- Not matched: nothing that was compared.
- **[O]** Modules with a `workbench.toml` need the generated query and descriptor headers (`workbench_descriptor.h`, ...:
  `build::workbench`), which patchkit makes in process; one more export, not done (fixtures with a `workbench` folder were left out).
- **[O]** Rust modules and services (`rustc_codegen_gcc`) are out by design.
- **[O]** `dko` metadata: `source_sha256` and the `toolchain` record are not computed in the browser (`gcc --version` lines
  and the dependency list from `cc1 -MM`); a site that wants a native-equal `.dko.json` or a build cache needs them.
- **[O]** Only Chromium was run. Firefox and Safari (4 GB wasm32 limit, 32 MB stack request, `DecompressionStream`) untested; a
  pinned wasi-sdk hash, a Linux and x86 build host, and a CI check that compares all modules (this driver is the loop) are open
  as in finding 74.
- **[O]** The bundle must hold exactly the SDK and headers of the site's build: native hashes them into every object, the
  browser takes whatever it was given. A `.dkmod` builds against the SDK of the machine that receives it
  ([module distribution](../plans/module-distribution.md)), so the site would ship the SDK bundle with each release.
- The reference was `dist/patchkit/patchkit` of the main tree (built from another branch), not a binary of this commit;
  46 equal `.dkm` files make a pack-logic difference unlikely, but the gate form should use one commit.

## Recommendation

Do it, after the Project Manager page and in this order: (1) cache the SDK library object per SDK hash (the `sdk.o` is the
same bytes every time and is 1 s of 1.1 to 2.4 s; ship it prebuilt in the bundle or keep it in `BlobStore`), (2) add the
`workbench` header export, (3) move the driver loop into a gate check (browser vs native over the fixtures), (4) wire an
"Import `.dkmod`" button on the site that runs the worker. Payoff: the site installs user-supplied C modules without the
desktop app and without a build server, with the same bytes as the checked native build; cost about 7 MB on demand
(cached) and a reproducible wasm toolchain build in CI (7 minutes) plus the glue (about 250 lines of JS, 3 small exports).
Confidence: high that C modules stay byte-identical (the pipeline is deterministic and 46 files agree on two devices);
medium on shipping effort (Safari and Firefox, large modules and workbench headers are untested). Stripped names and
`wasm-opt` on `cc1.wasm` and an own `nm` in patchkit (instead of `nm.wasm` and `objcopy.wasm`) would save about 1 MB more.
If the product decision "the browser compiles nothing" stands, this finding still shows the route stays open.

## Driver

```sh
tools/patchkit/gcc_wasm_build.sh out/gcc-wasm            # as, cc1, ld, nm, objcopy, include/ (7 minutes warm sources)
cargo build --release -p patchkit --lib --target wasm32-unknown-unknown --target-dir out/gcc-wasm/pk-target
wasm-opt -Oz ... -o out/gcc-wasm/out/patchkit.wasm       # as apps/web does (tools/nx/wasm.mjs)
(cd out/gcc-wasm/jsdeps && npm i --save-exact @bjorn3/browser_wasi_shim@0.4.2 playwright-core@1.65.1)
node tools/patchkit/browser_build/run.mjs --work out/gcc-wasm --ids test/c-demo,test/sine --device dt2 \
  --patchkit out/gcc-wasm/out/patchkit.wasm --pk dist/patchkit/patchkit [--fixtures true] [--root /guest/path]
```

`run.mjs` exports each id as a `.dkmod` (`pk_package`), packs it natively for the reference, serves the files to headless
Chromium (the cached Playwright headless shell; the repository has no Playwright dependency, so `playwright-core` is
installed under `out/`), and prints `IDENTICAL` or `DIFFERENT` per module with timings. `--fixtures` also gives the page
the fixtures' `src/` trees: wrapper fixtures (`dyn-fx-gain-b`, `lfo-shape-b`, `settings-b`, ...) include a sibling module's
source, which a `.dkmod` does not carry.
