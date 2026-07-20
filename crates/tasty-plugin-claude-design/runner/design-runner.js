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

// URL이 실제 로그인된 claude.ai/design 앱인지 판정. path substring match(/\/design/)는
// 로그아웃 시 claude.com/product/design(마케팅 페이지)로의 리다이렉트도 함께 매칭해
// 오탐(false "logged_in: true")을 내므로, 호스트 + 정확한 경로로 앵커링한다.
function isDesignAppUrl(url) {
  if (!url) return false;
  let u;
  try {
    u = new URL(url);
  } catch (_) {
    return false;
  }
  return u.hostname === 'claude.ai' && /^\/design(\/|$)/.test(u.pathname);
}

// 채팅 UI 셀렉터 (실측 확정, 설계 관찰 기록 참조).
const SEL_COMPOSER = '[data-testid="chat-composer-input"], div[role="textbox"].ProseMirror';
const SEL_SEND = '[data-testid="chat-send-button"]';
const SEL_MESSAGES = '[data-testid="chat-messages"]';
// 턴 종료(완료) 신호: 클라이언트가 턴 lease 를 반납하는 ReleaseTurn RPC. 실측상 디자인
// 턴은 첫 Chat 스트림을 열어둔 채(닫히지 않음) RenewTurn 으로 lease 를 연장하며 도구 호출·
// 편집을 수행하다가, 완료 시 ReleaseTurn 을 딱 한 번 호출한다. 따라서 "첫 Chat 응답 스트림
// close" 가 아니라 이 ReleaseTurn 이 진짜 턴 종료 신호다(과거엔 Chat 스트림 close 를 기다려
// 영영 안 풀리고 timeout 났다 — 응답은 화면에 다 떠도 완료를 못 읽음). 주의: 서비스명이
// `...v1alpha.OmeletteService` 라 메서드 앞은 `/`, 서비스명 안은 `.` 다. 쿼리스트링 허용.
const RELEASE_TURN_RE = /OmeletteService\/ReleaseTurn(\?|$)/;
// "Your other tab is working on a request" 배너 — claude.ai/design 은 한 프로젝트에
// 동시 한 turn 만 허용한다(동시성 lock 프로토콜의 근거). 전송 직후 이 배너가 뜨면
// 충돌이므로 timeout 까지 헛대기 말고 즉시 busy 로 보고한다.
const BUSY_TEXT_RE = /other tab is working/i;
// 배너는 transient alert 다. body 전체 텍스트로 판정하면 대화 transcript 의 문구 언급까지
// 매칭돼 false-positive(→ chat 을 잘못 busy 처리)가 난다. ARIA alert/status 컨테이너로
// 스코프해 transcript 와 구분한다. ⚠ 실제 배너의 정확한 컨테이너는 라이브 DOM 으로 확인
// 필요(§7-7) — 못 맞히면 backstop 이 조용히 no-op 이 되고 timeout 으로 폴백하므로 안전
// (위험한 false-positive 대신 안전한 false-negative 방향).
const SEL_BUSY_BANNER = '[role="alert"], [role="status"]';
// 턴 진행(응답 생성) 중 신호 후보 — turn_status 가 liveness 재확인에 쓴다.
// ⚠ 라이브 DOM 으로 정밀화 필요(protocol §7-7): stop 버튼/composer 비활성 셀렉터.
const SEL_STOP = '[data-testid="stop-button"], button[aria-label*="Stop" i]';

// off-screen: 화면 밖으로 던져 사용자 포커스/시야를 방해하지 않는다(설계 §1·§8).
// bringToFront() 는 절대 호출하지 않는다.
// anti-throttle: Chrome 은 occluded/off-screen 창을 idle 시 렌더링 throttle 하는데,
// 그러면 idle 후 navigate 시 요소가 'visible' 로 안 잡혀 chat composer 대기가 타임아웃
// 난다. 아래 플래그로 백그라운드 throttling 을 끈다.
const OFFSCREEN_ARGS = [
  '--window-position=-32000,-32000',
  '--disable-backgrounding-occluded-windows',
  '--disable-renderer-backgrounding',
  '--disable-background-timer-throttling',
];

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

