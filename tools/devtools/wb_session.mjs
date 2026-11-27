import { readFile, realpath, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';

export async function inspectSession(source) {
  const picked = path.resolve(source);
  const isDirectory = (await stat(picked)).isDirectory();
  const root = await realpath(isDirectory ? picked : path.dirname(picked));
  if (!isDirectory && path.basename(picked) !== 'session.json')
    throw new Error('Expected a session folder or its session.json.');

  async function asset(relative) {
    if (typeof relative !== 'string' || relative.split(/[\\/]/u).some((part) => !part || part === '.' || part === '..'))
      throw new Error('Invalid relative session asset.');
    const file = await realpath(path.join(root, relative));
    const resolved = path.relative(root, file);
    if (resolved === '..' || resolved.startsWith('..' + path.sep) || path.isAbsolute(resolved))
      throw new Error('Session asset escapes its folder.');
    if (!(await stat(file)).isFile()) throw new Error('Session asset is not a file.');
    return file;
  }

  const document = JSON.parse(await readFile(await asset('session.json'), 'utf8'));
  const sha = (value) => typeof value === 'string' && /^[\da-f]{64}$/iu.test(value);
  if (
    document?.schema !== 'dk.session/1' ||
    typeof document.id !== 'string' ||
    typeof document.name !== 'string' ||
    typeof document.firmware?.device !== 'string' ||
    !sha(document.firmware?.sha256) ||
    (document.firmware.imageSha256 !== null && !sha(document.firmware.imageSha256)) ||
    (document.checkpoint !== null && !sha(document.checkpoint)) ||
    !Array.isArray(document.attachments)
  )
    throw new Error('Not a supported Workbench session.');
  const checkpoint = document.checkpoint && (await asset(`card/${document.checkpoint}.overlay`));
  if (
    checkpoint &&
    createHash('sha256')
      .update(await readFile(checkpoint))
      .digest('hex') !== document.checkpoint
  )
    throw new Error('Saved card checkpoint failed its checksum.');
  return {
    root,
    document,
    files: {
      base: await asset('card/base.img'),
      checkpoint,
      attachments: await Promise.all(
        document.attachments.map(async (entry) => ({
          ...entry,
          file: await asset(entry.path),
        })),
      ),
    },
  };
}

export async function sessionCommand({ sub, rest, opts, fail, emit = console.log }) {
  if (sub !== 'inspect' || rest.length !== 1)
    return fail('usage: mise run wb:cli -- [--format json|text] session inspect FOLDER_OR_SESSION_JSON');
  try {
    const session = await inspectSession(rest[0]);
    const { document, files } = session;
    emit(
      opts.format === 'json'
        ? JSON.stringify(session)
        : [
            document.name + ' · ' + document.firmware.device.toUpperCase(),
            `Card: ${files.base}`,
            `Checkpoint: ${files.checkpoint ?? 'imported base'}`,
            ...files.attachments.map((file) => `${file.kind}: ${file.file}`),
          ].join('\n'),
    );
    return 0;
  } catch (e) {
    return fail(e.message, 1);
  }
}
