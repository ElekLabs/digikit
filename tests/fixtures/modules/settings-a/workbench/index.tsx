import { definePage } from '@digi/module-ui';
import { schema } from './gen/settings';
import Panel from './Panel';

export default definePage({ id: schema.module, api: 1, Page: Panel });
