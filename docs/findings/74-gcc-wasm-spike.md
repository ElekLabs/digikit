# gcc cc1 and binutils as for wasm32-wasi: spike

Plan: [sdk-api](../plans/sdk-api.md) phase 6b. Marks per AGENTS.md. Spike time: about 1.5 hours of work, most of it
waiting for builds. All results are **[D]** (measured here on macOS arm64, wasmtime 50 and node 24); nothing was
checked on an image.

## Question

Can the `cc1` of gcc 16.2.0 and the `as` of binutils 2.47 (same sources and target as `out/tools/m68k-elf`,
`-mcpu=54418`) be built as wasm32-wasip1 and produce the same object bytes as the native tools, so the browser can
compile C content modules?

## Result

**Yes.** Both build with wasi-sdk 34 (clang 23.1) in one pass; no source patch of gcc or binutils is needed except one
function. Output is **byte-identical** to the native toolchain:

- 32 C files of `modules/` and `tests/fixtures/modules/` (all that the native gcc compiles with the shared include
  dirs, including `ui.c`, `menu.c`, the freqshift effect): the assembly text from the wasm `cc1` and the object from the
  wasm `as` equal the native `.s` and `.o` byte for byte. The sweep is a loop that is not kept (the build script
  is the recipe; the loop is a few lines of `cmp`).
- The same holds under wasmtime and under node's WASI (checked on `menu.c` and freqshift).
- `cc1` preprocesses by itself (integrated cpp), so the gcc driver and `cpp` are not needed. The driver is only used
  by native pk build to call `cc1` and `as`; the browser calls both directly. cc1 flags: the driver passes
  `-quiet -dumpbase X.c -dumpbase-ext .c -mcpu=54418 -Os -Wall -Wextra -Werror -ffreestanding -fno-builtin -fno-pie
  -ffunction-sections -fdata-sections -fno-asynchronous-unwind-tables -fno-unwind-tables -fno-stack-protector -o X.s`
  (`-nostdlib` and `-fno-pic` are driver-level or default). `as` gets `-mcpu=54418 -o X.o X.s`.
- cc1 reads gcc's freestanding headers (`stdint.h`, `stddef.h`, `stdarg.h`, `limits.h`, ...; 108 KB of the
  `lib/gcc/m68k-elf/16.2.0/include` dir) at run time: map them into the guest and pass `-isystem` (the native search
  order is `-I` dirs, then that dir).

## What blocked and how it was solved

Prior art searched: there is no maintained "gcc in wasm" project; the known route is wasi-sdk or emscripten for the
host side, and gcc is a plain C++17 program (built with `-fno-exceptions -fno-rtti`, no setjmp in the compile path). It
needed:

| Issue | Fix |
| --- | --- |
| libiberty `pex-unix.c` (fork, exec, wait, kill) and `mktemp`, `umask` missing in wasi-libc | stubs that fail with ENOSYS in a force-included header (`tools/patchkit/gcc_wasm_shim.h`); the wasm tools never spawn |
| signal, mman, getpid, process-clock functions live in `wasi-emulated-*` libraries; configure probes then misdetect `psignal`, `strsignal`, `putenv`, ... and libiberty replacements clash with wasi-libc | put the libraries after the sources in a compiler wrapper (wasm-ld resolves archives in order); do not force-include `stdlib.h` (autoconf probes break) |
| gas needs `libsframe` for bfd | do not use `--disable-libsframe` |
| default wasm stack is 64 KB | `-Wl,-z,stack-size=32M` |
| gcc `diagnostics/sarif-sink.cc` opens an AF_UNIX socket (`EXPERIMENTAL_SARIF_SOCKET` only) | compile a copy with that one function emptied |
| `fixincludes` does not build; `make all-gcc` ends with an error when it runs the wasm `xgcc` for the specs file | `make -o all-fixincludes -k all-gcc`; `gcc/cc1` exists by then |
| host generators (`genmodes`, `gengtype`, ...) must run natively | gcc's configure does this itself with `CC_FOR_BUILD=clang`, `CXX_FOR_BUILD=clang++` (canadian-style cross: build=macOS, host=wasm32-wasip1, target=m68k-elf) |
| GMP, MPFR, MPC | static wasm libraries from the gcc prerequisites tarballs (hash checked), `--disable-assembly`; their `config.sub` is replaced with binutils' to accept `wasm32-wasip1`. ISL and zstd are not used (`-Os` needs no graphite) |
| **first cc1 differed**: string sections were `.rodata` instead of `.rodata.fn.str1.1,"aMS"` | gcc's configure probes the assembler; with no target `as` it picks "no SHF_MERGE". Pass `--with-as`/`--with-ld` pointing at the native m68k-elf tools (probes only; wasm cc1 never runs them). Then all 32 files match. **[C]** a build without `--with-as` is silently different and must not ship |

