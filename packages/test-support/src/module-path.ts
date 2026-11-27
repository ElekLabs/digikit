import { groupOf, modulePath } from '@digi/workbench-core/modules/module-path.ts';

/**
 * The store path of a packed module: `modules/<kind>/<author>-<name>.dkm`, the rule of Transfer (`modulePath`), the kind from the records of the file's bytes.
 * `fileName` is the name a test chooses (`modules/<kind>/<fileName>.dkm`).
 */
export const moduleStorePathOf = (id: string, bytes: Uint8Array, fileName?: string): string =>
  modulePath(id, groupOf(bytes), fileName);
