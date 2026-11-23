# test/syspkg

The fixture system package for the stage-1 assembly cases (`core/runtime:test-assembly`). `body.bin` is 1000 bytes of a fixed ramp, so stage 1 reads it in
more than one 512-byte chunk. `pnpm nx:pilot run test/syspkg:pack` writes `dist/syspkg/test/system.dksp`: the 48-byte header (`runtime/core/sdk/src/syspkg.rs`)
and the body, built against resident table 2.0. It holds no runtime code; the body is a stub. It is not a module and has no `module.toml`.
