import { For, Match, Switch, createSignal, onCleanup } from 'solid-js';
import type { ModuleClient } from '@digi/module-ui';
import { dkpSpans, kindAt, type SpanKind } from './dkp-spans';
import { ProjectManager } from './gen/project_manager_pb.js';
import { readProjectFile, type FileRead } from './project-file';

/** Bytes of one line of the hex view. */
export const LINE_BYTES = 16;

const hexByte = (byte: number) => byte.toString(16).padStart(2, '0');
const offsetText = (offset: number) => offset.toString(16).padStart(4, '0').toUpperCase();

interface Cell {
  hex: string;
  kind: SpanKind | undefined;
}
interface Line {
  offset: number;
  cells: Cell[];
}

/** The lines of the hex view: each byte with the kind of the section it belongs to (none for a file this page does not know). */
export function hexLines(bytes: Uint8Array): Line[] {
  const spans = dkpSpans(bytes);
  const lines: Line[] = [];
  for (let offset = 0; offset < bytes.length; offset += LINE_BYTES) {
    const cells: Cell[] = [];
    for (let at = offset; at < Math.min(offset + LINE_BYTES, bytes.length); at++) {
      cells.push({ hex: hexByte(bytes[at]), kind: spans && kindAt(spans, at) });
    }
    lines.push({ offset, cells });
  }
  return lines;
}

/** The raw bytes of a project's file in hex with byte offsets, coloured by section. All pages are fetched when the view is created; it stops when it is removed. */
export default function ProjectRaw(props: { client: ModuleClient; slot: number }) {
  // oxlint-disable-next-line solid/reactivity -- the view is made for one project and removed when the selection changes
  const { client, slot } = props;
  const service = client.service(ProjectManager);
  const [read, setRead] = createSignal<FileRead>();
  let active = true;
  onCleanup(() => {
    active = false;
  });
  void readProjectFile(service, slot, () => active).then((result) => {
    if (result) setRead(result);
  });

  const file = () => {
    const now = read();
    return now?.state === 'ok' ? now.file : undefined;
  };
  const failure = () => {
    const now = read();
    return now?.state === 'error' ? now.text : undefined;
  };

  return (
    <div class="pm-raw">
      <Switch>
        <Match when={read() === undefined}>
          <p class="empty" role="status">
            Loading…
          </p>
        </Match>
        <Match when={read()?.state === 'missing'}>
          <p class="empty">The device has no file for this project.</p>
        </Match>
        <Match when={failure()}>
          {(text) => (
            <p class="pm-error" role="alert">
              {text()}
            </p>
          )}
        </Match>
        <Match when={file()}>
          {(found) => (
            <pre class="pm-hex" aria-label="File bytes">
              <For each={hexLines(found().bytes)}>
                {(line) => (
                  <div class="pm-hex-line">
                    <span class="pm-hex-offset">{offsetText(line.offset)}</span>
                    <For each={line.cells}>
                      {(cell) => <span class={cell.kind ? `pm-byte pm-k-${cell.kind}` : 'pm-byte'}>{cell.hex}</span>}
                    </For>
                  </div>
                )}
              </For>
            </pre>
          )}
        </Match>
      </Switch>
    </div>
  );
}