// 로그인 전용 브라우저. Google OAuth 는 자동화 지문(navigator.webdriver /
// --enable-automation)을 탐지하면 "안전하지 않은 브라우저"로 로그인을 차단한다.
// 번들 Chromium 은 CF 는 통과하나 Google 에 걸리므로, 실제 시스템 Chrome
// (channel:'chrome')을 자동화 플래그 숨겨 띄운다. Chrome 미설치 시 번들로 폴백.
async function launchLoginBrowser(pw) {
  const base = {
    headless: false,
    ignoreDefaultArgs: ['--enable-automation'],
    args: ['--disable-blink-features=AutomationControlled'],
  };
  try {
    return await pw.chromium.launch({ ...base, channel: 'chrome' });
  } catch (e) {
    // 번들 Chromium 도 Google 에 걸릴 수 있다(공식 Chrome 이 없는 아키텍처, 예: arm64
    // Linux 는 이 우회조차 못 쓴다) — 그 경우 `tasty design import-session` 으로 로컬
    // Firefox 의 기존 로그인 세션을 가져오는 게 대안이다.
    log(
      'chrome channel unavailable, falling back to bundled chromium (may be blocked by ' +
        "Google's automation detection — if login hangs/fails, try `tasty design " +
        'import-session` instead):',
      e.message,
    );
    return await pw.chromium.launch(base);
  }
}

