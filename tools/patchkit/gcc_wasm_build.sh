#!/bin/bash
# Build the m68k-elf `as` and `cc1` (binutils 2.47, gcc 16.2.0 as in pk_toolchains.py) as wasm32-wasip1 modules.
# Spike, finding 74. Needs: the sources from `pk_toolchains` in out/tools/src, curl, clang/clang++ (native, for the build
# side), python3, GNU make, and the native m68k-elf as/ld (only so gcc's configure probes the right assembler features:
# without them the wasm cc1 writes different section names).
#   usage: gcc_wasm_build.sh WORKDIR [NATIVE_M68K_BIN]      -> WORKDIR/out/{as,cc1,ld,nm,objcopy}.wasm
set -euo pipefail
WORK=$(mkdir -p "$1" && cd "$1" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
SRC=$(cd "$ROOT/out/tools/src" && pwd -P)
M68K=${2:-$(cd "$ROOT/out/tools/m68k-elf/bin" && pwd -P)}
BU=$SRC/binutils-2.47 GCC=$SRC/gcc-16.2.0
JOBS=${JOBS:-6}
WASI_V=34 WASI=$WORK/wasi-sdk
BUILD=$(sh "$BU/config.guess")
mkdir -p "$WORK/dl" "$WORK/out" "$WORK/prefix"

fetch() { [ -s "$WORK/dl/$(basename "$1")" ] || curl -fsSL -o "$WORK/dl/$(basename "$1")" "$1"; }

# 1. wasi-sdk (clang + wasi-libc + libc++)
if [ ! -x "$WASI/bin/clang" ]; then
  fetch "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$WASI_V/wasi-sdk-$WASI_V.0-arm64-macos.tar.gz"
  mkdir -p "$WASI" && tar xzf "$WORK/dl/wasi-sdk-$WASI_V.0-arm64-macos.tar.gz" -C "$WASI" --strip-components=1
fi

# 2. compiler wrappers. wasi-libc keeps signal, mman, getpid, process-clock functions in emulation libraries; wasm-ld
#    resolves archives in command order, so they go last on every command line (configure probes included).
mkdir -p "$WORK/wrap"
for p in clang clang++; do
cat > "$WORK/wrap/wasi-$p" <<EOF
#!/bin/bash
args=()
for a in "\$@"; do
  case "\$a" in */diagnostics/sarif-sink.cc) a=$WORK/sarif-sink-nosock.cc;; esac   # unix sockets do not exist in WASI
  args+=("\$a")
done
exec $WASI/bin/$p --target=wasm32-wasip1 --sysroot=$WASI/share/wasi-sysroot -Wno-unused-command-line-argument -Wno-unknown-warning-option "\${args[@]}" -lwasi-emulated-signal -lwasi-emulated-process-clocks -lwasi-emulated-mman -lwasi-emulated-getpid
EOF
chmod +x "$WORK/wrap/wasi-$p"
done
# gcc's sarif-sink.cc: replace maybe_open_sarif_sink_for_socket (EXPERIMENTAL_SARIF_SOCKET only) with an empty function.
python3 -I - "$GCC/gcc/diagnostics/sarif-sink.cc" "$WORK/sarif-sink-nosock.cc" <<'EOF'
import sys
t = open(sys.argv[1]).read()
i = t.index("void\nmaybe_open_sarif_sink_for_socket (context &dc)")
j = t.index("\n}\n", i) + 3
open(sys.argv[2], "w").write(t[:i] + "void\nmaybe_open_sarif_sink_for_socket (context &dc)\n{\n  (void) dc;\n}\n" + t[j:])
EOF

wasi_env() {
  export CC=$WORK/wrap/wasi-clang CXX=$WORK/wrap/wasi-clang++
  export AR=$WASI/bin/llvm-ar RANLIB=$WASI/bin/llvm-ranlib NM=$WASI/bin/llvm-nm STRIP=$WASI/bin/llvm-strip
  export CFLAGS="-include $HERE/gcc_wasm_shim.h -O2 -D_WASI_EMULATED_SIGNAL -D_WASI_EMULATED_PROCESS_CLOCKS -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_GETPID"
  export CXXFLAGS="$CFLAGS -fno-exceptions"
  export LDFLAGS="-Wl,-z,stack-size=33554432"          # cc1 recurses deeply; the default wasm stack is 64 KB
  export CC_FOR_BUILD=clang CXX_FOR_BUILD=clang++ CFLAGS_FOR_BUILD=-O1 CXXFLAGS_FOR_BUILD="-O1 -std=c++17"
  unset LIBS
}
wasi_env

