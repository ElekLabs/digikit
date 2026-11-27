// A schema in the shape `patchkit workbench-ts` generates (`as const`).
export const schema = {
  module: 'test/typed',
  version: 2,
  settings: [
    {
      id: 'gain',
      label: 'Gain',
      kind: 'integer',
      apply: 'immediate',
      min: -10,
      max: 10,
      step: 2,
      decimals: 1,
      unit: 'dB',
      default: 0,
    },
    { id: 'flag', label: 'Flag', kind: 'boolean', apply: 'next-restart', default: true },
    {
      id: 'mode',
      label: 'Mode',
      kind: 'choice',
      apply: 'immediate',
      choices: ['Off', 'Soft', 'Hard'],
      default: 'Soft',
    },
    { id: 'name', label: 'Name', kind: 'text', apply: 'immediate', maxBytes: 8 },
    {
      id: 'anim',
      label: 'Animation',
      kind: 'file-name',
      apply: 'next-restart',
      maxBytes: 47,
      extension: 'dkbs',
      resetLabel: 'Stock',
    },
  ],
} as const;
