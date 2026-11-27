import { renderContext } from './context.ts';
import { renderDetail } from './detail.ts';
import { el } from './element.ts';
import { renderErrors } from './errors.ts';
import { paintGlyph, renderOverview, sweepPosition } from './overview.ts';
import { initialState, type SetPayload, type ViewState } from './state.ts';

/** The app object: all page state lives here, nothing at module level. */
function createApp(doc: Document) {
  const $ = (id: string) => doc.getElementById(id)!;
  const s: ViewState = initialState();
  let fetchError = '';
  let animated: ReturnType<typeof renderOverview> = [];
  let raf = 0;

  const selectedGlyph = () => s.set?.glyphs.find((g) => g.id === s.selected);

  const tick = (ms: number) => {
    for (const a of animated) paintGlyph(a.canvas, a.glyph, sweepPosition(a.glyph, ms / 1000), s);
    raf = animated.length ? requestAnimationFrame(tick) : 0;
  };

  const toolbar = () => {
    const bar = $('toolbar');
    bar.replaceChildren();
    for (const n of [1, 2, 4, 8])
      bar.append(el('button', { class: s.scale === n ? 'on' : '', onclick: () => ((s.scale = n), render()) }, `${n}x`));
    const flag = (label: string, key: 'grid' | 'inverted' | 'animate') =>
      bar.append(el('button', { class: s[key] ? 'on' : '', onclick: () => ((s[key] = !s[key]), render()) }, label));
    flag('pixel grid', 'grid');
    flag('inverted', 'inverted');
    flag('animate', 'animate');
  };

  const render = () => {
    cancelAnimationFrame(raf);
    $('title').textContent = s.set ? `${s.set.title} (${s.set.glyphs.length} glyphs)` : 'Glyphs';
    renderErrors($('errors'), s.set, fetchError);
    toolbar();
    animated = renderOverview($('overview'), s, (id) => ((s.selected = id), render()));
    const g = selectedGlyph();
    const detail = $('detail');
    const holder = el('div', {});
    const context = el('div', {});
    detail.replaceChildren(holder, context);
    renderDetail(holder, g, s);
    renderContext(context, g);
    if (animated.length) raf = requestAnimationFrame(tick);
  };

  const load = async () => {
    try {
      const res = await fetch('/set.json', { cache: 'no-store' });
      const body = await res.json();
      if (!res.ok) throw new Error(body.error ?? res.statusText);
      s.set = body as SetPayload;
      fetchError = '';
      if (s.selected && !selectedGlyph()) s.selected = null;
    } catch (e) {
      fetchError = String(e); // keep the last good set on screen
    }
    render();
  };

  return { load };
}

const app = createApp(document);
void app.load();
if (import.meta.hot) import.meta.hot.on('glyphs:update', () => void app.load());