# 3. binutils: gas, ld, nm, objcopy (the tools pk build runs besides gcc; finding 75)
if [ ! -s "$WORK/out/objcopy.wasm" ]; then
  mkdir -p "$WORK/bu" && cd "$WORK/bu"
  "$BU/configure" --build="$BUILD" --host=wasm32-wasip1 --target=m68k-elf --disable-nls --disable-werror --disable-gdb \
    --disable-sim --disable-gprof --disable-gprofng --disable-gold --disable-shared \
    --enable-static --without-zstd --disable-plugins --disable-libctf > cfg.log 2>&1
  # wasi-libc's struct rusage has no ru_maxrss (ld --stats); ld has its own configure
  make -j"$JOBS" configure-ld > make.log 2>&1
  sed -i.bak 's|#define HAVE_GETRUSAGE 1|/* no ru_maxrss in WASI */|' ld/config.h
  make -j"$JOBS" all-gas all-ld all-binutils >> make.log 2>&1
  cp gas/as-new "$WORK/out/as.wasm"
  cp ld/ld-new "$WORK/out/ld.wasm"; cp binutils/nm-new "$WORK/out/nm.wasm"; cp binutils/objcopy "$WORK/out/objcopy.wasm"
fi

# 4. gmp, mpfr, mpc as static wasm libraries (versions and hashes from gcc's contrib/prerequisites.sha512)
if [ ! -s "$WORK/prefix/lib/libmpc.a" ]; then
  for f in gmp-6.3.0.tar.bz2 mpfr-4.2.2.tar.bz2 mpc-1.3.1.tar.gz; do
    fetch "https://gcc.gnu.org/pub/gcc/infrastructure/$f"
    grep " $f\$" "$GCC/contrib/prerequisites.sha512" | sed "s|$f|$WORK/dl/$f|" | shasum -a 512 -c - >/dev/null
    mkdir -p "$WORK/deps" && tar xf "$WORK/dl/$f" -C "$WORK/deps"
  done
  for d in gmp-6.3.0 mpfr-4.2.2 mpc-1.3.1; do   # their config.sub is too old for wasm32-wasip1
    for t in "$WORK/deps/$d/config.sub" "$WORK/deps/$d/build-aux/config.sub"; do [ -f "$t" ] && cp "$BU/config.sub" "$t"; done
    for t in "$WORK/deps/$d/config.guess" "$WORK/deps/$d/build-aux/config.guess"; do [ -f "$t" ] && cp "$BU/config.guess" "$t"; done
  done
  mkdir -p "$WORK/gmp-b" && (cd "$WORK/gmp-b" && "$WORK/deps/gmp-6.3.0/configure" --host=wasm32-wasip1 --build="$BUILD" \
    --prefix="$WORK/prefix" --disable-assembly --enable-static --disable-shared --disable-cxx >cfg.log 2>&1 && make -j"$JOBS" >make.log 2>&1 && make install >>make.log 2>&1)
  mkdir -p "$WORK/mpfr-b" && (cd "$WORK/mpfr-b" && "$WORK/deps/mpfr-4.2.2/configure" --host=wasm32-wasip1 --build="$BUILD" \
    --prefix="$WORK/prefix" --with-gmp="$WORK/prefix" --enable-static --disable-shared >cfg.log 2>&1 && make -j"$JOBS" >make.log 2>&1 && make install >>make.log 2>&1)
  mkdir -p "$WORK/mpc-b" && (cd "$WORK/mpc-b" && "$WORK/deps/mpc-1.3.1/configure" --host=wasm32-wasip1 --build="$BUILD" \
    --prefix="$WORK/prefix" --with-gmp="$WORK/prefix" --with-mpfr="$WORK/prefix" --enable-static --disable-shared >cfg.log 2>&1 && make -j"$JOBS" >make.log 2>&1 && make install >>make.log 2>&1)
fi

# 5. cc1. `make all-gcc` ends with an error when it runs the wasm xgcc for the specs file; cc1 is built by then.
#    -o all-fixincludes: fixincludes is not needed (no system headers) and does not build for WASI.
if [ ! -s "$WORK/out/cc1.wasm" ]; then
  mkdir -p "$WORK/gcc-b" && cd "$WORK/gcc-b"
  "$GCC/configure" --build="$BUILD" --host=wasm32-wasip1 --target=m68k-elf --enable-languages=c --disable-nls \
    --disable-multilib --disable-shared --disable-threads --disable-libssp --disable-libgomp --disable-libquadmath \
    --disable-libatomic --without-headers --with-newlib --disable-bootstrap --disable-lto --disable-plugin \
    --disable-libcc1 --disable-gcov --without-zstd --without-isl --with-gmp="$WORK/prefix" --with-mpfr="$WORK/prefix" \
    --with-mpc="$WORK/prefix" --with-as="$M68K/m68k-elf-as" --with-ld="$M68K/m68k-elf-ld" > cfg.log 2>&1
  make -o all-fixincludes -k -j"$JOBS" all-gcc > make.log 2>&1 || true
  [ -s gcc/cc1 ] || { echo "cc1 was not built; see $WORK/gcc-b/make.log" >&2; exit 1; }
  cp gcc/cc1 "$WORK/out/cc1.wasm"
fi
# gcc's own freestanding headers (stdint.h, stddef.h, ...) are read by cc1 at run time: map them into the guest.
mkdir -p "$WORK/out/include" && cp "$M68K/../lib/gcc/m68k-elf/16.2.0/include/"*.h "$WORK/out/include/"
ls -l "$WORK/out"
