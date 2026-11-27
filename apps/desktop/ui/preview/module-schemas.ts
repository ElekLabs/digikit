import type { DescribedSchema } from '@digi/module-ui';

type Generated = { schema: DescribedSchema & { module: string } };

// The settings each module declares in workbench.toml, as patchkit generated them (id and name included). The fixtures' samples exist
// only here and in tests.
const shipped = import.meta.glob<Generated>('../../../../modules/*/*/*/workbench/gen/settings.ts', { eager: true });
const samples = import.meta.glob<Generated>('../../../../tests/fixtures/modules/*/workbench/gen/settings.ts', {
  eager: true,
});

/** The generated schema of every module the preview can load, by module id. */
export function previewSettingsSchemas() {
  return new Map([...Object.values(shipped), ...Object.values(samples)].map(({ schema }) => [schema.module, schema]));
}

/** The ids of the sample modules the preview seeds on its card. */
export const previewSampleIds = Object.values(samples).map(({ schema }) => schema.module);
