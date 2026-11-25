import fs from 'node:fs';
import type { Session } from '@digi/test-support';
import { readLoadTable } from './addrmap.ts';
import { planSymbol, rememberShipMap, shipMapOf, type RuntimePlan } from './runtime-plan.ts';
import { serviceMapPath } from './service-files.ts';

/** What a lookup needs of a runtime: its session (`PatchedSession`, `TestRuntime` and `Rt` all have it). */
type WithSession = { readonly session: Session };

export { rememberShipMap };

/** How a lookup finds the service: the ship map of the image, which the load table is decoded with. */
export interface LookupOptions {
  shipMap?: string;
}

/**
 * The load address of service `service`: its record in the device load table (`readLoadTable`, docs/ADDRMAP.md), found by the last part of the module id (a service module's id ends in the service name). The table
 * is read from the running device's memory, so it works while the daemon is paused. The test that asks is a live-patchkit task (it decodes the table with `patchkit addr`).
 */
async function loadAddress(
  runtime: WithSession,
  plan: RuntimePlan,
  service: string,
  opts: LookupOptions,
): Promise<number> {
  const shipMap = opts.shipMap ?? shipMapOf(plan);
  if (!shipMap)
    throw new Error(
      `no address map for the image of this plan: call rememberShipMap(plan, planFile) when the plan is prepared`,
    );
  const hit = (await readLoadTable(runtime.session, plan, shipMap)).find((l) => l.id.split('/').at(-1) === service);
  if (!hit)
    throw new Error(`service ${service} is not in the load table: the card has no ${service}.dks, or it was refused`);
  return hit.base;
}

/**
 * The symbols of the service as the card holds it (docs/ADDRMAP.md): name to offset from the load address. The service file loads as packed (the port file resolves its
 * firmware references at load and appends nothing to it), so the address map is the one of the pack (`dist/modules/<device>/<name>.addrmap.json`).
 */
export function serviceMapEntries(device: Session['options']['device'], service: string): Map<string, number> {
  const file = serviceMapPath(device, service);
  if (!fs.existsSync(file)) throw new Error(`${file}: run nx run core/services/${service}:pack`);
  const map = JSON.parse(fs.readFileSync(file, 'utf8')) as { entries: { cpu: string; addr: number; name: string }[] };
  return new Map(map.entries.filter((e) => e.cpu === 'cf').map((e) => [e.name, e.addr]));
}

/**
 * The address of `symbol` of a loaded service, e.g. `serviceSymbol(runtime, plan, 'fx', 'DKRT_FXP')`: the load address plus the offset in the service's address map.
 */
export async function serviceSymbol(
  runtime: WithSession,
  plan: RuntimePlan,
  service: string,
  symbol: string,
  opts: LookupOptions = {},
): Promise<number> {
  const entries = serviceMapEntries(runtime.session.options.device, service);
  const offset = entries.get(symbol);
  if (offset === undefined) throw new Error(`${symbol} is not in the address map of the ${service} service`);
  return (await loadAddress(runtime, plan, service, opts)) + offset;
}

/**
 * The count of nested calls the service's owner refused (a nested `with`, `run` or `step`, a full event queue: `Owner::refused`; not start or stop refusals, not calls before start or after stop): the first word of `DK_SVC_OWNER`. A scenario that ends
 * with a count above 0 had a call into the service land inside another call into it.
 */
export async function refusedCalls(
  runtime: WithSession,
  plan: RuntimePlan,
  service: string,
  opts: LookupOptions = {},
): Promise<number> {
  const at = await serviceSymbol(runtime, plan, service, 'DK_SVC_OWNER', opts);
  const reply = (await runtime.session.client.call('mem.cf.peek', { addr: at })) as { value: number };
  return reply.value >>> 0;
}

/** Assert that no nested call into the owner of `service` was refused in the scenario so far (what `Owner::refused` counts; start and stop refusals and calls before start or after stop are not counted). */
export async function expectNoRefusedCalls(
  runtime: WithSession,
  plan: RuntimePlan,
  service: string,
  opts: LookupOptions = {},
): Promise<void> {
  const n = await refusedCalls(runtime, plan, service, opts);
  if (n !== 0) throw new Error(`expected no nested call refused in the ${service} service owner, got ${n} (nested with/run/step or a full event queue)`);
}

/**
 * A name lookup for a test that reads the state of a service and of the image together: the names of the service's own symbols (`DKRT_FXP`, `DKRT_FXS`, the
 * `DKRT_FXP_*` statics; the old lower-case `fxp_*` names of the hook labels map to them) resolve through the service, every other name through the image's plan.
 * The base is read once: call it again after a reload that can move the service.
 */
export async function serviceSymbols(
  runtime: WithSession,
  plan: RuntimePlan,
  service: string,
  opts: LookupOptions = {},
): Promise<(name: string) => number> {
  const entries = serviceMapEntries(runtime.session.options.device, service);
  const base = await loadAddress(runtime, plan, service, opts);
  return (name) => {
    const own = entries.has(name) ? name : name.startsWith('fxp_') ? `DKRT_${name.toUpperCase()}` : name;
    const offset = entries.get(own);
    return offset === undefined ? planSymbol(plan, name) : base + offset;
  };
}

const bound = new WeakMap<object, (name: string) => number>();

/** Bind the symbols of `service` for the session after a boot (`inspectFxBoot`). A fixture that declares `noServices` has no service: pass `expected: false` and the
 * symbols are cleared. Otherwise a service that cannot be found is an error naming the service and the reason (no silent fallback to the image's plan). */
export async function bindServiceSymbols(
  runtime: WithSession,
  plan: RuntimePlan,
  service: string,
  opts: LookupOptions & { expected?: boolean } = {},
): Promise<void> {
  if (opts.expected === false) {
    bound.delete(runtime.session);
    return;
  }
  try {
    bound.set(runtime.session, await serviceSymbols(runtime, plan, service, opts));
  } catch (e) {
    bound.delete(runtime.session);
    throw new Error(
      `symbols of the ${service} service are not available: ${e instanceof Error ? e.message : String(e)}`,
    );
  }
}

/** True when the last boot of the session had the service (`bindServiceSymbols` bound its symbols): false for a boot without the service file, which has none of its cells. */
export function hasServiceSymbols(runtime: WithSession): boolean {
  return bound.has(runtime.session);
}

/** The name lookup of the last boot of the session (`bindServiceSymbols`), or the image's plan alone when no service was found. Symbols of the fx service: `DKRT_FXP`,
 * `DKRT_FXS`, `DKRT_FXP_TAB`, `DKRT_FXP_COUNT`, `DKRT_FXP_SEND_ROW`, ... (the old `fxp_*` names map to them). */
export function symbolsOf(runtime: WithSession, plan: RuntimePlan): (name: string) => number {
  return bound.get(runtime.session) ?? ((name) => planSymbol(plan, name));
}
