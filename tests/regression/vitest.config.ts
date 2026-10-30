import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// Coupled daemons take two cores; admission belongs to the caller's shared budget.
// Stock audio (stock, fx-*) is distinct from patched-daemon runtime and modules.
// Vitest 3.2 reads only singleFork per project; worker caps are root-level.
const serial = { poolOptions: { forks: { singleFork: true } } };

export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)),
  test: {
    // Caps the shared pool; only stock runs files in parallel.
    maxWorkers: Number(process.env.REGRESSION_JOBS ?? 4),
    // REGRESSION_JUNIT_FILE (verify's failed-unit record) names the report; REGRESSION_JUNIT=1 alone keeps the default.
    reporters:
      process.env.REGRESSION_JUNIT === '1' || process.env.REGRESSION_JUNIT_FILE
        ? [
            'default',
            [
              'junit',
              {
                outputFile:
                  process.env.REGRESSION_JUNIT_FILE ??
                  '../../out/regression/junit.xml',
              },
            ],
          ]
        : ['default'],
    projects: [
      {
        test: {
          name: 'stock',
          include: ['dt2/**/*.test.ts'],
          pool: 'forks',
          testTimeout: 120_000,
          hookTimeout: 120_000,
        },
      },
      {
        test: {
          name: 'runtime',
          include: ['runtime/**/*.test.ts'],
          pool: 'forks',
          ...serial,
          testTimeout: 300_000,
          hookTimeout: 150_000,
        },
      },
      {
        test: {
          name: 'modules',
          include: ['modules/**/*.test.ts'],
          pool: 'forks',
          ...serial,
          testTimeout: 300_000,
          hookTimeout: 150_000,
        },
      },
      {
        test: {
          name: 'fx-dn2',
          include: ['scenarios/dn2-*.test.ts'],
          pool: 'forks',
          ...serial,
          testTimeout: 30_000,
          hookTimeout: 1_900_000,
        },
      },
      {
        test: {
          name: 'fx-dt2',
          include: ['scenarios/dt2-fx.test.ts'],
          pool: 'forks',
          ...serial,
          testTimeout: 30_000,
          hookTimeout: 1_900_000,
        },
      },
    ],
  },
});
