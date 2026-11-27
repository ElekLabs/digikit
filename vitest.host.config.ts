import { fileURLToPath } from 'node:url';
import solid from 'vite-plugin-solid';
import { configDefaults, defineConfig } from 'vitest/config';
// The Nx test-host targets read this file; host tests are device-free.
import projects from './vitest.host.projects.json' with { type: 'json' };

const root = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  root,
  test: {
    maxWorkers: 1,
    // desktop-host: the Solid client build (the server build runs no effects), for the tests of reactive code
    projects: projects.map(
      (p: {
        name: string;
        dir: string;
        include: string;
        exclude?: string[];
        conditions?: string[];
        solid?: boolean;
        alias?: Record<string, string>;
        testTimeout?: number;
      }) => ({
        // desktop-host compiles the .tsx files its tests render (a page, a view) with Solid.
        ...(p.solid ? { plugins: [solid()] } : {}),
        // Module tests reach workspace packages by path.
        resolve: {
          alias: {
            '@digi/dkbs': fileURLToPath(new URL('packages/dkbs/src/index.ts', import.meta.url)),
            '@digi/sharc-rig-kit/suite': fileURLToPath(new URL('packages/sharc-rig-kit/src/suite.ts', import.meta.url)),
            '@digi/sharc-rig-kit': fileURLToPath(new URL('packages/sharc-rig-kit/src/index.ts', import.meta.url)),
            // the aliases of the project's tsconfig (paths), relative to the repository root
            ...Object.fromEntries(
              Object.entries(p.alias ?? {}).map(([k, v]) => [k, fileURLToPath(new URL(v, import.meta.url))]),
            ),
          },
          ...(p.conditions ? { conditions: p.conditions } : {}),
        },
        ...(p.conditions ? { ssr: { resolve: { conditions: p.conditions } } } : {}),
        test: {
          ...(p.conditions ? { server: { deps: { inline: ['solid-js'] } } } : {}),
          name: p.name,
          // the Solid plugin defaults tests to jsdom; a test that needs a DOM names happy-dom in its own docblock
          ...(p.solid ? { environment: 'node' } : {}),
          root: fileURLToPath(new URL(p.dir, import.meta.url)),
          include: [p.include],
          exclude: [...configDefaults.exclude, '**/*.device.test.ts', ...(p.exclude ?? [])],
          // a project that runs the command lines or loads wasm names its own limit
          testTimeout: p.testTimeout ?? 5000,
        },
      }),
    ),
  },
});
