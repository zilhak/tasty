// Claude Design 런너 (M3) — off-screen 헤드풀 Playwright 상주 스크립트.
//
// 이 파일은 Rust 플러그인 바이너리에 include_str! 로 임베드되어 on_start 시
// TASTY_PLUGIN_DATA_DIR/runner/design-runner.js 로 기록된 뒤, 자식 node 프로세스로
// 실행된다. Playwright 모듈은 시스템 설치본을 NODE_PATH 로 참조한다(번들·설치 안 함).
//
// 프로토콜 (설계 §5, NDJSON):
//   stdin  (plugin → runner): {"id":N,"op":"ping|status|probe|shutdown", ...}
//   stdout (runner → plugin): {"id":N,"kind":"pong|status|error|...", ...}
//   진단 로그는 stderr 로만 — stdout 은 순수 NDJSON 으로 유지.
//
// M3 범위: 자식 감독 + NDJSON 왕복 + off-screen 헤드풀 launch 검증.
//   - ping   : 생존 확인
//   - status : 부작용 없이 현재 상태 보고(브라우저 강제 기동 안 함)
//   - probe  : off-screen 헤드풀 기동 → claude.ai/design 이동 → CF/로그인 휴리스틱 보고
//   - shutdown: 브라우저 종료 후 종료
// 로그인(auth 주입)·Chat 은 후속 마일스톤(M4/M5).

'use strict';

const readline = require('readline');

const DESIGN_URL = 'https://claude.ai/design';
// off-screen: 화면 밖으로 던져 사용자 포커스/시야를 방해하지 않는다(설계 §1·§8).
// bringToFront() 는 절대 호출하지 않는다.
const OFFSCREEN_ARGS = ['--window-position=-32000,-32000'];

let playwright = null;
let browser = null;
let context = null;
let page = null;

function log(...args) {
  process.stderr.write(`[design-runner] ${args.join(' ')}\n`);
}

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function loadPlaywright() {
  if (playwright) return playwright;
  // NODE_PATH(시스템 설치 playwright 의 부모 디렉토리)로 해석된다.
  playwright = require('playwright');
  return playwright;
}

async function ensureBrowser() {
  if (browser && browser.isConnected()) return;
  const pw = loadPlaywright();
  log('launching off-screen headful chromium...');
  // headless:false 가 핵심 — Cloudflare Managed Challenge 는 헤드리스 지문을 차단한다
  // (조사 §5/§6). 진짜 헤드풀만 통과.
  browser = await pw.chromium.launch({
    headless: false,
    args: OFFSCREEN_ARGS,
  });
  context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  page = await context.newPage();
  log('browser ready');
}

// 현재 페이지 상태에서 CF 통과/로그인 여부를 휴리스틱으로 판정.
async function inspectPage() {
  if (!page) return { browser: 'closed', url: null, cf_ok: null, logged_in: null };
  let url = null;
  let title = '';
  try {
    url = page.url();
    title = (await page.title()) || '';
  } catch (e) {
    log('inspect failed:', e.message);
  }
  // Cloudflare 챌린지 페이지는 title 이 "Just a moment..." (조사 §5).
  const cfChallenge = /just a moment/i.test(title);
  // auth 없이 design 에 가면 로그인 페이지로 유도된다.
  const looksLogin = /\/login|\/sign-in|\/auth/i.test(url || '');
  const onDesign = /\/design/i.test(url || '') && !looksLogin;
  return {
    browser: 'open',
    url,
    title,
    cf_ok: !cfChallenge,
    // M3 은 auth 주입 전이므로 logged_in 은 대개 false. design 화면에 도달하면 true.
    logged_in: onDesign,
  };
}

async function handle(req) {
  const { id, op } = req;
  switch (op) {
    case 'ping':
      send({ id, kind: 'pong' });
      break;

    case 'status': {
      // 부작용 없음: 브라우저가 이미 떠 있으면 그 상태를, 아니면 closed 를 보고.
      if (browser && browser.isConnected()) {
        const info = await inspectPage();
        send({ id, kind: 'status', ...info });
      } else {
        send({ id, kind: 'status', browser: 'closed', url: null, cf_ok: null, logged_in: null });
      }
      break;
    }

    case 'probe': {
      // M3 검증용: off-screen 헤드풀을 실제로 띄워 claude.ai/design 까지 도달하는지 확인.
      try {
        await ensureBrowser();
        log('navigating to', DESIGN_URL);
        await page.goto(DESIGN_URL, { waitUntil: 'domcontentloaded', timeout: 30000 });
        // CF 자동 재통과 / 리다이렉트가 정착할 시간을 잠깐 준다.
        await page.waitForTimeout(2500);
        const info = await inspectPage();
        send({ id, kind: 'status', ...info });
      } catch (e) {
        send({ id, kind: 'error', code: 'probe_failed', message: e.message });
      }
      break;
    }

    case 'shutdown':
      send({ id, kind: 'bye' });
      await cleanup();
      process.exit(0);
      break;

    default:
      send({ id, kind: 'error', code: 'unknown_op', message: `unknown op: ${op}` });
  }
}

async function cleanup() {
  try {
    if (browser) await browser.close();
  } catch (e) {
    log('cleanup error:', e.message);
  }
}

function main() {
  const rl = readline.createInterface({ input: process.stdin });
  rl.on('line', (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let req;
    try {
      req = JSON.parse(trimmed);
    } catch (e) {
      send({ id: null, kind: 'error', code: 'bad_json', message: e.message });
      return;
    }
    // 직렬 처리: 캔버스는 턴 기반이라 요청을 겹치지 않게 한다(설계 §10-3).
    queue = queue.then(() => handle(req)).catch((e) => {
      send({ id: req && req.id, kind: 'error', code: 'handler_crash', message: e.message });
    });
  });
  rl.on('close', async () => {
    await cleanup();
    process.exit(0);
  });
  // stdin 종료/부모 사망 시 안전 종료.
  process.on('SIGTERM', async () => { await cleanup(); process.exit(0); });
  log('runner started, awaiting NDJSON on stdin');
}

let queue = Promise.resolve();
main();
