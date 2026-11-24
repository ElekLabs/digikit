// Transfer page, Modules mode. On your computer: the built modules of the library folder (out/modules/<device>/, filled by the workbench
// app's `lib_build`) in their groups, and "Add .dkm" or a drop for a module from outside it. On device: what the card holds. The page speaks the
// upload protocol itself, in TypeScript (packages/workbench-core/src/modules), over the selected target; no process is started and nothing is built. Install
// uploads and verifies each ticked module on the card; that is all. The device's selection is the device's own: the user selects the module
// in MODULES on the device, then applies and restarts there. Restart only restarts the device and reports what loaded. Delete removes the
// ticked files from the card; a loaded module stays loaded until the restart.
import WaitNotice from './WaitNotice';
import { For, Show, createMemo, createSignal, type JSX } from 'solid-js';
import CopyErrorButton from '../components/CopyErrorButton';
import Segmented, { type SegmentedOption } from '../components/Segmented';
import { errorLine, formatBytes } from '../../src/modules/backend';
import {
  ListRow,
  LocalPane,
  DevicePane,
  SelectAll,
  createConfirm,
  DeleteButton,
  writing,
  type ChipStatus,
  type PageCtx,
} from './TransferParts';
import { activateSystemPackage } from '../../src/runtime/activate-system-package';
import { noteActivity } from '../../src/transfer/activity';
import ModuleFilesArea from './ModuleFilesArea';
import type { ModuleFileClient } from '../../src/module-ui/module-files';
import { useTransferApp } from '../../src/transfer/app-context';
import { createPaneSelect } from '../../src/transfer/pane-select';
import SplitPane from './SplitPane';
import {
  applyEvent,
  applyVerified,
  available,
  visibleRefusals,
  countLabel,
  DONE_ON_CARD,
  deleteDoneText,
  deletePaths,
  dependencyBadge,
  deviceTags,
  eventText,
  installModules,
  installServices,
  serviceRefusalText,
  installOutcome,
  localTags,
  tickedFit,
  tickedLoad,
  onDeviceWithPending,
  rowKey,
  rowStatus,
  startProgress,
  type TransferEvent,
  type TransferRow,
} from '../../src/transfer/transfer-core';
import './target-select.css';
import { deviceNameOf, deviceNamesText } from '../../src/data/device-profiles';

