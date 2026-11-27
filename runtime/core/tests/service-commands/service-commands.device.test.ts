import { hasCap } from '@digi/dk-session';
import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { installModule, moduleArtifact } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';

/** What only the USB transport of the firmware shows of runtime commands: a module's registered command answers over the real SysEx path with bodies of 0 to 250 bytes
 * and an oversized request is E_MSG. Registration refusals, the handler's own status and the unregistered-command reply are host tests (`module_cmds.rs`
 * `a_range_that_is_empty_unreserved_or_taken_is_refused_and_the_handlers_status_is_the_reply`). */
async function expectServiceCommands({ session, dk }: PatchedSession): Promise<void> {
  expect(['zero', 'none'], 'service RPC coverage runs without the DSP').toContain((await session.status()).dsp);
  await expectRuntimeReady(session);
  await installModule(dk, 'test/svc-echo', moduleArtifact(session.options.device, 'test/svc-echo'), {
    device: session.options.device,
  });
  expect(hasCap(await dk.info(), 'SERVICE_CMDS'), 'INFO advertises service commands').toBe(true);

  const echo = 0x3e;
  for (const body of [
    Uint8Array.from([...'hello '].map((char) => char.charCodeAt(0)).concat([0x80, 0xff])),
    new Uint8Array(),
    Uint8Array.from({ length: 150 }, (_, index) => index),
    Uint8Array.from({ length: 250 }, (_, index) => index),
  ]) {
    const reply = await dk.request(echo, body);
    expect(reply.status, `${body.length}-byte echo status`).toBe(0);
    expect(reply.body, `${body.length}-byte echo body`).toEqual(body);
  }

  expect((await dk.request(echo, new Uint8Array(300))).status, 'oversized request is E_MSG').toBe(-50);
  await session.expectNoFault('service commands');
}

runtimeTests('a module command echoes bodies of every size over the real USB path and an oversized request is refused', expectServiceCommands, {
  dsp: 'zero',
});
