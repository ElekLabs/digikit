# Nx host cache pilot (2026-11-01)

**[D]** Bounded local observations on base `04bfd560` plus the worktree-isolation fix, using
Node 24.15.0, pnpm 12.8.2, Nx/devkit 22.7.12 and `@monodon/rust` 3.0.0. The target was
`pnpm nx:pilot run control-client:test-host --outputStyle=static`. This establishes the exercised
host cache behavior, not exhaustive input closure or a performance improvement.

## Cache isolation

**[D]** Nx 22 uses the main worktree for both its shared cache and task database. A worktree's
`nx.json` `cacheDirectory` alone did not isolate these artifacts. The initial successful host run
and cache hit used hash `14423200522536462197` in the main checkout's `.nx` directory, which appeared
as untracked until moved intact into the pilot's ignored destination.
Inspection of installed Nx sources found `sharedCacheDirectory` in
`dist/src/utils/cache-directory.js` and main-root database selection in
`dist/src/utils/db-connection.js`.

The public pilot script now sets absolute paths from the checkout root:

```sh
NX_CACHE_DIRECTORY="$PWD/.nx/cache"
NX_WORKSPACE_DATA_DIRECTORY="$PWD/.nx/workspace-data"
```

The initial shared Nx artifacts were moved intact into the pilot's ignored
`.nx/pilot-validation/shared-cache-before-isolation`. The main checkout was clean after the isolated
runs. These were Nx cache artifacts; this observation does not indicate a change to PGO artifacts.

## Host input probes

**[D]** After isolation, the following sequence completed locally:

| Input state | Task hash | Cache result | Exit | Test evidence |
| --- | --- | --- | --- | --- |
| Original baseline | `4186974415010007784` | cache-miss | 0 | Six tests passed freshly |
| Same input repeat | `4186974415010007784` | local-cache-hit | 0 | Replayed successful result |
| Unrelated README HTML comment | `4186974415010007784` | local-cache-hit | 0 | Replayed successful result |
| Appended standalone `assert.fail` test | `16930213597592617922` | cache-miss | 1 | Original six passed; intentional probe failed |
| Byte-exact original files restored | `4186974415010007784` | local-cache-hit | 0 | Replayed original successful result |

The README and test file were saved before mutation and restored byte-for-byte. Their final Git
diffs were empty. Local records are `.nx/pilot-validation/02-repeat.json`, `03-unrelated.json`,
`04-changed-input.json` and `05-restored.json`; the baseline was observed in command output and
cached terminal output. The JSON records retain hashes, cache status and exit status; they are ignored
local evidence, not committed artifacts.

The first sandboxed host attempt failed all six tests with Unix socket `EPERM`. Running outside that
restriction passed the six original tests. This is evidence of the sandbox restriction, not an
established product flake.

## Limits and follow-up

**[D]** Project/target inspection, graph export, targeted configuration formatting and independent
review completed before the probes; `Cargo.lock` remained unchanged. Input coverage was reviewed
statically. Config, lockfile, toolchain and every possible environment input were not each mutated
and tested. The exercised fresh result and cached replay agreed; no timing claim is made.

**[O]** Rust and runtime functional execution remain pending. Both have `cache: false`; Rust cache
enablement needs a Cargo configuration/environment audit, and runtime cache enablement needs closure
over ignored and external inputs. Existing verification routing remains in place. See the
[pilot plan](../plans/nx-pilot.md) for commands, input boundaries and migration conditions.
