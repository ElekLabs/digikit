// The DSP mode of a device test file, derived from its source, and the check that every runtimeAcceptance record states it.
// A record says `"dsp": "sharc"` when its files start coupled sessions (ColdFire and SHARC: about 3 cores per emulator); a record
// without `dsp` is zero-DSP (about 1 core). tools/devtools/emu-slot.mjs uses the mode as the cores of a task (DIGI_EMU_DSP).
// The test files decide, not a list: a file is coupled when its source, or an exported helper it calls from its import closure,
// passes `dsp: 'sharc'` (or 'cold') to a session (packages/test-support/src/session.ts `resolveDsp` defaults to zero).
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const DSP_MODES = ['zero', 'sharc'];

// TypeScript is a dependency of tests/regression in the real checkout (as in ts-imports.mjs).
const ts = createRequire(fileURLToPath(new URL('../../tests/regression/package.json', import.meta.url)))('typescript');

// The top-level declarations of a source: [{ names, sharc, flags, refs, trueProps }]. `sharc`: the statement has a `dsp` property
// that is the string 'sharc' or 'cold' (also as the fallback of `??` or `||`). `flags`: the options a statement turns coupled with,
// from `dsp: opts.flag ? 'sharc' : 'zero'`. `refs`: the identifiers it mentions. `trueProps`: its `name: true` properties.
// Comments are no part of the syntax tree.
const COUPLED = /^['"](?:sharc|cold)['"]$/;
const parsed = new Map();
const declarations = (text) => {
  if (!parsed.has(text)) parsed.set(text, parse(text));
  return parsed.get(text);
};
const parse = (text) =>
  ts.createSourceFile('x.ts', text, ts.ScriptTarget.Latest, true).statements.map((st) => {
    const names = [];
    if (ts.isVariableStatement(st))
      for (const d of st.declarationList.declarations) if (ts.isIdentifier(d.name)) names.push(d.name.text);
    if ((ts.isFunctionDeclaration(st) || ts.isClassDeclaration(st)) && st.name) names.push(st.name.text);
    let sharc = false;
    const flags = new Set();
    const refs = new Set();
    const trueProps = new Set();
    const visit = (node) => {
      if (ts.isIdentifier(node)) refs.add(node.text);
      if (ts.isShorthandPropertyAssignment(node)) trueProps.add(node.name.text);
      if (ts.isPropertyAssignment(node)) {
        const name = node.name.getText();
        const value = node.initializer;
        if (value.kind === ts.SyntaxKind.TrueKeyword) trueProps.add(name);
        if (name === 'dsp') {
          if (ts.isConditionalExpression(value) && COUPLED.test(value.whenTrue.getText())) {
            const condition = ts.isPropertyAccessExpression(value.condition) ? value.condition.name.text : '';
            if (condition) flags.add(condition);
            else if (COUPLED.test(value.whenFalse.getText())) sharc = true; // other conditions pick between coupled modes or pass `dsp` on
          } else if (COUPLED.test(value.getText())) sharc = true;
          else if (ts.isBinaryExpression(value) && COUPLED.test(value.right.getText())) sharc = true;
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(st);
    return { names, sharc, flags, refs, trueProps };
  });

// Names of the declarations that start a coupled session, by fixpoint over the import closure `helperSources`, and the
// names that do so only with an option turned on: Map name -> Set of options.
const analyse = (helperSources) => {
  const all = helperSources.flatMap(declarations).filter((d) => d.names.length);
  const names = new Set();
  const optional = new Map();
  for (const d of all)
    for (const n of d.names) if (d.flags.size) optional.set(n, new Set([...(optional.get(n) ?? []), ...d.flags]));
  for (let grew = true; grew;) {
    grew = false;
    for (const d of all) {
      if (d.names.every((n) => names.has(n))) continue;
      if (d.sharc || [...d.refs].some((r) => names.has(r))) {
        for (const n of d.names) names.add(n);
        grew = true;
      }
    }
  }
  return { names, optional };
};
export const coupledHelpers = (helperSources) => analyse(helperSources).names;

// 'sharc' or 'zero'. `source`: the test file text; `helperSources`: the texts of the files it imports (its closure).
export const dspModeOf = (source, helperSources = []) => {
  const { names, optional } = analyse(helperSources);
  const coupled = declarations(source).some(
    (d) =>
      d.sharc ||
      [...d.refs].some((r) => names.has(r)) ||
      [...d.refs].some((r) => [...(optional.get(r) ?? [])].some((flag) => d.trueProps.has(flag))),
  );
  return coupled ? 'sharc' : 'zero';
};

// Derived mode of `file` (relative to project `root`). `importInputs`: the closure function of tools/nx/ts-imports.mjs.
export const fileDspMode = (workspaceRoot, root, file, importInputs) => {
  const self = join(root, file);
  const closure = importInputs([self])
    .map((p) => p.replace('{workspaceRoot}/', ''))
    .filter((p) => p !== self);
  const read = (p) => {
    try {
      return readFileSync(join(workspaceRoot, p), 'utf8');
    } catch {
      return '';
    }
  };
  return dspModeOf(read(self), closure.map(read));
};

// The mode a record declares: `dsp`, default zero.
export const recordMode = (rec) => rec.dsp ?? 'zero';

// Mode of one run target: coupled when any of its files' records says so.
export const taskMode = (tests, recs) =>
  recs.some((rec) => recordMode(rec) === 'sharc' && (rec.files ?? []).some((f) => tests.includes(f)))
    ? 'sharc'
    : 'zero';

// Throws when a record's `dsp` is no mode, or differs from the mode derived from a file it lists.
export const checkDspModes = (workspaceRoot, root, recs, importInputs) => {
  for (const rec of recs) {
    if (rec.dsp !== undefined && !DSP_MODES.includes(rec.dsp))
      throw new Error(
        `metadata.runtimeAcceptance of ${root}: dsp ${JSON.stringify(rec.dsp)} is not ${DSP_MODES.join(' or ')}`,
      );
    for (const file of rec.files ?? []) {
      const derived = fileDspMode(workspaceRoot, root, file, importInputs);
      if (derived !== recordMode(rec))
        throw new Error(
          `metadata.runtimeAcceptance of ${root}: ${file} starts ${derived} sessions (it or a helper it calls passes dsp: 'sharc'?), ` +
            `but its record has dsp ${JSON.stringify(recordMode(rec))}; set "dsp": ${JSON.stringify(derived)} (one record per mode)`,
        );
    }
  }
};
