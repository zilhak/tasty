import { test, expect, mock } from 'claude-code/testing';
import type { Engine } from 'claude-code/testing';
import type { On } from 'claude-code';
import { batch, position, retention } from '../hooks/reader';
import { heading } from '../hooks/messages';

const root = '/owned/tasty/notify/7.log';
function fixture(on: On, variables: Record<string, string> = { TASTY_SURFACE_ID: '7', TASTY_PARENT_HOME: '/owned/tasty' }) {
  const clock = mock.clock(on);
  on('env.get', ($, e) => ({ value: variables[e.name] }));
  const files: Record<string, string> = { [root]: 'old\n', [`${root}.meta`]: 'retention_start=0                   \n' };
  const prompts: string[] = [];
  const origins: unknown[] = [];
  const env: Record<string, string | undefined> = {};
  let refuse = false;
  let readDelay = 0;
  on('env.set', ($, e) => { env[e.name] = e.value; return { value: {} }; });
  on('fs.exists', ($, e) => ({ value: e.path in files }));
  on('fs.stat', ($, e) => ({ value: { kind: 'file', isLink: false, size: new TextEncoder().encode(files[e.path]).length, mtimeMs: 1 } }));
  on('fs.read', async ($, e) => {
    if (e.path === root && readDelay) await clock.sleep(readDelay);
    return { value: e.as === 'bytes' ? { base64: btoa(Array.from(new TextEncoder().encode(files[e.path]), n => String.fromCharCode(n)).join('')) } : files[e.path]! };
  });
  on('prompt.submit', ($, e) => { if (refuse) return { drop: 'test refusal' }; prompts.push(e.text); origins.push(e.origin); return { text: e.text, origin: e.origin }; });
  on('session.start', ($, e) => ({ cwd: e.cwd }));
  on('session.end', ($, e) => ({ sessionId: e.sessionId }));
  return { clock, files, prompts, origins, env, variables, delay: (value: number) => { readDelay = value; }, refuse: (value: boolean) => { refuse = value; } };
}
const start = ($: Engine) => $.session.start({ cwd: '/owned', surface: null, isInteractive: false });

test('startup ignores old log; fast completion is coalesced once; partial UTF8 line waits', async ($, on) => {
  const f = fixture(on);
  await start($);
  expect(f.env.TASTY_CLAUDE_NOTIFY_MOD).toBe('7');
  f.files[root] += 'child one\n두 번째\npartial';
  await f.clock.advance(500);
  expect(f.prompts.length).toBe(1);
  expect(f.prompts[0]).toContain('child one\n두 번째\n');
  expect(f.prompts[0]).toContain('not proof');
  expect(f.origins[0]).toEqual({ kind: 'plugin', name: 'tasty-notify' });
  await f.clock.advance(1000);
  expect(f.prompts.length).toBe(1);
  f.files[root] += '\n';
  await f.clock.advance(500);
  expect(f.prompts.length).toBe(2);
  expect(f.prompts[1]).toContain('partial\n');
});

test('reload resumes session cursor and has one timer; clear continues; end cancels', async ($, on) => {
  const f = fixture(on);
  await start($);
  f.files[root] += 'between loads\n';
  await start($);
  await f.clock.advance(500);
  expect(f.prompts.length).toBe(1);
  await $.session.end({ reason: 'clear', sessionId: 'test', resume: { id: 'test' } });
  f.files[root] += 'after clear\n';
  await f.clock.advance(500);
  expect(f.prompts.length).toBe(2);
  await $.session.end({ reason: 'prompt_input_exit', sessionId: 'test', resume: { id: 'test' } });
  f.files[root] += 'after end\n';
  await f.clock.advance(1000);
  expect(f.prompts.length).toBe(2);
});

