// The Transfer commands of wb (tools/devtools/wb.mjs): the Transfer pane's install and delete, through the pane's own code
// (packages/workbench-core/src/modules/dktransfer.ts). `install` resolves the services each module needs, binds them to your stock firmware and
// uploads them before the modules; `delete` takes the services no remaining module needs along with the modules. No child process.
//
//     wb [--port SUB | --emu [--run-dir DIR]] transfer install ID... [--stock STOCK.syx] [--restart] [--library DIR]
//     wb [--port SUB | --emu [--run-dir DIR]] transfer restart
//     wb [--port SUB | --emu [--run-dir DIR]] transfer delete PATH...
//
// ID is a module id of the library (`out/modules/<device>/`, `dist/modules/<device>/` for the `.dks` services); PATH is a card path (`modules/pluck.dkm`).
// `--stock` is the stock firmware .syx of the device: it is what a service that hooks the firmware is bound to. The default is the harness's stock image of the
// device (`stockImage`: dt2 1.17, dn2 1.12 in $DIGI_FIRMWARE_DIR or firmware/); when that file is missing a module that needs such a service is refused (the others install).
// `--restart` then selects the installed modules (the selection is exactly these), restarts the device, waits for the boot and checks that MODULES lists them (exit 1 when
// one is missing). `restart` alone restarts with the selection the device has. `--library DIR` reads the `.dkm` files from DIR/<device>/ instead of out/modules/<device>/. One JSON object per event, like the pane's progress (`--format text` prints the pane's lines).
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { fileHashHex } from '../../packages/dk-session/src/index.ts';
import { DkNeeds } from '../../packages/workbench-core/src/modules/dkneeds.ts';
import { DkBind, createServiceBinder } from '../../packages/workbench-core/src/modules/dkbind.ts';
import { RUNTIME_SERVICE_NAMES } from '../../packages/dk-session/src/gen_proto.ts';
import {
  SERVICE_EXT,
  deleteFiles,
  install,
  libraryModule,
  reloadModules,
  restartDevice,
} from '../../packages/workbench-core/src/modules/dktransfer.ts';
import { nameHash, serviceFileOf } from '../../packages/workbench-core/src/modules/services.ts';
import { stockImage } from '../../packages/workbench-core/src/node/stock-image.ts';
import { eventText } from '../../packages/workbench-core/src/transfer/transfer-core.ts';

/** The library of a device: the built modules and the unbound services (the files the Transfer pane lists). */
async function loadLibrary(host, device, needs) {
  const modules = [];
  const services = [];
  for (const f of await host.library.scan(device)) {
    const bytes = new Uint8Array(await host.library.read(device, f.path));
    if (f.path.endsWith(SERVICE_EXT)) {
      const stem = (f.path.split('/').pop() ?? f.path).slice(0, -SERVICE_EXT.length);
      const s = serviceFileOf(stem, bytes, fileHashHex(bytes));
      if (s) services.push(s);
    } else {
      const m = await libraryModule(bytes, (b) => needs.of(b));
      if (m) modules.push(m);
    }
  }
  return { modules, services };
}

