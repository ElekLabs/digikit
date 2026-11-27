import type { MemoryQueryService } from '@digi/module-ui';

/** A fixture's preview file: the module's query service from the settings as the instance read them when it loaded. */
export type PreviewQueryFactory = (loaded: (settingId: string) => Uint8Array | undefined) => MemoryQueryService;

const previews = import.meta.glob<{ default: PreviewQueryFactory }>(
  '../../../../tests/fixtures/modules/*/workbench/preview.ts',
  {
    eager: true,
  },
);
const schemas = import.meta.glob<{ schema: { module: string } }>(
  '../../../../tests/fixtures/modules/*/workbench/gen/settings.ts',
  {
    eager: true,
  },
);
// A shipped module's preview file belongs to the module of the `schema` it exports (a module without settings), else of its generated settings.
const shippedPreviews = import.meta.glob<{ default: PreviewQueryFactory; schema?: { module: string } }>(
  '../../../../modules/*/*/*/workbench/preview.ts',
  { eager: true },
);

/** The preview query services by module id: a fixture's `workbench/preview.ts` belongs to the module of its `workbench/gen/settings.ts`. */
export function previewQueryServices(): Map<string, PreviewQueryFactory> {
  const services = new Map<string, PreviewQueryFactory>();
  for (const [path, file] of Object.entries(previews)) {
    const schema = schemas[path.replace(/preview\.ts$/, 'gen/settings.ts')];
    if (schema) services.set(schema.schema.module, file.default);
  }
  for (const file of Object.values(shippedPreviews)) if (file.schema) services.set(file.schema.module, file.default);
  return services;
}