test('truncation meta, uncounted shrink, delete and recreate resume at beginning', async ($, on) => {
  const f = fixture(on); await start($);
  f.files[`${root}.meta`] = 'retention_start=100                 \n';
  f.files[root] = 'new\n'; await f.clock.advance(500);
  expect(f.prompts[0]).toContain('new\n');
  f.files[root] = 'x\n'; await f.clock.advance(500);
  expect(f.prompts[1]).toContain('x\n');
  delete f.files[root]; await f.clock.advance(500);
  f.files[root] = 'recreated\n'; await f.clock.advance(500);
  expect(f.prompts[2]).toContain('recreated\n');
  f.files[`${root}.meta`] = 'retention_start=0                   \n';
  f.files[root] = 'host reboot\n'; await f.clock.advance(500);
  expect(f.prompts[3]).toContain('host reboot\n');
});

test('submit refusal retains cursor for retry; other surface logs are not read', async ($, on) => {
  const f = fixture(on); await start($);
  f.files['/owned/tasty/notify/8.log'] = 'foreign\n';
  f.files[root] += 'own\n'; f.refuse(true);
  await f.clock.advance(500); expect(f.prompts.length).toBe(0);
  f.refuse(false); await f.clock.advance(500);
  expect(f.prompts.length).toBe(1); expect(f.prompts[0]).toContain('own\n');
  await f.clock.advance(500); expect(f.prompts.length).toBe(1);
});

test('outside Tasty clears inherited marker and starts no watcher', async ($, on) => {
  const f = fixture(on, { TASTY_SURFACE_ID: '', TASTY_PARENT_HOME: '' }); await start($);
  await f.clock.advance(1000); expect(f.prompts.length).toBe(0);
  expect(f.env.TASTY_CLAUDE_NOTIFY_MOD).toBeUndefined();
});

test('reader byte offsets, bounded batches and invalid retention', () => {
  expect(retention('unknown=9\nretention_start=123   \n')).toBe(123);
  expect(retention('')).toBe(0);
  expect(() => retention('retention_start=999999999999999999999')).toThrow();
  expect(batch(new TextEncoder().encode('한\nrest'), 0).end).toBe(4);
  expect(batch(new TextEncoder().encode('partial'), 0).end).toBe(0);
  expect(position({ namespace: root, next: 40, retention: 20, missing: false }, 0, 100)).toBe(0);
});


test('initial missing log catches its first fast completion; large snapshots refuse without consuming', async ($, on) => {
  const f = fixture(on); delete f.files[root]; await start($);
  f.files[root] = 'first completion\n'; await f.clock.advance(500);
  expect(f.prompts.length).toBe(1);
  const saved = f.files[root];
  f.files[root] += 'x'.repeat(1024 * 1024); await f.clock.advance(500);
  expect(f.prompts.length).toBe(1);
  f.files[root] = saved + 'after refusal\n'; await f.clock.advance(500);
  expect(f.prompts.length).toBe(2);
});

test('new namespace takes a fresh EOF rather than replaying another surface', async ($, on) => {
  const f = fixture(on); await start($);
  f.variables.TASTY_PARENT_HOME = '/other';
  f.files['/other/notify/7.log'] = 'other old line\n';
  await start($); await f.clock.advance(500);
  expect(f.prompts.length).toBe(0);
  f.files['/other/notify/7.log'] += 'current only\n'; await f.clock.advance(500);
  expect(f.prompts.length).toBe(1);
});


test('retirement during an asynchronous read cannot submit or rearm the watcher', async ($, on) => {
  const f = fixture(on); await start($);
  f.files[root] += 'retired completion\n'; f.delay(1000);
  await f.clock.advance(500);
  await $.session.end({ reason: 'prompt_input_exit', sessionId: 'test', resume: { id: 'test' } });
  await f.clock.advance(2000);
  expect(f.prompts.length).toBe(0);
});

test('visible heading has three translations and an English fallback', () => {
  expect(heading('en')).toContain('not proof');
  expect(heading('ko')).toContain('성공의 증거가 아닙니다');
  expect(heading('ja')).toContain('成功の証拠ではありません');
  expect(heading('unknown')).toBe(heading('en'));
});
