import type { Register, Timer } from 'claude-code';
import type { NotifyCursor } from '../types/index';
import { batch, MAX_READ, position, retention } from './reader';
import { heading } from './messages';

const cursorRef = { plugin: 'tasty-notify', key: 'cursor' } as const;

export const register: Register = (on) => {
  let timer: Timer | undefined;
  let stopped = false;
  let generation = 0;
  on('session.start', async ($, e, next) => {
    timer?.cancel();
    stopped = false;
    const run = ++generation;
    const currentRun = () => !stopped && generation === run;
    await $.env.set('TASTY_CLAUDE_NOTIFY_MOD', undefined);
    const surface = await $.env.get('TASTY_SURFACE_ID');
    const home = await $.env.get('TASTY_PARENT_HOME');
    const locale = await $.env.get('TASTY_LOCALE');
    if (!surface || !/^\d+$/.test(surface) || !home || !/^(\/|[A-Za-z]:[\\/])/.test(home)) {
      return next(e);
    }
    const path = `${home.replace(/[\\/]$/, '')}/notify/${surface}.log`;
    const namespace = path;
    let cursor = (await $.state.get(cursorRef)).value;
    const save = async (value: NotifyCursor) => {
      if (!currentRun()) throw new Error('notify watcher retired');
      const result = await $.state.set(cursorRef, value);
      if (!result.isSet) throw new Error('notify cursor refused');
      cursor = value;
    };
    const snapshot = async (): Promise<{ bytes: Uint8Array; base: number } | undefined> => {
      // A missing meta file is the old writer's zero baseline. Other I/O errors retry.
      const meta = async () => await $.fs.exists(`${path}.meta`)
        ? retention(await $.fs.read(`${path}.meta`)) : 0;
      const base = await meta();
      if (!await $.fs.exists(path)) return undefined;
      const stat = await $.fs.stat(path);
      if (stat.kind !== 'file' || stat.isLink || stat.size > MAX_READ) throw new Error('notify log bounds');
      const raw = await $.fs.read(path, { as: 'bytes' });
      const bytes = Uint8Array.from(atob(raw.base64), c => c.charCodeAt(0));
      if (bytes.length > MAX_READ || base !== await meta()) throw new Error('notify log changed');
      return { bytes, base };
    };
    try {
      if (!cursor || cursor.namespace !== namespace) {
        const initial = await snapshot();
        await save({ namespace, next: initial ? initial.base + initial.bytes.length : 0,
          retention: initial?.base ?? 0, missing: !initial });
      }
      const active = await $.env.set('TASTY_CLAUDE_NOTIFY_MOD', surface);
      if ('deny' in active) return next(e);
    } catch {
      // Do not arm at a later EOF and silently skip children completed during startup.
      return next(e);
    }
    const poll = async () => {
      try {
        const current = await snapshot();
        if (!current) {
          await save({ ...cursor!, next: 0, retention: 0, missing: true });
        } else {
          const start = position(cursor!, current.base, current.bytes.length);
          const part = batch(current.bytes, start);
          if (!currentRun()) return;
          if (part.end > start) {
            const submitted = await $.prompt.submit({ text: `${heading(locale)}\n${part.text}` });
            if (submitted.drop !== undefined) throw new Error('notify prompt refused');
          }
          await save({ namespace, next: current.base + part.end, retention: current.base, missing: false });
        }
      } catch {
        // Keep the last accepted cursor on transient I/O or submit refusal; never PTY input.
      } finally {
        if (currentRun()) timer = $.clock.after(500, () => { void poll(); });
      }
    };
    timer = $.clock.after(500, () => { void poll(); });
    return next(e);
  });
  on('session.end', async ($, e, next) => {
    // /clear keeps the same process and watcher; it does not emit session.start.
    if (e.reason !== 'clear') {
      stopped = true;
      generation++;
      timer?.cancel();
      await $.env.set('TASTY_CLAUDE_NOTIFY_MOD', undefined);
    }
    return next(e);
  });
};
