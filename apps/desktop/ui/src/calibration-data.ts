/// <reference types="vite/client" />
// The data side of the Calibration section: the profile and step files of devices/calibration/ and the calibration.toml each module ships, bundled as
// text (the app has no checkout at run time). Parsed by `loadCalibrationFiles` (workbench-core/profile/calibration-view.ts).
import type { CalFileTexts } from '@digi/workbench-core/profile/calibration-view.ts';

const profiles = import.meta.glob('../../../../devices/calibration/profiles/*.toml', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
const steps = import.meta.glob('../../../../devices/calibration/steps/*.toml', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
const cases = import.meta.glob('../../../../modules/*/*/*/calibration.toml', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

export const calibrationTexts = (): CalFileTexts => ({ profiles, steps, cases });
