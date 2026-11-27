// The subset of workbench.toml exercised by host companions in the preview.
import { parse } from 'smol-toml';

export interface TextSetting {
  id: string;
  label: string;
  maxBytes: number;
}

export interface ModuleDescriptor {
  id: string;
  name: string;
  component?: string;
  settings: TextSetting[];
  operations: string[];
}

export function readModuleDescriptor(source: string, manifest: string): ModuleDescriptor {
  const module = parse(manifest).module;
  if (
    !module ||
    typeof module !== 'object' ||
    !('id' in module) ||
    typeof module.id !== 'string' ||
    typeof module.name !== 'string' ||
    !module.name
  )
    throw new Error('Expected a module ID and name in module.toml.');
  const doc = parse(source);
  const header = doc.workbench;
  if (!header || typeof header !== 'object' || !('version' in header) || header.version !== 1)
    throw new Error('Expected a version 1 Workbench descriptor.');
  if (header.component !== undefined && typeof header.component !== 'string')
    throw new Error('Invalid component path.');
  const fields = doc.setting ?? [];
  const ops = doc.operation ?? [];
  if (!Array.isArray(fields) || !Array.isArray(ops)) throw new Error('Expected setting and operation tables.');
  const settings = fields.map((field): TextSetting => {
    if (
      !field ||
      typeof field !== 'object' ||
      !('type' in field) ||
      field.type !== 'text' ||
      typeof field.id !== 'string' ||
      !field.id ||
      typeof field.label !== 'string' ||
      typeof field.max_bytes !== 'number' ||
      !Number.isSafeInteger(field.max_bytes) ||
      field.max_bytes < 1
    )
      throw new Error('Invalid public text setting.');
    return { id: field.id, label: field.label, maxBytes: field.max_bytes };
  });
  const operations = ops.map((op) => {
    if (
      !op ||
      typeof op !== 'object' ||
      !('output' in op) ||
      op.output !== 'optional-text' ||
      typeof op.id !== 'string' ||
      !op.id
    )
      throw new Error('Invalid module operation.');
    return op.id;
  });
  const names = settings.map((field) => field.id);
  if (new Set(names).size !== names.length || new Set(operations).size !== operations.length)
    throw new Error('Duplicate field or operation ID.');
  return { id: module.id, name: module.name, component: header.component, settings, operations };
}

export function checkText(field: TextSetting, value: string) {
  if (new TextEncoder().encode(value).length > field.maxBytes)
    throw new Error(`${field.label} allows up to ${field.maxBytes} bytes.`);
}