/** `onFlash`: starts the one-click Flash flow (the host page gives it; without it the refusal box has no Flash button). */
export default function ModulesMode(props: { ctx: PageCtx; onFlash?: () => void }): JSX.Element {
  const { busy, run, setError } = props.ctx;
  const app = useTransferApp();
  const { env } = app;
  const transferBackend = app.backend;
  const {
    browseDevice,
    browsing,
    delTicks,
    lastView,
    jobProgress,
    progress,
    readingAfterJob,
    setBrowseDevice,
    setDelTicks,
    setLog,
    setProgress,
    setShowTests,
    setSourceId,
    setTicks,
    setVerified,
    showTests,
    ticks,
    verified,
  } = app.state;
  let input: HTMLInputElement | undefined;
  const coreOnly = () => lastView()?.coreOnly === true && !browsing();
  const systemPackage = env.extras?.systemPackage;
  const emulatorBusy = () => app.target.target().kind === 'emulator' && app.jobs.busy('emulator');

  const installSystemPackage = () =>
    run('Installing system package', async () => {
      if (!systemPackage || !coreOnly()) return;
      setLog([]);
      setProgress(undefined);
      const binding = app.target.bindSession();
      const install = async () => {
        const pkg = await systemPackage();
        const controller = new AbortController();
        const unbind = binding.beforeLeave(async () => controller.abort());
        try {
          await activateSystemPackage(await binding.session(), pkg, {
            repair: true,
            check: binding.check,
            signal: controller.signal,
            progress: jobProgress,
          });
        } finally {
          unbind();
        }
      };
      if (binding.target.kind === 'emulator')
        await app.jobs.runJob('Installing system package', { group: 'emulator' }, install);
      else await install();
      setLog(['System package verified and active.']);
    });

  const onEvent = (e: TransferEvent) => {
    noteActivity();
    const p = progress();
    if (p) setProgress(applyEvent(p, e));
    setVerified(applyVerified(verified(), e));
    if (e.event !== 'result' && e.event !== 'progress') setLog((l) => [...l, eventText(e)]);
  };
  /** Run a transfer job with a progress bar: an error shows on the bar and is thrown on; `done` is the text after a success. */
  const job = async (work: () => Promise<void>, done: () => string) => {
    setLog([]);
    try {
      await work();
    } catch (e) {
      const p = progress();
      if (p) setProgress({ ...p, message: errorLine(e) });
      throw e;
    }
    const p = progress();
    if (p) setProgress({ ...p, message: done() });
  };
  const install = () =>
    run('Installing', async () => {
      const v = lastView();
      if (!v || coreOnly()) return;
      const modules = installModules(v, ticks());
      const services = installServices(v, ticks());
      setProgress(
        startProgress(
          modules,
          false,
          Object.fromEntries(v.modules.filter((r) => r.id).map((r) => [r.id as string, r.file_size])),
          services,
        ),
      );
      let done = DONE_ON_CARD;
      await job(
        async () => {
          const out = installOutcome(await transferBackend.install(modules, onEvent), DONE_ON_CARD);
          if ('error' in out) throw new Error(out.error);
          done = out.message;
        },
        () => done,
      );
      setTicks(new Set<string>());
    });
  const restart = () =>
    run('Restarting', async () => {
      setProgress(startProgress([], true));
      setLog([]);
      try {
        await transferBackend.restart(onEvent);
      } catch (e) {
        const p = progress();
        if (p) setProgress({ ...p, message: errorLine(e) });
        throw e;
      }
      setVerified({});
    });
  // the site keeps added files in the list (to install from there); the desktop puts them on the card at once
  const addToList = () => env.library.addFiles;
  const addFiles = (files: File[]) =>
    run(addToList() ? 'Adding' : 'Uploading', async () => {
      const picked = files.filter((f) => /\.dkm$/i.test(f.name));
      if (!picked.length) {
        setError('Only .dkm files can be added.');
        return;
      }
      const data = await Promise.all(
        picked.map(async (f) => ({ name: f.name, data: new Uint8Array(await f.arrayBuffer()) })),
      );
      const add = addToList();
      setLog(add ? await add(data) : await transferBackend.add(data));
    });

  const confirm = createConfirm();
  const nDelete = () => {
    const v = lastView();
    return v ? deletePaths(v, delTicks()).length : 0;
  };
  const removeSelected = () =>
    run('Deleting', async () => {
      const v = lastView();
      if (!v) return;
      const paths = deletePaths(v, delTicks());
      setProgress(startProgress(paths, false));
      await job(
        () => transferBackend.remove(paths, onEvent),
        () => deleteDoneText(v, paths),
      );
      setDelTicks(new Set<string>());
    });
  /** The Source link of a library row, where the site has the module's source. */
  const sourceAction = (r: TransferRow) => {
    const id = r.id;
    const src = env.source;
    return id && src?.files(id).length
      ? { label: 'Source', title: `Show the source of ${id}`, onClick: () => setSourceId(id) }
      : undefined;
  };
  const label = (r: TransferRow) => r.label ?? r.name;
  const ticked = () => {
    const v = lastView();
    return v ? installModules(v, ticks()).length : 0;
  };
  /** The rows that the select-all boxes tick: every visible library module, and every file of the device that can be deleted. */
  const localKeys = () => {
    const v = lastView();
    return v ? available(v, showTests()).flatMap((g) => g.rows.filter((r) => r.id).map(rowKey)) : [];
  };
  const refusals = createMemo(() => {
    const v = lastView();
    return v ? visibleRefusals(v, showTests()) : [];
  });
  const refusalMessages = createMemo(() => refusals().map(serviceRefusalText));
  const deviceRows = createMemo(() => {
    const v = lastView();
    return v ? onDeviceWithPending(v, progress(), busy() === 'Installing') : [];
  });
  const deviceKeys = () =>
    deviceRows()
      .filter((r) => r.on_card !== 'no')
      .map((r) => r.path);
  // the same selection model as the Samples lists: click, Shift, Cmd or Ctrl, a marquee, Cmd+A, Esc (no folders here)
  const lsel = createPaneSelect({
    keys: localKeys,
    selected: ticks,
    set: setTicks,
    disabled: () => writing(busy()),
  });
  const dsel = createPaneSelect({
    keys: deviceKeys,
    selected: delTicks,
    set: (next) => {
      setDelTicks(next);
      confirm.stop();
    },
    disabled: () => writing(busy()),
  });
  const status = (r: TransferRow): ChipStatus | undefined => {
    const st = rowStatus(progress(), verified(), r.path);
    return st?.kind === 'uploading' ? { kind: 'uploading', sent: st.sent, bytes: st.bytes } : st;
  };

  /** `service` or `module` while an install with services runs (the column is empty otherwise). */
  const phase = (r: TransferRow): string | undefined => {
    const steps = progress()?.modules ?? [];
    if (!steps.some((m) => m.kind === 'service')) return undefined;
    return steps.find((m) => m.path === r.path)?.kind ?? (r.path.endsWith('.dks') ? 'service' : 'module');
  };

  // The module whose row was clicked. The tick stays install/delete only: a row whose module has a [files] table is selected by a click
  // without being ticked; its files area shows under the lists. A card-only row has the module id when its header is known (the header name).
  const [selectedId, setSelectedId] = createSignal<string>();
  const filesId = (r: TransferRow) => {
    const id = r.id ?? (r.known ? r.name : undefined);
    return id && env.moduleFiles?.(id) ? id : undefined;
  };

  function moduleFiles(id: string): ModuleFileClient {
    return {
      list: () => app.backend.listModuleFiles(id),
      read: (name) => app.backend.readModuleFile(id, name),
      send: (name, bytes, progress) => app.backend.putModuleFile(id, name, bytes, progress),
      remove: (name) => app.backend.removeModuleFile(id, name),
    };
  }

  const browseOptions = (): SegmentedOption<string>[] =>
    (env.libraryDevices ?? []).map((d) => ({ id: d, label: deviceNameOf(d) }));

  const { libraryOnly } = app.target;
  const localOnly = () => libraryOnly() || browsing() || !lastView();
  const lists = (
    <SplitPane id="transfer">
      <LocalPane
        onDrop={(f) => {
          if (!coreOnly() && (addToList() || !localOnly())) void addFiles(f);
        }}
        select={lsel}
        selectAll={
          <SelectAll
            label="Select all modules to install"
            keys={localKeys()}
            ticked={ticks()}
            disabled={!!busy()}
            onChange={setTicks}
          />
        }
        actions={
          <>
            <Show when={browsing()}>
              <Segmented
                label="Show modules for"
                options={browseOptions()}
                value={browseDevice()}
                disabled={!!busy()}
                onChange={(d) => {
                  setBrowseDevice(d);
                  void run('Reading', async () => {});
                }}
              />
            </Show>
            <input
              ref={input}
              type="file"
              accept=".dkm"
              multiple
              hidden
              onChange={(e) => {
                const files = Array.from(e.currentTarget.files ?? []);
                e.currentTarget.value = '';
                void addFiles(files);
              }}
            />
            <button
              type="button"
              class="act small"
              disabled={!!busy() || coreOnly() || (!addToList() && localOnly())}
              title={
                addToList()
                  ? 'Add module files from your computer to this list, then install them'
                  : 'Put a module from outside the library on the device'
              }
              onClick={() => input?.click()}
            >
              Add .dkm…
            </button>
            <span class="drop">or drop files</span>
            <Show when={lastView()?.modules.some((r) => r.test)}>
              <label class="tests toggle">
                <input
                  type="checkbox"
                  role="switch"
                  checked={showTests()}
                  onChange={(e) => setShowTests(e.currentTarget.checked)}
                />{' '}
                show test modules
              </label>
            </Show>
            <button
              type="button"
              class="act primary"
              disabled={!!busy() || ticked() === 0 || localOnly()}
              title={
                browsing()
                  ? `Connect a ${deviceNamesText('or')} running the digikit runtime to install`
                  : ticked()
                    ? 'Build, upload and verify the ticked modules on the device; select them in MODULES on the device'
                    : 'Tick a module to install'
              }
              onClick={() => void install()}
            >
              {busy() === 'Installing' ? 'Installing…' : countLabel('Install', ticked())}
            </button>
          </>
        }
      >
        <Show when={lastView()} fallback={<p class="empty">{busy() ? `${busy()}…` : 'No module list yet.'}</p>}>
          {(v) => (
            <>
              <Show when={refusalMessages().length}>
                <div class="err refusals" role="alert">
                  <div class="msg">
                    <For each={refusalMessages()}>{(message) => <div>{message}</div>}</For>
                  </div>
                  <CopyErrorButton text={refusalMessages().join('\n')} />
                  <Show
                    when={
                      props.onFlash &&
                      refusals().some(
                        (r) =>
                          r.kind === 'fingerprint' ||
                          r.kind === 'no-port' ||
                          (r.kind === 'version' && r.service === 'runtime'),
                      )
                    }
                  >
                    <button
                      type="button"
                      class="act small"
                      disabled={!!busy()}
                      title="Flash a runtime that matches these services (Firmware page)"
                      onClick={() => props.onFlash?.()}
                    >
                      Flash
                    </button>
                  </Show>
                </div>
              </Show>
              <For each={available(v(), showTests())}>
                {(g) => (
                  <div class="group">
                    <h4>{g.group}</h4>
                    <Show
                      when={g.rows.length}
                      fallback={<p class="empty">{g.group === 'Effects' ? 'No run-time effects yet.' : 'None.'}</p>}
                    >
                      <ul class="rows">
                        <For each={g.rows}>
                          {(r) => (
                            <ListRow
                              rowKey={rowKey(r)}
                              selected={ticks().has(rowKey(r)) || (!!filesId(r) && selectedId() === filesId(r))}
                              onSelect={
                                r.id
                                  ? (e) => {
                                      const f = filesId(r);
                                      if (f) setSelectedId(f);
                                      else {
                                        setSelectedId(undefined);
                                        lsel.click(rowKey(r), e);
                                      }
                                    }
                                  : undefined
                              }
                              check={{
                                label: `Install ${label(r)}`,
                                checked: ticks().has(rowKey(r)),
                                disabled: writing(busy()),
                                onChange: () => lsel.tick(rowKey(r)),
                              }}
                              name={label(r)}
                              title={r.id ?? r.path}
                              tags={localTags(r)}
                              dependencies={dependencyBadge(r.services)}
                              version={r.version ?? ''}
                              size={formatBytes(r.file_size)}
                              action={sourceAction(r)}
                            />
                          )}
                        </For>
                      </ul>
                    </Show>
                  </div>
                )}
              </For>
              <Show when={tickedFit(v(), ticks())?.tone === 'warn' && tickedFit(v(), ticks())}>
                {(f) => (
                  <p
                    class="fitbar"
                    role="status"
                    title={tickedLoad(v(), ticks()) ? `${f().text}\n${tickedLoad(v(), ticks())}` : f().text}
                  >
                    {f().text}
                  </p>
                )}
              </Show>
            </>
          )}
        </Show>
      </LocalPane>

      <Show when={!env.targets.inHeader || !localOnly()} fallback={<section class="col" aria-label="On device" />}>
        <DevicePane
          ctx={props.ctx}
          select={dsel}
          selectAll={
            <SelectAll
              label="Select all files to delete"
              keys={deviceKeys()}
              ticked={delTicks()}
              disabled={!!busy()}
              onChange={(next) => {
                setDelTicks(next);
                confirm.stop();
              }}
            />
          }
          actions={
            <>
              <DeleteButton
                n={browsing() || coreOnly() ? 0 : nDelete()}
                busy={busy()}
                confirming={confirm.confirming()}
                title="Delete the ticked files from the device (a loaded module stays loaded until the next restart)"
                onClick={() => confirm.click(() => void removeSelected())}
              />
              <button
                type="button"
                class="act"
                disabled={!!busy() || !lastView() || browsing() || coreOnly()}
                title="RESTART the device (it loads what its MODULES menu has selected)"
                onClick={() => void restart()}
              >
                {busy() === 'Restarting' ? 'Restarting…' : 'Restart'}
              </button>
            </>
          }
        >
          <WaitNotice active={!!busy()} cancel={busy() !== 'Installing' && !readingAfterJob()} />
          <Show when={lastView()}>
            <Show
              when={deviceRows().length}
              fallback={
                <p class="empty">
                  {browsing()
                    ? `Connect a ${deviceNamesText('or')} to see what is on it.`
                    : 'The device holds no modules.'}
                </p>
              }
            >
              <ul class="rows">
                <For each={deviceRows()}>
                  {(r) => (
                    <ListRow
                      rowKey={r.on_card === 'no' ? undefined : r.path}
                      selected={delTicks().has(r.path) || (!!filesId(r) && selectedId() === filesId(r))}
                      onSelect={
                        r.on_card === 'no'
                          ? undefined
                          : (e) => {
                              const f = filesId(r);
                              if (f) setSelectedId(f);
                              else {
                                setSelectedId(undefined);
                                dsel.click(r.path, e);
                              }
                            }
                      }
                      check={
                        r.on_card === 'no'
                          ? undefined
                          : {
                              label: `Delete ${label(r)}`,
                              checked: delTicks().has(r.path),
                              disabled: writing(busy()),
                              onChange: () => dsel.tick(r.path),
                            }
                      }
                      name={label(r)}
                      title={r.path}
                      tags={deviceTags(r)}
                      dependencies={dependencyBadge(r.card_services)}
                      version={r.version ?? ''}
                      size={formatBytes(r.file_size)}
                      status={status(r)}
                      phase={phase(r)}
                    />
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </DevicePane>
      </Show>
    </SplitPane>
  );

  return (
    <>
      <Show when={coreOnly()}>
        <div class="err" role="status">
          <span class="msg">
            The target is running in core-only mode.
            <Show when={systemPackage}> Install the system package to restore module support.</Show>
          </span>
          <Show when={systemPackage}>
            <button
              type="button"
              class="act small"
              disabled={!!busy() || emulatorBusy()}
              onClick={() => void installSystemPackage()}
            >
              Install system package
            </button>
          </Show>
        </div>
      </Show>
      {lists}
      <Show when={!localOnly() && !coreOnly() && selectedId()} keyed>
        {(id) => (
          <Show when={env.moduleFiles?.(id)}>
            {(spec) => (
              <ModuleFilesArea
                ctx={props.ctx}
                id={id}
                spec={spec()}
                files={moduleFiles(id)}
                refresh={lastView}
                prepareSend={async () => {
                  if (lastView()?.modules.find((row) => row.id === id)?.on_card === 'no') {
                    await app.backend.install([id], () => {});
                  }
                }}
              />
            )}
          </Show>
        )}
      </Show>
    </>
  );
}
