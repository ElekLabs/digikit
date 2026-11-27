import { mountWorkbench } from './mount-workbench';
import { createWorkbenchEnvironment } from './workbench-environment';

const { host, apps, options } = createWorkbenchEnvironment();
mountWorkbench(host, apps, options);