async function ensureBrowser() {
  if (browser && browser.isConnected()) return;
  const pw = loadPlaywright();
  // headless:false 가 핵심 — Cloudflare Managed Challenge 는 헤드리스 지문을 차단한다(조사
  // §5/§6). 나아가 번들 Chromium 은 헤드풀이어도 자동화 지문이 CF 챌린지에 묶여 통과가
  // 막히는 사례가 있어(로컬 실측), 로그인 브라우저처럼 실제 시스템 Chrome(channel:'chrome')을
  // 우선 쓴다 — 지문이 import 한 cf_clearance(같은 실제 Chrome 에서 추출)와 일치하고 봇 탐지도
  // 덜 걸린다. 자동화 플래그(navigator.webdriver / --enable-automation)는 숨긴다. Chrome
  // 미설치 시 번들로 폴백(그 경우 CF 에 막힐 수 있음 — import-session + 로그인 유지로 완화).
  const base = {
    headless: false,
    ignoreDefaultArgs: ['--enable-automation'],
    args: [...OFFSCREEN_ARGS, '--disable-blink-features=AutomationControlled'],
  };
  try {
    log('launching off-screen headful system Chrome (channel:chrome)...');
    browser = await pw.chromium.launch({ ...base, channel: 'chrome' });
  } catch (e) {
    log('chrome channel unavailable, falling back to bundled chromium (CF may challenge):', e.message);
    browser = await pw.chromium.launch(base);
  }
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
  // Cloudflare 챌린지 페이지 판정. title 은 브라우저 로케일에 따라 현지화되므로(예: 한국어
  // "잠시만 기다리십시오…" / "사람인지 확인하는 중") 영어 "Just a moment" 만 보면 오탐한다 —
  // 여러 로케일 title + CF 챌린지 DOM 마커(#challenge-running / turnstile)를 함께 본다.
  const cfTitle =
    /just a moment|잠시만 기다|사람인지|checking your browser|un momento|einen moment|checking if the site/i.test(
      title,
    );
  let cfBody = false;
  try {
    cfBody =
      (await page.locator('#challenge-running, #cf-challenge-running, [class*="cf-turnstile"], #cf-please-wait').count()) > 0;
  } catch (e) {
    // navigate 중 등으로 locator 실패 시 title 판정만 사용.
  }
  const cfChallenge = cfTitle || cfBody;
  const onDesign = isDesignAppUrl(url);
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
        loginBrowser = await launchLoginBrowser(pw);
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
          if (isDesignAppUrl(url)) { ok = true; break; }
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
      // 기계적 chat: 프로젝트 진입 → composer 입력 → send 클릭 → ReleaseTurn(턴 종료) 대기
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
        await composer.waitFor({ state: 'visible', timeout: 30000 });
        await composer.click();
        await composer.fill(req.message);

        // 전송 전 스레드 텍스트 baseline.
        const before = await page.evaluate((sel) => {
          const c = document.querySelector(sel);
          return c ? (c.innerText || '') : '';
        }, SEL_MESSAGES);

        // 턴 종료(ReleaseTurn) 대기를 클릭 전에 건다(전송 후 걸면 놓칠 수 있음). timeout 은
        // 디자인 턴이 길 수 있어 넉넉히.
        const timeoutMs = req.timeout_ms || 1800000;
        const turnReleased = page.waitForResponse((r) => RELEASE_TURN_RE.test(r.url()), { timeout: timeoutMs });
        await page.locator(SEL_SEND).click();

        // 전송 직후: 정상 턴(→ ReleaseTurn 으로 종료) vs "다른 탭 작업 중" 배너 경쟁. 배너가
        // 이기면 동시성 충돌이므로 timeout 까지 헛대기 말고 즉시 busy 로 보고(프로토콜 backstop).
        const busySeen = page
          .locator(SEL_BUSY_BANNER, { hasText: BUSY_TEXT_RE })
          .first()
          .waitFor({ state: 'visible', timeout: timeoutMs })
          .then(() => true)
          .catch(() => false);
        const race = await Promise.race([
          turnReleased.then(() => ({ kind: 'done' })).catch((e) => ({ kind: 'wait_err', e })),
          busySeen.then((seen) => (seen ? { kind: 'busy' } : { kind: 'none' })),
        ]);
        if (race.kind === 'busy') {
          send({ id, kind: 'busy', message: 'another tab is working on a request in this project' });
          break;
        }
        if (race.kind === 'wait_err') {
          throw race.e; // ReleaseTurn 대기 실패(진짜 타임아웃 등) → 아래 catch 로.
        }
        // ReleaseTurn 수신 = 턴 종료. DOM 반영 여유 후 응답 델타 추출.
        await page.waitForTimeout(800);

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

    case 'turn_status': {
      // 부작용 없음: 현재(또는 지정) 프로젝트 캔버스가 응답 생성/작업 중인지 관찰만 한다.
      // lock 프로토콜 TTL 만료 시 "정말 죽었나 vs 아직 작업 중인가" 재확인용(§5-2). 관찰
      // 이라 전송/충돌을 일으키지 않는다.
      try {
        await ensureBrowser();
        if (req.project) {
          const want = `/design/p/${req.project}`;
          if (!page.url().includes(want)) {
            await page.goto(projectUrl(req.project), { waitUntil: 'domcontentloaded', timeout: 30000 });
            await page.waitForTimeout(1200);
          }
        }
        const info = await page.evaluate((stopSel) => {
          // 배너는 ARIA alert/status 컨테이너 안의 문구로만 판정(대화 transcript 의 문구
          // 언급과 구분 — body 전체 텍스트 매칭은 false-positive).
          const alerts = Array.from(document.querySelectorAll('[role="alert"], [role="status"]'));
          const busyBanner = alerts.some((el) => /other tab is working/i.test(el.innerText || ''));
          const stopBtn = !!document.querySelector(stopSel);
          const composer = document.querySelector(
            '[data-testid="chat-composer-input"], div[role="textbox"].ProseMirror',
          );
          const composerDisabled = composer
            ? composer.getAttribute('contenteditable') === 'false' ||
              composer.getAttribute('aria-disabled') === 'true'
            : null;
          return { busyBanner, stopBtn, composerDisabled };
        }, SEL_STOP);
        // working = 응답 생성 중이거나 다른 탭이 프로젝트를 잡고 있음. (신호 정밀도는
        // §7-7 라이브 검증 대상 — 현재는 best-effort.)
        const working = info.stopBtn === true || info.busyBanner === true || info.composerDisabled === true;
        send({ id, kind: 'turn_status', working, ...info, url: page.url() });
      } catch (e) {
        send({ id, kind: 'error', code: 'turn_status_failed', message: e.message });
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
