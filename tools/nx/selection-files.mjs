// The gate's own selection files: the code and data that decide which device tests a change selects (docs/TESTING.md "Runtime areas").
// `device-test-keys.mjs` is not one: it defines the device test cache keys, so its changes select normally.
// No device run proves them; the selector unit tests (`workspace:test-host`, nx-selection) and a `mise run test:plan` replay do. A change
// confined to these files selects no device test and no smoke, and a test newly named in an area entry is not "changed" by that entry.
// This list is the one owner of the set; test-subset.mjs asks it.
export const SELECTION_FILES = [
  'tools/nx/runtime-areas.json',
  'tools/nx/runtime-areas.mjs',
  'tools/nx/test-subset.mjs',
  'tools/nx/device-owners.mjs',
  'tools/nx/gate-context.mjs',
  'tools/nx/port-entries.mjs',
  'tools/nx/selection-files.mjs',
];
export const SELECTION_DIRS = ['tools/nx/tests/'];

export const isSelectionFile = (file) =>
  SELECTION_FILES.includes(file) || SELECTION_DIRS.some((dir) => file.startsWith(dir));

/** Whether a non-empty change touches nothing but selection files. */
export const confinedToSelection = (changed) => changed.length > 0 && changed.every(isSelectionFile);

/**
 * The selection after the confinement rule: a diff confined to selection files drops the tests the areas and ownership chose and the
 * device files (and, in smokeSelected, the smoke set); tests the user named (`named`) always join. Pure: no I/O.
 * `selection.tests` are the automatic tests, `named` the user's; the result's `tests` is their union by project and file.
 */
export const applyConfinement = (selection, named, changed) => {
  const confined = confinedToSelection(changed);
  const seen = new Set();
  const key = (t) => `${t.project}:${t.file}`;
  const tests = [...(confined ? [] : selection.tests), ...named].filter((t) => !seen.has(key(t)) && seen.add(key(t)));
  return { ...selection, tests, deviceFiles: confined ? [] : selection.deviceFiles };
};
