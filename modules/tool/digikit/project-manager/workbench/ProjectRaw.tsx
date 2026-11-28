import { For, Match, Switch, createSignal, onCleanup } from 'solid-js';
import type { ModuleClient } from '@digi/module-ui';
import { decodeDkp, kindAt, type Decoded, type Field, type SpanKind } from './dkp-spans';
import { ProjectManager } from './gen/project_manager_pb.js';
import { readProjectFile, type FileRead } from './project-file';

/** Bytes of one line of the hex view. */
export const LINE_BYTES = 16;

const hexByte = (byte: number) => byte.toString(16).padStart(2, '0');
const asciiChar = (byte: number) => (byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : '.');
const offsetText = (offset: number) => offset.toString(16).padStart(4, '0').toUpperCase();

interface Cell {
  offset: number;
  hex: string;
  ascii: string;
  kind: SpanKind | undefined;
}
interface Line {
  offset: number;
  cells: Cell[];
}

/** The lines of the hex view: each byte with the kind of the section it belongs to (none for a file this page does not know). */
export function hexLines(bytes: Uint8Array, decoded: Decoded | undefined = decodeDkp(bytes)): Line[] {
  const spans = decoded?.spans;
  const lines: Line[] = [];
  for (let offset = 0; offset < bytes.length; offset += LINE_BYTES) {
    const cells: Cell[] = [];
    for (let at = offset; at < Math.min(offset + LINE_BYTES, bytes.length); at++) {
      cells.push({ offset: at, hex: hexByte(bytes[at]), ascii: asciiChar(bytes[at]), kind: spans && kindAt(spans, at) });
    }
    lines.push({ offset, cells });
  }
  return lines;
}

/** The decoded fields grouped by section, in file order. */
function groups(fields: Field[]): { section: string; fields: Field[] }[] {
  const result: { section: string; fields: Field[] }[] = [];
  for (const field of fields) {
    const last = result[result.length - 1];
    if (last?.section === field.section) last.fields.push(field);
    else result.push({ section: field.section, fields: [field] });
  }
  return result;
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
  const [hovered, setHovered] = createSignal<{ start: number; end: number }>();
  const fieldAt = (fields: Field[], offset: number) => fields.find((f) => offset >= f.offset && offset < f.offset + f.length);
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
          {(found) => {
            const decoded = decodeDkp(found().bytes);
            const hot = (offset: number) => {
              const range = hovered();
              return range !== undefined && offset >= range.start && offset < range.end;
            };
            const hoverByte = (offset: number) => {
              const f = decoded && fieldAt(decoded.fields, offset);
              setHovered(f ? { start: f.offset, end: f.offset + f.length } : undefined);
            };
            return (
              <>
                <pre class="pm-hex" aria-label="File bytes" onMouseLeave={() => setHovered(undefined)}>
                  <For each={hexLines(found().bytes, decoded)}>
                    {(line) => (
                      <div class="pm-hex-line">
                        <span class="pm-hex-offset">{offsetText(line.offset)}</span>
                        <span class="pm-hex-cells">
                          <For each={line.cells}>
                            {(cell) => (
                              <span
                                class={`pm-byte${cell.kind ? ` pm-k-${cell.kind}` : ''}${hot(cell.offset) ? ' pm-hot' : ''}`}
                                data-offset={cell.offset}
                                onMouseEnter={() => hoverByte(cell.offset)}
                              >
                                {cell.hex}
                              </span>
                            )}
                          </For>
                        </span>
                        <span class="pm-ascii" aria-hidden="true">
                          <For each={line.cells}>
                            {(cell) => (
                              <span
                                class={`pm-ascii-cell${hot(cell.offset) ? ' pm-hot' : ''}`}
                                data-offset={cell.offset}
                                onMouseEnter={() => hoverByte(cell.offset)}
                              >
                                {cell.ascii}
                              </span>
                            )}
                          </For>
                        </span>
                      </div>
                    )}
                  </For>
                </pre>
                {decoded && (
                  <section class="pm-decoded" aria-label="Decoded" onMouseLeave={() => setHovered(undefined)}>
                    <h3>
                      Decoded
                      <span class={`pm-checksum ${decoded.checksum}`}>
                        {decoded.checksum === 'ok' ? 'checksum ok' : 'checksum mismatch'}
                      </span>
                    </h3>
                    <For each={groups(decoded.fields)}>
                      {(group) => (
                        <>
                          <h4>{group.section}</h4>
                          <For each={group.fields}>
                            {(f) => (
                              <div
                                class={`pm-field${hot(f.offset) ? ' pm-hot' : ''}`}
                                data-offset={f.offset}
                                onMouseEnter={() => setHovered({ start: f.offset, end: f.offset + f.length })}
                              >
                                <span class="pm-field-label">{f.label}</span>
                                <span class="pm-field-value">{f.value}</span>
                                <span class="pm-field-size">{f.size === undefined ? '' : `${f.size} B`}</span>
                              </div>
                            )}
                          </For>
                        </>
                      )}
                    </For>
                  </section>
                )}
              </>
            );
          }}
        </Match>
      </Switch>
    </div>
  );
}