export async function transferCommand({ host, root: repoRoot, session, sub, rest, opts, fail }) {
  // an emulator on a busy host boots slowly in real time (DN2 more than DT2)
  const bootTimeoutMs = opts.emu ? 600_000 : undefined;
  const emit = (e) => console.log(opts.format === 'text' ? eventText(e) : JSON.stringify(e));
  if (sub === 'delete') {
    if (rest.length === 0) fail('transfer delete needs card paths');
    await deleteFiles(session, rest, emit);
    return 0;
  }
  if (sub === 'restart') {
    const { loaded } = await restartDevice(session, emit, {
      bootTimeoutMs,
      loadedTimeoutMs: bootTimeoutMs ? 120_000 : undefined,
    });
    emit({ event: 'loaded', loaded, missing: [] });
    return 0;
  }
  if (sub === 'reload') {
    // the new files are on the card already (wb transfer install): reload the loaded modules without a boot
    if (rest.length === 0) fail('transfer reload needs card paths');
    const { refused, failed } = await reloadModules(session, rest, emit);
    for (const r of refused)
      console.error(`wb: ${r.path} was not reloaded (status ${r.result}); the old module still runs`);
    for (const r of failed)
      console.error(
        `wb: the reload of ${r.path} failed after the old module was removed (status ${r.result}); the module is not loaded`,
      );
    return refused.length + failed.length ? 1 : 0;
  }
  if (sub !== 'install' || rest.length === 0)
    fail(
      'usage: wb transfer install ID... [--stock STOCK.syx] [--restart] [--reload] | wb transfer reload PATH... | wb transfer restart | wb transfer delete PATH...',
    );
  const info = await session.info();
  const wasm = new Uint8Array(await host.firmware.patchkitWasm());
  const needs = await DkNeeds.create(wasm);
  const { modules, services } = await loadLibrary(host, info.port, needs);
  const stockFile = opts.stock ?? defaultStock(info.port, repoRoot);
  const stock = stockFile ? new Uint8Array(readFileSync(stockFile)) : undefined;
  const stockHash = stock ? createHash('sha256').update(stock).digest('hex') : null;
  const binder = createServiceBinder(
    async () => DkBind.create(wasm, new Uint8Array(await host.firmware.bindBundle())),
    { bytesOf: async (h) => (h === stockHash ? stock : undefined) },
  );
  const deps = {
    services,
    binder,
    firmwareHash: stockHash,
    runtimeIdentity: (i) => ({ version: i.runtime, fingerprint: i.svc_abi }),
    runtimeServices: new Set(RUNTIME_SERVICE_NAMES.map(nameHash)),
    cache: new Map(),
    needsOf: (b) => needs.of(b),
  };
  const refusedAll = (r) => r.refused?.length && r.uploaded.length === 0 && rest.length === r.refused.length;
  const r = await install(session, rest, modules, emit, deps);
  if (!opts.restart && !opts.reload) return refusedAll(r) ? 1 : 0;
  const refused = new Set((r.refused ?? []).map((x) => x.module));
  const chosen = rest.filter((id) => !refused.has(id));
  if (chosen.length === 0) return 1;
  const chosenPaths = chosen.map((id) => modules.find((m) => m.id === id).path);
  // the selection is what the next boot loads: it names every chosen module whether it is reloaded now or restarted
  await session.select(chosenPaths);
  let toBoot = chosen;
  if (opts.reload) {
    // a module that is loaded and can be reloaded is swapped without a boot. The others go through the restart below, which loads the selection: a module that was refused
    // (not loaded, the transport plays, a key was dropped; the old one still ran) and one whose reload failed after the old one was removed (it is not loaded now); the events say which
    const { refused, failed } = await reloadModules(session, chosenPaths, emit);
    const back = new Set([...refused, ...failed].map((f) => f.path));
    toBoot = chosen.filter((id) => back.has(modules.find((m) => m.id === id).path));
    if (toBoot.length === 0) return 0;
  }
  const { loaded } = await restartDevice(session, emit, {
    bootTimeoutMs,
    loadedTimeoutMs: bootTimeoutMs ? 120_000 : undefined,
  });
  const missing = toBoot.filter((id) => !loaded.includes(id));
  emit({ event: 'loaded', loaded, missing });
  return missing.length ? 1 : 0;
}

/** The stock image of the device (the harness's file, `stockImage`). A missing file says where it looked; refused modules follow. */
function defaultStock(device, root) {
  const file = stockImage(device, root);
  if (existsSync(file)) return file;
  console.error(
    `wb: the stock firmware of ${device} is not at ${file} (set $DIGI_FIRMWARE_DIR or pass --stock FILE.syx): a module that needs a service is refused`,
  );
  return undefined;
}
