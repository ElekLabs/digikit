import { wavePlot, sine } from './shape.mjs';
export default { title: 'Sine', size: 'knob', tags: ['wave'], draw: (p) => wavePlot(p, sine) };
