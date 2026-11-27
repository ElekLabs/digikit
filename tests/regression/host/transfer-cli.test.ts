import { beforeEach, expect, test, vi } from 'vitest';
import type { Session } from '@digi/test-support';

const fake = vi.hoisted(() => ({ exec: vi.fn(), mkdir: vi.fn(), write: vi.fn() }));
vi.mock('node:child_process', () => ({
  execFile: Object.assign(vi.fn(), { [Symbol.for('nodejs.util.promisify.custom')]: fake.exec }),
}));
vi.mock('node:fs', async (original) => ({
  default: {
    ...(await original<{ default: typeof import('node:fs') }>()).default,
    mkdirSync: fake.mkdir,
    writeFileSync: fake.write,
  },
}));
import { cliInstall, transferCommand } from '../runtime/transfer-cli.ts';

const session = { runDir: '/run/own-daemon' } as Session;

beforeEach(() => {
  vi.resetAllMocks();
});

test('service install uses its own module, artifact directory and 300-second deadline', async () => {
  fake.exec.mockResolvedValueOnce({
    stdout: 'install progress\n{"event":"loaded","loaded":["test/svc-echo"]}\n',
    stderr: '',
  });
  await cliInstall(session, 'test/svc-echo', '/artifacts/service-command/dt2', 300_000);
  expect(fake.exec.mock.calls[0]?.[1].slice(-3)).toEqual(['install', 'test/svc-echo', '--restart']);
  expect(fake.exec.mock.calls[0]?.[2].timeout).toBe(300_000);
  expect(fake.write.mock.calls[0]?.[0]).toBe('/artifacts/service-command/dt2/install.log');
});

test.each(['', '{"event":"progress"}\n', '{"event":"loaded","loaded":["digikit/remote"]}\n'])(
  'service install rejects missing or unrelated loaded results: %j',
  async (stdout) => {
    fake.exec.mockResolvedValueOnce({ stdout, stderr: '' });
    await expect(cliInstall(session, 'test/svc-echo', '/artifacts/service-command/dt2')).rejects.toThrow(
      'install --restart did not load test/svc-echo',
    );
    expect(fake.write).toHaveBeenCalledWith('/artifacts/service-command/dt2/install.log', `${stdout}\n`);
  },
);

test('a failed CLI command retains stdout and stderr in its artifact', async () => {
  const error = Object.assign(new Error('exit 1'), { stdout: 'partial progress', stderr: 'device refused' });
  fake.exec.mockRejectedValueOnce(error);
  await expect(transferCommand(session, ['delete', 'modules/pluck.dkm'], '/artifacts/delete.log')).rejects.toThrow(
    'wb transfer delete modules/pluck.dkm failed: exit 1; see /artifacts/delete.log',
  );
  expect(fake.write).toHaveBeenCalledWith('/artifacts/delete.log', 'partial progress\ndevice refused\nexit 1');
});

test('malformed CLI JSON leaves the raw command log intact', async () => {
  fake.exec.mockResolvedValueOnce({ stdout: '{invalid JSON}\n', stderr: 'diagnostic' });
  await expect(transferCommand(session, ['restart'], '/artifacts/restart.log')).rejects.toThrow(SyntaxError);
  expect(fake.write).toHaveBeenCalledExactlyOnceWith('/artifacts/restart.log', '{invalid JSON}\n\ndiagnostic');
});
