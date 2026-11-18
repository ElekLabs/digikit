// The DSP mode of a device test file, derived from its source, and the check that every runtimeAcceptance record states it.
// A record says `"dsp": "sharc"` when its files start coupled sessions (ColdFire and SHARC: about 3 cores per emulator); a record
// without `dsp` is zero-DSP (about 1 core). tools/devtools/emu-slot.mjs uses the mode as the cores of a task (DIGI_EMU_DSP).
// The test files decide, not a list: a file is coupled when its source, or an exported helper it calls from its import closure,
// passes `dsp: 'sharc'` (or 'cold') to a session (packages/test-support/src/session.ts `resolveDsp` defaults to zero). A `dsp`
// value that is not a literal or a pass-through counts as coupled: unknown is slower, never under-counted.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { closureFiles } from './ts-imports.mjs';

export const DSP_MODES = ['zero', 'sharc'];

// TypeScript is a dependency of tests/regression in the real checkout (as in ts-imports.mjs).
const ts = createRequire(fileURLToPath(new URL('../../tests/regression/package.json', import.meta.url)))('typescript');

// The top-level declarations of a source: [{ names, sharc, flags, refs, props }]. `sharc`: the statement has a `dsp` property
// that can be coupled (see `dspValue`). `flags`: the options a statement turns coupled with, from
// `dsp: opts.flag ? 'sharc' : 'zero'` or `opts.flag === 'x' ? ...`: [{ name, value }] (value undefined: any truthy value).
// `refs`: the identifiers it mentions. `props`: its properties as [{ name, value }] (value: source text, or undefined for a
// shorthand or an expression we cannot read; those count as set).
// Fail-safe: a `dsp` value that is not a string literal and not a pass-through of the caller's own `dsp` counts as coupled
// (unknown is slower, never under-counted). Comments are no part of the syntax tree.
const COUPLED = /^['"](?:sharc|cold)['"]$/;
const ZERO = /^['"]zero['"]$/;
const PASS = (node) =>
  (ts.isPropertyAccessExpression(node) && node.name.text === 'dsp') ||
  (ts.isIdentifier(node) && node.text === 'dsp') ||
  // packages/test-support/src/session.ts: resolveDsp is the one place that defaults an omitted `dsp` (to zero)
  (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'resolveDsp');

// 'coupled' | 'none' for the value of a `dsp` property; conditions on an option go to `flags` instead.
const dspValue = (node, flags) => {
  if (ts.isParenthesizedExpression(node)) return dspValue(node.expression, flags);
  if (ts.isStringLiteralLike(node))
    return COUPLED.test(node.getText()) ? 'coupled' : ZERO.test(node.getText()) ? 'none' : 'coupled';
  if (PASS(node)) return 'none';
  if (ts.isBinaryExpression(node) && ['??', '||'].includes(node.operatorToken.getText()))
    return dspValue(node.left, flags) === 'coupled' || dspValue(node.right, flags) === 'coupled' ? 'coupled' : 'none';
  if (ts.isConditionalExpression(node)) {
    const test = node.condition;
    const flag = ts.isPropertyAccessExpression(test)
      ? { name: test.name.text }
      : ts.isBinaryExpression(test) &&
          test.operatorToken.kind === ts.SyntaxKind.EqualsEqualsEqualsToken &&
          ts.isPropertyAccessExpression(test.left)
        ? { name: test.left.name.text, value: test.right.getText() }
        : undefined;
    const yes = dspValue(node.whenTrue, flags);
    const no = dspValue(node.whenFalse, flags);
    if (flag && no === 'none') {
      if (yes === 'coupled') flags.push(flag);
      return 'none';
    }
    return yes === 'coupled' || no === 'coupled' ? 'coupled' : 'none';
  }
  return 'coupled';
};

// Content-keyed parse cache of the process, shared by every caller (checkDspModes and the tests).
const parsed = new Map();
const declarations = (text) => {
  const key = createHash('sha1').update(text).digest('hex');
  if (!parsed.has(key)) parsed.set(key, parse(text));
  return parsed.get(key);
};
const parse = (text) =>
  ts.createSourceFile('x.ts', text, ts.ScriptTarget.Latest, true).statements.map((st) => {
    const names = [];
    if (ts.isVariableStatement(st))
      for (const d of st.declarationList.declarations) if (ts.isIdentifier(d.name)) names.push(d.name.text);
    if ((ts.isFunctionDeclaration(st) || ts.isClassDeclaration(st)) && st.name) names.push(st.name.text);
    let sharc = false;
    const flags = [];
    const refs = new Set();
    const props = [];
    const visit = (node) => {
      if (ts.isIdentifier(node)) refs.add(node.text);
      if (ts.isShorthandPropertyAssignment(node)) props.push({ name: node.name.text });
      if (ts.isPropertyAssignment(node)) {
        const name = node.name.getText();
        const value = node.initializer;
        props.push({
          name,
          value:
            ts.isLiteralExpression(value) ||
            value.kind === ts.SyntaxKind.TrueKeyword ||
            value.kind === ts.SyntaxKind.FalseKeyword
              ? value.getText()
              : undefined,
        });
        if (name === 'dsp' && dspValue(value, flags) === 'coupled') sharc = true;
      }
      ts.forEachChild(node, visit);
    };
    visit(st);
    return { names, sharc, flags, refs, props };
  });

// Whether `props` (of a caller) turn the option `flag` on: set to anything but a false value, or to its value.
const turnsOn = (props, flag) =>
  props.some(
    (p) =>
      p.name === flag.name &&
      (p.value === undefined ||
        (flag.value === undefined ? !['false', 'null', 'undefined', '0'].includes(p.value) : p.value === flag.value)),
  );

const analyse = (helperSources) => {
  const all = helperSources.flatMap(declarations).filter((d) => d.names.length);
  const names = new Set();
  const optional = new Map();
  for (const d of all)
    for (const n of d.names) if (d.flags.length) optional.set(n, [...(optional.get(n) ?? []), ...d.flags]);
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
      [...d.refs].some((r) => (optional.get(r) ?? []).some((flag) => turnsOn(d.props, flag))),
  );
  return coupled ? 'sharc' : 'zero';
};

// Derived mode of `file` (relative to project `root`). `importInputs`: the closure function of tools/nx/ts-imports.mjs.
// `texts`: a path -> text cache owned by the caller.
export const fileDspMode = (workspaceRoot, root, file, importInputs, texts = new Map()) => {
  const self = join(root, file);
  const closure = closureFiles(importInputs, self).filter((p) => p !== self);
  const read = (p) => {
    if (texts.has(p)) return texts.get(p);
    let text;
    try {
      text = readFileSync(join(workspaceRoot, p), 'utf8');
    } catch {
      text = '';
    }
    texts.set(p, text);
    return text;
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
  const texts = new Map(); // one read per file per call
  for (const rec of recs) {
    if (rec.dsp !== undefined && !DSP_MODES.includes(rec.dsp))
      throw new Error(
        `metadata.runtimeAcceptance of ${root}: dsp ${JSON.stringify(rec.dsp)} is not ${DSP_MODES.join(' or ')}`,
      );
    for (const file of rec.files ?? []) {
      const derived = fileDspMode(workspaceRoot, root, file, importInputs, texts);
      if (derived !== recordMode(rec))
        throw new Error(
          `metadata.runtimeAcceptance of ${root}: ${file} starts ${derived} sessions (it or a helper it calls passes dsp: 'sharc'?), ` +
            `but its record has dsp ${JSON.stringify(recordMode(rec))}; set "dsp": ${JSON.stringify(derived)} (one record per mode)`,
        );
    }
  }
};
