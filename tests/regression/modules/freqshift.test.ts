import { runtimeModuleOf } from './acceptance.ts';
import { moduleLoadTests } from './suite.ts';

moduleLoadTests(runtimeModuleOf(import.meta.url));
