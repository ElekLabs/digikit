import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const ui = (path: string) => fileURLToPath(new URL(`../${path}`, import.meta.url));

function sources(dir: string): string[] {
  return readdirSync(ui(dir), { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory()
      ? sources(`${dir}/${entry.name}`)
      : /\.(ts|tsx)$/.test(entry.name)
        ? [readFileSync(ui(`${dir}/${entry.name}`), 'utf8')]
        : [],
  );
}

describe('the preview native boundary', () => {
  it('replaces every Tauri API module the app imports, so no call reaches Tauri internals in a browser', () => {
    const imported = new Set(
      sources('src').flatMap((text) => [...text.matchAll(/from '(@tauri-apps\/[^']+)'/g)].map((m) => m[1]!)),
    );
    const config = readFileSync(ui('preview/vite.config.ts'), 'utf8');
    const missing = [...imported].filter((name) => !config.includes(`find: '${name}'`));
    expect(missing).toEqual([]);
  });
});
