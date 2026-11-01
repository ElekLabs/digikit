# Nx pilot

Status: dependencies installed without lifecycle scripts; all three resolved targets inspected with
Nx 22.7.12 and the project graph exported to `.nx/pilot-graph.json`, with `Cargo.lock` unchanged.
Targeted Prettier checks passed for six configuration files; independent review reported no issues.
The bounded host proof is complete after cache/workspace metadata isolation: six tests passed fresh,
repeat and unrelated-README probes hit the cache, a temporary failing test caused a miss and failure,
and restoring the original files recovered the original successful cache entry. See the
[observations and limits](../findings/nx-pilot.md). The named Rust target also executed once: only
dkneeds compiled and all ten tests passed, with caching disabled. Runtime functional execution and
Rust/runtime cache input audits remain pending. No speedup has been measured. Existing mise commands and verification routing remain the
owners of merge acceptance. The parked `--only` routing change is independent of this pilot.

## Boundary

Pin Nx and `@nx/devkit` to 22.7.12 and `@monodon/rust` to 3.0.0. Use the existing Node 24.15.0,
pnpm 12.8.2 and Rust 1.98.1 pins in [mise.toml](../../mise.toml). The root `nx:pilot` script disables
the Nx daemon and Cloud, and sets Cargo offline mode and `RUSTUP_TOOLCHAIN=1.98.1` before graph
discovery. Use that entrypoint rather than a bare Nx invocation. There is no remote cache or new gate.
The script sets `NX_CACHE_DIRECTORY="$PWD/.nx/cache"` and
`NX_WORKSPACE_DATA_DIRECTORY="$PWD/.nx/workspace-data"` to absolute paths in the current checkout.
Nx 22 otherwise shares its cache and task database with the main worktree, even when this worktree's
`nx.json` sets `cacheDirectory`. Run the script from the repository root so cache and metadata stay
within this checkout. The initial shared Nx artifacts were preserved intact under
`.nx/pilot-validation/shared-cache-before-isolation`; the main checkout is clean again.
The local cache and isolated Cargo build files live under ignored `.nx/`; do not share or upload them.

The Rust plugin discovers the Cargo project and its dependency graph. Its include filter admits only
`packages/dkneeds/Cargo.toml`. The plugin still calls root `cargo metadata --format-version=1`, which
resolves the workspace: the filter does not isolate metadata resolution. Offline dependency metadata
must already be available, and graph discovery must leave `Cargo.lock` unchanged. Missing metadata is
a failed pilot prerequisite, not a reason to turn on network access or run workspace builds.

The plugin test executor accepts only stable/beta/nightly as its toolchain option, so it cannot express
the repository's exact Rust pin. The pilot uses the standard `nx:run-commands` executor for the test
command and the plugin only for Cargo graph discovery. There is no custom plugin, executor, hasher,
runner or wrapper.

## Targets and graph

| Named target | Existing work invoked | Cache | Scheduling |
| --- | --- | --- | --- |
| `control-client:test-host` | Root host Vitest config, project `control-client` | Local | Normal host target |
| `dkneeds:test` | `cargo test -p dkneeds --lib --locked --offline`, Rust 1.98.1 | Disabled | `parallelism: false`; existing shared benchlock |
| `regression:service-commands` | Runtime Vitest project, only `runtime/service-commands.test.ts` | Disabled | `parallelism: false`; one Vitest worker |

Project discovery lists `control-client`, `@digi/test-support`, `@digi/dk-session`, `dkneeds`,
`regression`, `@digi/desktop` and `@digi/web`. The resolved dkneeds target has caching disabled and
the configured named Cargo command; the plugin also supplies Cargo release metadata.
The exported project graph has no dependency edges for control-client or dkneeds and shows regression
depending on control-client, `@digi/test-support` and `@digi/dk-session`. There is no generated-core
build edge for dkneeds. This pilot adds no `dependsOn` or prerequisite
build targets. Runtime inputs are prepared separately. Other existing package scripts may appear
as Nx targets through package discovery; do not use broad `run-many` commands for this pilot.
The three commands disable automatic argument forwarding to keep their suite and package scope fixed.

The host target has no persistent test outputs to restore (`outputs: []`); a cache hit replays the
successful test result and terminal output. Cargo's `.nx/cargo/dkneeds` is a separate incremental
build directory, not a declared task output and not a generated-core/PGO target directory. Nx's
`parallelism: false` serializes heavy targets within one Nx invocation, not across separate Nx processes
or other tools. The dkneeds command itself acquires the repository's existing shared benchlock before
Cargo; do not wrap it in another lock. Do not run any pilot validation alongside PGO work or a gate.

## Inputs and limits

The host hash includes all control-client files, the host Vitest config and project registry, root
package/lock/workspace files, mise and Nx config, the resolved Vitest dependency, Node version/platform/
architecture, pnpm version, and selected Node/locale environment variables. The current tests use mock
Unix sockets and their own sources; they do not start emulator daemons. New helper imports or filesystem
reads require an input review before relying on a cache hit.

