import { untrack, type JSX } from 'solid-js';
import { foldersFromOutside } from '@digi/workbench-core/transfer/samples-core.ts';
import { ROWS_TYPE, dropFolder } from '@digi/workbench-core/transfer/select-core.ts';
import SplitPane from '@digi/workbench-core/ui/transfer/SplitPane.tsx';
import { droppedFiles, type DeviceDrop, type PageCtx } from '@digi/workbench-core/ui/transfer/TransferParts.tsx';
import type { SamplesApp } from './app';
import LocalSamplesPane from './LocalSamplesPane';
import TargetSamplesPane from './TargetSamplesPane';

export default function SamplesMode(props: { ctx: PageCtx; samples: SamplesApp }): JSX.Element {
  const samples = untrack(() => props.samples);
  const { source, destination } = samples;

  const onDeviceDrop: DeviceDrop['onDrop'] = (kind, folder, e) => {
    const target = dropFolder(folder, destination.folder());
    if (kind === 'files') {
      const dropped = droppedFiles(e);
      if (dropped.folders) props.ctx.setError(foldersFromOutside(dropped.folders));
      void samples.uploadFiles(dropped.files, target);
      return;
    }
    let keys: unknown;
    try {
      keys = JSON.parse(e.dataTransfer?.getData(ROWS_TYPE) ?? '[]');
    } catch {
      return;
    }
    if (!Array.isArray(keys) || !keys.every((key) => typeof key === 'string')) return;
    const wanted = new Set<string>(keys);
    void samples.uploadRows(
      source.rows().filter((row) => wanted.has(row.key)),
      target,
    );
  };

  return (
    <SplitPane id="transfer">
      <LocalSamplesPane ctx={props.ctx} source={source} canUpload={destination.writable} upload={samples.uploadRows} />
      <TargetSamplesPane
        ctx={props.ctx}
        destination={destination}
        onDrop={onDeviceDrop}
        open={samples.openDestination}
        openLocation={samples.openLocation}
        createFolder={samples.createFolder}
        removeSelected={samples.removeSelected}
        restart={samples.restart}
      />
    </SplitPane>
  );
}
