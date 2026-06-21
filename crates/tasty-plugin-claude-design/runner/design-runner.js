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

const DESIGN_ORIGIN = 'https://claude.ai';
const DESIGN_URL = `${DESIGN_ORIGIN}/design`;
const projectUrl = (uuid) => `${DESIGN_ORIGIN}/design/p/${uuid}`;

// 채팅 UI 셀렉터 (실측 확정, 설계 관찰 기록 참조).
const SEL_COMPOSER = '[data-testid="chat-composer-input"], div[role="textbox"].ProseMirror';
const SEL_SEND = '[data-testid="chat-send-button"]';
const SEL_MESSAGES = '[data-testid="chat-messages"]';
// 턴 종료 신호: 모델 턴 스트리밍 RPC 가 닫히는 시점.
const CHAT_RPC_RE = /\/OmeletteService\/Chat$/;

// off-screen: 화면 밖으로 던져 사용자 포커스/시야를 방해하지 않는다(설계 §1·§8).
// bringToFront() 는 절대 호출하지 않는다.
const OFFSCREEN_ARGS = ['--window-position=-32000,-32000'];

let playwright = null;
let browser = null;
let context = null;
let page = null;
// 파싱된 storageState 객체(또는 null). set_auth 로 주입되며 ensureBrowser 가
// newContext 에 사용한다. 디스크엔 plugin(Rust auth.rs)이 평문 저장(ADR-0018).
let authState = null;

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
  const ctxOpts = { viewport: { width: 1280, height: 800 } };
  // 저장된 로그인 세션이 있으면 주입 — CF 통과 + 로그인 유지(조사 §6 Test 3).
  if (authState) ctxOpts.storageState = authState;
  context = await browser.newContext(ctxOpts);
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

    case 'set_auth': {
      // 저장된(또는 비우는) 세션을 주입. 떠 있는 컨텍스트는 닫아 다음 기동 때
      // 새 auth 로 재생성한다.
      try {
        authState = req.storage_state ? JSON.parse(req.storage_state) : null;
        if (context) {
          await context.close();
          context = null;
          page = null;
        }
        send({ id, kind: 'ok' });
      } catch (e) {
        send({ id, kind: 'error', code: 'bad_auth', message: e.message });
      }
      break;
    }

    case 'login': {
      // 사용자가 직접 로그인해야 하므로 화면 안(visible)에 별도 브라우저를 띄운다.
      // 상주 off-screen 브라우저와 독립. 로그인 완료를 폴링해 storageState 를 추출.
      let loginBrowser = null;
      try {
        const pw = loadPlaywright();
        loginBrowser = await pw.chromium.launch({ headless: false });
        const lctx = await loginBrowser.newContext({ viewport: { width: 1280, height: 900 } });
        const lpage = await lctx.newPage();
        await lpage.goto(DESIGN_URL, { waitUntil: 'domcontentloaded', timeout: 30000 });
        log('login: waiting for user to authenticate (up to 5 min)...');
        const deadline = Date.now() + 5 * 60 * 1000;
        let ok = false;
        while (Date.now() < deadline) {
          await lpage.waitForTimeout(1000);
          let url = '';
          try { url = lpage.url(); } catch (_) { /* navigating */ }
          // design 앱 도달(로그인 페이지 아님) = 로그인 성공.
          if (/\/design/i.test(url) && !/\/login|\/sign-in|\/auth/i.test(url)) { ok = true; break; }
        }
        if (!ok) {
          await loginBrowser.close();
          send({ id, kind: 'login_needed', message: 'login not completed within timeout' });
          break;
        }
        await lpage.waitForTimeout(1500); // 세션 쿠키 정착 대기.
        const state = await lctx.storageState();
        await loginBrowser.close();
        authState = state; // 즉시 사용 가능하게.
        send({ id, kind: 'login_ok', storage_state: JSON.stringify(state) });
      } catch (e) {
        try { if (loginBrowser) await loginBrowser.close(); } catch (_) { /* ignore */ }
        send({ id, kind: 'error', code: 'login_failed', message: e.message });
      }
      break;
    }

    case 'list_projects': {
      // /design 홈의 프로젝트 행에서 UUID/이름 추출 (a[href^="/design/p/"]).
      try {
        await ensureBrowser();
        if (!/\/design\/?$/.test(page.url())) {
          await page.goto(DESIGN_URL, { waitUntil: 'domcontentloaded', timeout: 30000 });
        }
        await page.waitForTimeout(1500);
        const raw = await page.evaluate(() =>
          [...document.querySelectorAll('a[href^="/design/p/"]')].map((a) => ({
            uuid: (a.getAttribute('href') || '').replace('/design/p/', '').split(/[/?#]/)[0],
            name: (a.textContent || '').trim(),
          })),
        );
        const seen = new Set();
        const projects = [];
        for (const p of raw) {
          if (p.uuid && !seen.has(p.uuid)) { seen.add(p.uuid); projects.push(p); }
        }
        send({ id, kind: 'projects', projects });
      } catch (e) {
        send({ id, kind: 'error', code: 'list_failed', message: e.message });
      }
      break;
    }

    case 'chat': {
      // 기계적 chat: 프로젝트 진입 → composer 입력 → send 클릭 → Chat 스트림 종료 대기
      // → 응답 델타 추출. (관찰로 확정한 결정론적 레시피.)
      try {
        await ensureBrowser();
        if (req.project) {
          const want = `/design/p/${req.project}`;
          if (!page.url().includes(want)) {
            await page.goto(projectUrl(req.project), { waitUntil: 'domcontentloaded', timeout: 30000 });
          }
        }
        const composer = page.locator(SEL_COMPOSER).first();
        await composer.waitFor({ state: 'visible', timeout: 20000 });
        await composer.click();
        await composer.fill(req.message);

        // 전송 전 스레드 텍스트 baseline.
        const before = await page.evaluate((sel) => {
          const c = document.querySelector(sel);
          return c ? (c.innerText || '') : '';
        }, SEL_MESSAGES);

        // 턴 종료 신호를 클릭 전에 건다. timeout 은 디자인 턴이 길 수 있어 넉넉히.
        const timeoutMs = req.timeout_ms || 180000;
        const chatDone = page.waitForResponse((r) => CHAT_RPC_RE.test(r.url()), { timeout: timeoutMs });
        await page.locator(SEL_SEND).click();
        const resp = await chatDone;
        await resp.finished(); // 스트림 닫힘 = 모델 턴 종료.
        await page.waitForTimeout(800); // DOM 반영 여유.

        const reply = await page.evaluate(({ sel, before }) => {
          const c = document.querySelector(sel);
          if (!c) return null;
          const after = (c.innerText || '');
          // 새로 붙은 텍스트(= 내 메시지 echo + assistant 응답 + 도구 흔적). best-effort.
          const delta = after.startsWith(before) ? after.slice(before.length) : after;
          return delta.trim();
        }, { sel: SEL_MESSAGES, before });

        send({ id, kind: 'chat_done', reply, url: page.url() });
      } catch (e) {
        send({ id, kind: 'error', code: 'chat_failed', message: e.message });
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