The declared Rust inputs include all dkneeds files (including committed `src/gen_limits.rs`), SDK format source,
workspace manifests and lockfile, tracked root Cargo config, tool pins, the existing benchlock helpers
and Python dependency metadata, pinned rustc/Cargo identity, and selected Rust flags/wrapper/test
environment variables. All workspace manifests are intentionally conservative because Cargo resolves
the workspace even for a named package. The command compiles/tests only the dependency-free dkneeds
library and uses its own Cargo target directory. It does not regenerate SDK sources.

Rust caching stays disabled pending an input audit covering global Cargo configuration, linker/compiler
changes, arbitrary Cargo environment overrides and local dependency patches outside the checkout.
Its declared inputs are a starting point for that review, not a claim of complete closure. This pilot
tests Rust graph and execution compatibility independently of caching. Do not add inline global-config
hashing or a custom hasher to enable it.

The host pilot assumes installed JavaScript packages match the frozen lockfile. Keep installation fixed
during validation; use `--skipNxCache` after local package modifications and clear the local Nx cache
before trusting hits again. No cache correctness or speed claim is approved until the protocol below
passes. This is a local pilot, not a guarantee for remote or cross-machine cache reuse.

Runtime caching stays disabled. The named test reads ignored/external daemon binaries, firmware,
ready state, cards and prepared module artifacts. Tracking their filenames or a Git diff would not
prove their contents are unchanged. No attempt is made to hash that closure yet.

## Commands and prerequisites

After dependency installation and after PGO work has finished, from the repository root:

```sh
pnpm nx:pilot show project control-client
pnpm nx:pilot show project dkneeds
pnpm nx:pilot show project regression
pnpm nx:pilot graph --file=.nx/pilot-graph.html
pnpm nx:pilot run control-client:test-host
pnpm nx:pilot run dkneeds:test
pnpm nx:pilot run regression:service-commands
```

For a worktree reusing an existing Python environment, optionally set `UV_PROJECT_ENVIRONMENT` to
its absolute path and `UV_NO_SYNC=1` to avoid synchronization, with
`UV_CACHE_DIR="$PWD/.nx/uv-cache"` for local uv artifacts. If `out` is not linked to the shared
repository, supply an absolute `BENCHLOCK_PATH` to that repository's existing `out/.benchlock` so the
Rust target coordinates with other builds. `RUSTUP_AUTO_INSTALL=0` prevents implicit toolchain
installation; put the pinned Node/pnpm binaries on `PATH`. These are execution prerequisites, not a
claim that the disabled Rust result cache covers those environment inputs.

The runtime command requires prebuilt, preflighted `DIGI_EMUD_DT2` and `DIGI_EMUD_DN2`, complete ready
inputs for DT2 OS 1.17 and DN2 OS 1.12, prepared `svc-echo.dkm` artifacts for both devices under
`out/regression/modules/<device>/` (or `DIGI_RUNTIME_MODULES`), and the DT2 card sample directory at
`out/runs/patchkit/samples`. The shared live helper requires that directory even for service commands.
See [regression prerequisites](../../tests/regression/README.md#prerequisites) and
[testing inputs](../TESTING.md#inputs). Prepare inputs using the existing owner workflows separately;
the Nx target invokes Vitest directly and never builds daemons, module artifacts, plusdrive or PGO.
Do not substitute `mise run test:runtime`: it also builds plusdrive.

## Validation and remaining work

The coordinator owns installation and the dependency-only lockfile update. Offline project discovery,
all three resolved targets and the exported project graph have passed inspection. Preserve the named
commands without workspace-wide Cargo tests, generated-core builds, implicit runtime preparation,
Cloud connections or added dependency tasks.

The following bounded protocol completed for `control-client:test-host` with fixed toolchains and
environment; config/lock/toolchain coverage was reviewed statically rather than individually probed:

1. Run normally once to execute tests and populate the local cache, then repeat normally. Record
   the task hash, fresh result and explicit cache-hit evidence; they must agree for the same inputs.
2. Change an unrelated Markdown file and repeat. Require the same hash and a local cache hit, then
   restore the file.
3. Introduce a reversible failing assertion in a control-client test. Require a new hash and an
   executed failure rather than a replayed pass; restore it and require the original cached result.
   Review config/lock/toolchain input coverage. Add another miss check or a `--skipNxCache` run only
   if that review or the observed results leave an unresolved concern. Keep fresh and cached evidence
   explicit. A performance claim requires the repository's approved paired measurement.

The resolved Rust target's `cache: false` and one functional run have been checked: only dkneeds
compiled in the isolated target directory and all ten tests passed, with `Cargo.lock` unchanged.
No Nx result cache entry was present for that run. Enabling Rust result caching remains a separate follow-up requiring Cargo input
closure review and the full hit/miss/fresh-agreement protocol above, including Rust source, manifest,
configuration and toolchain changes.

Inspect the resolved runtime target's `cache: false`, then run it once after prerequisites are ready
and the CPU budget is available. Confirm both firmware device cases actually execute. Review the
existing missing-prerequisite failures and ensure the target adds no hidden builds. This is a
functional acceptance check, not evidence of runtime cache safety.

Before integration, run the applicable formatting, path and routed merge checks once under the
coordinator's budget. Do not remove existing routing until the pilot is validated, a unit's full input
and acceptance contract has migrated, and that unit's old selection logic is explicitly retired.
Any failure of graph isolation, input invalidation or fresh/cached agreement blocks migration.
