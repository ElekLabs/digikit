import { render } from 'solid-js/web';
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@web/styles/site.css';
import './console.css';
import './native.css';
import './desktop-env';
import Console from './Console';
import { installNativeFeel } from './native';

installNativeFeel();

render(() => <Console />, document.getElementById('app')!);