Not needed: setjmp/longjmp, C++ exceptions, mmap beyond the emulation, threads, the gcc driver, fork/exec.

## Sizes

| file | raw | gzip -9 | brotli -q11 |
| --- | --- | --- | --- |
| `cc1.wasm` | 20.4 MB | 6.6 MB | 4.8 MB |
| `cc1.wasm` stripped of names | 19.6 MB | 6.5 MB | 4.7 MB |
| `as.wasm` | 1.23 MB | 426 KB | 360 KB |
| `as.wasm` stripped | 0.91 MB | 354 KB | 300 KB |
| gcc headers | 108 KB | | |

`wasm-opt -Oz` was not tried. About 5 MB of brotli for cc1 is acceptable for an on-demand download that the browser
caches.

## Times and memory

Native: gcc driver (cc1 + as) 0.06 s (freqshift), 0.20 s (`ui.c`, 27 KB), 0.28 s (`menu.c`, 40 KB).

| run | freqshift | `menu.c` |
| --- | --- | --- |
| wasmtime, cc1 | 0.30 s | 0.75 s |
| node 24 WASI, cc1 | 0.07 s run (+30 ms compile) | 1.1 s run |
| wasm `as` (wasmtime / node) | 0.05 s | 0.02 s (node) |

So wasm is about 2.5 to 4 times slower than native and a module build stays under about 2 s. The first wasmtime run of a
fresh `cc1.wasm` compiles it (3.4 s, 1.7 GB peak once); later runs use the cache. Peak memory of a run: 130 to 140 MB
in wasmtime, 130 MB (small file) to 730 MB RSS (menu.c) in node, which includes V8 code. Browsers cap wasm32 at 4 GB;
this fits. **[O]** Browser timing and memory were not measured (needs a WASI shim in a web worker, for example
`@bjorn3/browser_wasi_shim`); node's WASI is the same ABI.

## Recipe

`tools/patchkit/gcc_wasm_build.sh WORKDIR` builds `WORKDIR/out/as.wasm`, `cc1.wasm` and `include/` from the sources that
`pk_toolchains` downloaded (wasi-sdk 34 arm64-macos is fetched; other build hosts need the matching archive URL).
`tools/patchkit/gcc_wasm_run.mjs TOOL.wasm HOST[::GUEST],... -- ARGS` runs a tool under node. A clean run from nothing (download, as, GMP/MPFR/MPC,
cc1) took 7 minutes on this machine with 6 jobs; the rebuilt binaries pass the same 32-file comparison.

## Open

- **[C]** `ld` builds with the same recipe and the browser run works: see [finding 160](160-browser-module-compile.md).
- **[O]** Rust modules (rustc_codegen_gcc / libgccjit) are not covered; this is C only.
- **[O]** Browser run, wasm-opt size, a pinned wasi-sdk hash, Linux and x86 build hosts.
- **[O]** Byte identity was proven for 32 files. A bigger corpus belongs in the gate if this ships: compile every
  module with both and compare.

## Recommendation

Ship gcc in wasm (route 1) for phase 6b. It gives identical bytes with no new compiler to maintain: the registered
risk of an own ColdFire C compiler (and of clang's M68k backend, which cannot match gcc bytes and has weaker ColdFire
support) is avoided. Cost to carry: a 5 MB download, a reproducible build script, a pinned wasi-sdk, and one gate step
that compares wasm and native output. Next: build `ld` the same way, run `cc1`/`as`/`ld` from a web worker, then
wire `pk build` (phase 6c) to call them through a small WASI host.
