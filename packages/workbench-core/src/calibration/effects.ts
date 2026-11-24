import type { DkModule, DkSession } from '@digi/dk-session';
import { parseMachineRef } from './rigsetup.ts';

export type EffectRoute = 'master' | 'send';
export type ExpectedEffectRoute = 'default' | EffectRoute;

export interface DispatchRow {
  module: string;
  kind: 'machine' | 'effect';
  row: number;
  route?: EffectRoute;
}

export function isContentModule(module: DkModule): boolean {
  return module.rows !== 0 || module.types.length !== 0 || !!module.effects?.length;
}

export function moduleDispatch(modules: DkModule[], ids: string[]): DispatchRow[] {
  return modules
    .filter(({ name }) => ids.includes(name))
    .flatMap((module) => {
      const rows: DispatchRow[] = [];
      for (let row = 0; row < 8; row++) {
        if (module.rows & (1 << row)) rows.push({ module: module.name, kind: 'machine', row });
      }
      for (const effect of module.effects ?? []) {
        const route = effect.running === 1 ? 'master' : effect.running === 2 ? 'send' : undefined;
        if (route && effect.row < 8) rows.push({ module: module.name, kind: 'effect', row: 8 + effect.row, route });
      }
      return rows;
    });
}

export function sessionEffects(session: Pick<DkSession, 'modules'>) {
  return async (reference: string, route: ExpectedEffectRoute): Promise<DispatchRow> => {
    const { id, n } = parseMachineRef(reference, 'effect');
    const modules = await session.modules();
    const module = modules.find(({ name }) => name === id);
    const effect = module?.effects?.[n - 1];
    if (!effect) throw new Error('No loaded effect #' + n + ' for ' + id + '.');
    const expected = route === 'default' ? effect.dflt : route === 'master' ? 1 : 2;
    const expectedName = expected === 1 ? 'master' : expected === 2 ? 'send' : undefined;
    const observed = moduleDispatch(modules, [id]).find((row) => row.kind === 'effect' && row.row === effect.row + 8);
    if (!observed || !expectedName || observed.route !== expectedName || effect.saved !== effect.running) {
      const active = observed?.route ?? 'inactive';
      throw new Error(
        id +
          ' needs the ' +
          (expectedName ?? route) +
          ' route. Read-back is ' +
          active +
          '. Check FX ROUTING and wait for any reload to finish.',
      );
    }
    return observed;
  };
}
