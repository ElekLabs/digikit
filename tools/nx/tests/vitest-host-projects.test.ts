import { globSync, readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const root = resolve(import.meta.dirname, '../../..');
const entries = readdirSync(join(root, 'vitest.host'))
  .filter((f) => f.endsWith('.json'))
  .map((f) => f.slice(0, -'.json'.length));

type Target = { inputs?: unknown[]; options?: { command?: string } };

// Every project.json target that runs `vitest --project NAME` names `{workspaceRoot}/vitest.host/NAME.json` as an input, so its
// key follows its own entry and no other.
const vitestTargets = () => {
  const found: { file: string; target: string; project: string; inputs: unknown[] }[] = [];
  for (const file of globSync('{,*/,*/*/,tests/*/}project.json', { cwd: root })) {
    const targets: Record<string, Target> = JSON.parse(readFileSync(join(root, file), 'utf8')).targets ?? {};
    for (const [name, t] of Object.entries(targets)) {
      const m = /vitest run .*--project ([\w-]+)/.exec(t.options?.command ?? '');
      if (m) found.push({ file, target: name, project: m[1], inputs: t.inputs ?? [] });
    }
  }
  return found;
};

describe('vitest.host project files', () => {
  it('names each entry after its file', () => {
    for (const name of entries) {
      expect(JSON.parse(readFileSync(join(root, 'vitest.host', `${name}.json`), 'utf8')).name).toBe(name);
    }
  });

  it('is listed by every target that runs the project, and by no other project entry', () => {
    const targets = vitestTargets();
    expect(targets.length).toBeGreaterThan(10);
    for (const t of targets) {
      const own = `{workspaceRoot}/vitest.host/${t.project}.json`;
      expect(entries, `${t.file}:${t.target} runs unknown project ${t.project}`).toContain(t.project);
      expect(t.inputs, `${t.file}:${t.target} must list ${own}`).toContain(own);
      const others = t.inputs.filter(
        (i) => typeof i === 'string' && i.startsWith('{workspaceRoot}/vitest.host/') && i !== own,
      );
      expect(others, `${t.file}:${t.target} lists another project's entry`).toEqual([]);
    }
  });
});
