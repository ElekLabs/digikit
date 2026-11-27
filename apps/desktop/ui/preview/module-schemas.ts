import type { DescribedSchema } from '@digi/module-ui';

type Generated = { schema: DescribedSchema & { module: string } };

// The settings each module declares in workbench.toml, as patchkit generated them (id and name included). The fixtures' samples exist
// only here and in tests.
const shipped = import.meta.glob<Generated>('../../../../modules/*/*/*/workbench/gen/settings.ts', { eager: true });
const samples = import.meta.glob<Generated>('../../../../tests/fixtures/modules/*/workbench/gen/settings.ts', {
  eager: true,
});

// A module without settings has no generated schema; its `workbench/preview.ts` exports the empty one the preview loads it with.
const previewOnly = Object.values(
  import.meta.glob<{ schema?: Generated['schema'] }>('../../../../modules/*/*/*/workbench/preview.ts', { eager: true }),
).flatMap((file) => (file.schema ? [file.schema] : []));

/** The generated schema of every module the preview can load, by module id. */
export function previewSettingsSchemas() {
  const all = [...Object.values(shipped).map((file) => file.schema), ...previewOnly];
  return new Map(
    [...all, ...Object.values(samples).map((file) => file.schema)].map((schema) => [schema.module, schema]),
  );
}

/** The ids of the modules the preview seeds on its card: the sample fixtures and the modules that declare their own preview. */
export const previewSampleIds = [
  ...Object.values(samples).map(({ schema }) => schema.module),
  ...previewOnly.map((schema) => schema.module),
];
