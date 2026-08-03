//! IPC 메서드별 메타데이터 — plugin이 호출 가능한지, 어떤 권한이 필요한지.
//!
//! 이 테이블이 **단일 진실 원천**이다. 새 IPC 메서드를 추가할 때 반드시
//! 여기에도 등록한다. 매핑되지 않은 메서드는 [`method_meta`]가 `None`을 반환하며,
//! `CallerContext::ensure_allowed`가 plugin 호출을 거부한다.
//!
//! Local caller(CLI/사용자)는 권한 검사를 거치지 않는다. 이 테이블은 **plugin이
//! 호출했을 때**의 권한 요구사항이다.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use tasty_plugin_manifest::Permission;

/// 한 IPC 메서드에 대한 권한 메타.
#[derive(Debug, Clone, Copy)]
pub struct MethodMeta {
    /// plugin이 이 메서드를 호출할 수 있는지. false면 plugin은 어떤 경우에도 호출 불가.
    pub plugin_callable: bool,
    /// plugin이 호출하려면 매니페스트에 이 권한들이 모두 선언돼 있어야 함.
    pub required: &'static [Permission],
}

const fn plugin(required: &'static [Permission]) -> MethodMeta {
    MethodMeta {
        plugin_callable: true,
        required,
    }
}

const fn local_only() -> MethodMeta {
    MethodMeta {
        plugin_callable: false,
        required: &[],
    }
}

/// 등록된 IPC 메서드 — 단일 진실 원천. lint/검증 테스트가 이 테이블 위에서
/// 동작한다. 새 메서드는 여기에 추가한다.
///
/// prefix-기반 fallback(`surface.ime_*` 등)은 [`PREFIX_RULES`] 참조.
pub const METHOD_TABLE: &[(&str, MethodMeta)] = {
    use Permission::*;
    &[
        // ── 호스트 system ─────────────────────────────────────────────
        ("system.info", plugin(&[])),
        // GPU 리소스 카운트 read-only 스냅샷 (메모리 누수 soak 검증). 순수 조회지만
        // 내부 렌더러 구조를 노출하는 진단 표면이라 local_only — plugin 미노출.
        ("system.gpu_stats", local_only()),
        // ── workspace (read/write) ────────────────────────────────────
        ("workspace.list", plugin(&[SurfaceRead])),
        ("workspace.create", plugin(&[SurfaceWrite])),
        ("workspace.update", plugin(&[SurfaceWrite])),
        ("workspace.move", plugin(&[SurfaceWrite])),
        // ── workspace category (사이드바 폴더 CRUD) ──────────────────
        ("workspace_category.list", plugin(&[SurfaceRead])),
        ("workspace_category.create", plugin(&[SurfaceWrite])),
        ("workspace_category.rename", plugin(&[SurfaceWrite])),
        ("workspace_category.delete", plugin(&[SurfaceWrite])),
        ("workspace_category.move", plugin(&[SurfaceWrite])),
        // ── pane / split ──────────────────────────────────────────────
        ("pane.list", plugin(&[SurfaceRead])),
        ("pane.close", plugin(&[SurfaceWrite])),
        ("split", plugin(&[SurfaceWrite])),
        // ── tab ───────────────────────────────────────────────────────
        ("tab.list", plugin(&[SurfaceRead])),
        ("tab.create", plugin(&[SurfaceWrite])),
        ("tab.close", plugin(&[SurfaceWrite])),
        ("tab.move", plugin(&[SurfaceWrite])),
        // ── preset (layout preset CRUD + apply) ───────────────────────
        ("preset.list", plugin(&[SurfaceRead])),
        ("preset.get", plugin(&[SurfaceRead])),
        ("preset.save", plugin(&[SurfaceWrite])),
        ("preset.delete", plugin(&[SurfaceWrite])),
        ("preset.rename", plugin(&[SurfaceWrite])),
        ("preset.capture", plugin(&[SurfaceWrite])),
        ("preset.apply", plugin(&[SurfaceWrite])),
        // ── surface (구조 조작) ───────────────────────────────────────
        ("surface.list", plugin(&[SurfaceRead])),
        ("surface.close", plugin(&[SurfaceWrite])),
        ("surface.close_self", plugin(&[SurfaceWrite])),
        // tree/meta는 read 권한
        ("tree", plugin(&[SurfaceRead])),
        // webview — plugin 이 webview-enabled surface 의 URL 설정. SurfaceWrite 권한.
        ("webview.set_url", plugin(&[SurfaceWrite])),
        // surface.set_cwd — plugin 이 자기 RemoteSurface 의 cwd 를 host 에 통보.
        // 예: explorer 가 root 변경 시 carry 후보 cwd 갱신.
        ("surface.set_cwd", plugin(&[SurfaceWrite])),
        ("surface.meta.get", plugin(&[SurfaceRead])),
        ("surface.meta.list", plugin(&[SurfaceRead])),
        ("surface.meta.set", plugin(&[SurfaceWrite])),
        ("surface.meta.unset", plugin(&[SurfaceWrite])),
        // ── terminal I/O ──────────────────────────────────────────────
        ("surface.send", plugin(&[TerminalWrite])),
        ("surface.send_key", plugin(&[TerminalWrite])),
        ("surface.send_combo", plugin(&[TerminalWrite])),
        ("surface.send_to", plugin(&[TerminalWrite])),
        ("surface.send_wait_idle", plugin(&[TerminalWrite])),
        ("surface.wake", plugin(&[TerminalSpawn])),
        ("surface.set_mark", plugin(&[TerminalRead])),
        // completion 은 read 가 아니라 highlight(주의 환기) 발동 — PushNotification
        // 계열이므로 notification.* 와 동일한 Notification 권한.
        ("surface.completion", plugin(&[Notification])),
        ("surface.read_since_mark", plugin(&[TerminalRead])),
        ("surface.parse_since_mark", plugin(&[TerminalRead])),
        ("surface.commands", plugin(&[TerminalRead])),
        ("surface.last_command", plugin(&[TerminalRead])),
        ("surface.command_at", plugin(&[TerminalRead])),
        // ── output observer ─────────────────────────────────────────
        ("output.observe_start", plugin(&[TerminalRead])),
        ("output.observe_stop", plugin(&[TerminalRead])),
        ("output.observe_list", plugin(&[TerminalRead])),
        ("output.observe_info", plugin(&[TerminalRead])),
        ("surface.screen_text", plugin(&[TerminalRead])),
        ("surface.cursor_position", plugin(&[TerminalRead])),
        ("surface.foreground_process", plugin(&[TerminalRead])),
        ("surface.locate", plugin(&[SurfaceRead])),
        ("surface.respawn_terminal", plugin(&[TerminalSpawn])),
        ("surface.is_typing", plugin(&[TerminalRead])),
        // ── child-terminal 관리 (ADR-0040 / occupancy-04) ─────────────
        // 호스트가 내재화한 자식 터미널 registry. codex/claude plugin(05)이
        // 자체 registry 를 걷어내고 이 method 들로 위임한다. 권한은 각 method 가
        // 내부에서 조합하는 sibling 핸들러(tab.create=SurfaceWrite, surface.send=
        // TerminalWrite, surface.respawn_terminal=TerminalSpawn, surface.close=
        // SurfaceWrite)의 요구를 합집합으로 반영한다.
        (
            "terminal.spawn",
            plugin(&[SurfaceWrite, TerminalWrite, TerminalSpawn]),
        ),
        ("terminal.tell", plugin(&[TerminalWrite])),
        ("terminal.children", plugin(&[SurfaceRead])),
        ("terminal.parent", plugin(&[SurfaceRead])),
        // 자식 단건 상태 조회. children/parent 와 동일하게 순수 조회라
        // SurfaceRead 단독.
        ("terminal.state", plugin(&[SurfaceRead])),
        ("terminal.kill", plugin(&[SurfaceWrite])),
        ("terminal.respawn", plugin(&[TerminalWrite, TerminalSpawn])),
        ("terminal.broadcast", plugin(&[TerminalWrite])),
        // hook 이 idle/needs_input 신호를 호스트 registry 에 주입. 자식 상태 write.
        ("terminal.set_state", plugin(&[SurfaceWrite])),
        // 임의의 기존 surface 를 명시적으로 child 로 등록(soft 점유) —
        // `docs/features/child-terminal/index.md`("adopt" 절). sibling IPC 핸들러를
        // 호출하지 않고 순수 in-process core 함수(register_child/occupy_soft)만
        // 쓰므로 "child 관계 write" 성격의 SurfaceWrite 단독으로 충분
        // (terminal.kill/terminal.set_state 와 동일 컨벤션).
        ("terminal.adopt", plugin(&[SurfaceWrite])),
        // child 관계·soft 점유만 해제하고 surface 는 닫지 않음 —
        // `docs/features/child-terminal/index.md`("release" 절). adopt 와 대칭으로
        // 순수 in-process core 함수(remove_child/release_soft_occupancy)만 쓰므로
        // SurfaceWrite 단독.
        ("terminal.release", plugin(&[SurfaceWrite])),
        // ── headless PTY primitive (docs/adr/0050-headless-pty-primitive.md /
        // pty_registry) ────────────────────────────────────────────────
        // Surface 가 없는 백그라운드 PTY. child-terminal 과 달리 Surface 트리를
        // 전혀 건드리지 않으므로 Surface* 토큰이 섞이지 않는다 — 기존 Terminal* 3종만
        // 사용한다(위 ADR Decision 참고). spawn 은 Surface 를 안 만들어 SurfaceWrite
        // 불필요, wait 는 라이브 트리 대신 PtyEntry exit cell 로 판정해 SurfaceRead
        // 불필요, kill 은 Surface 를 닫지 않고 프로세스만 종료해 SurfaceWrite 불필요.
        ("pty.spawn", plugin(&[TerminalSpawn])),
        ("pty.write", plugin(&[TerminalWrite])),
        ("pty.read", plugin(&[TerminalRead])),
        ("pty.wait", plugin(&[TerminalRead])),
        ("pty.kill", plugin(&[TerminalWrite])),
        ("pty.list", plugin(&[TerminalRead])),
        // 승격 경로: headless PTY 를 실제 Surface 로 만든다 — spawn/write/... 와
        // 달리 Surface 트리를 새로 만들므로 terminal.spawn 과 동일하게 SurfaceWrite 를
        // 더한다(TerminalSpawn 은 새 터미널 surface 생성 권한).
        ("pty.attach_surface", plugin(&[SurfaceWrite, TerminalSpawn])),
        ("surface.fire_hook", plugin(&[SurfaceWrite])),
        // ── hooks ─────────────────────────────────────────────────────
        ("hook.set", plugin(&[SurfaceWrite])),
        ("hook.list", plugin(&[SurfaceRead])),
        ("hook.unset", plugin(&[SurfaceWrite])),
        ("global_hook.set", plugin(&[SurfaceWrite])),
        ("global_hook.list", plugin(&[SurfaceRead])),
        ("global_hook.unset", plugin(&[SurfaceWrite])),
        // ── webhook (인바운드 웹훅 리스너 — lifetime 6종/영속화 포함) ──────
        // register 만 plugin 호출 가능(Network 권한, S11). plugin 은 인라인
        // sequence 를 못 쓰고 자기 소유 hook 핸들러 id 만 바인딩할 수 있다(핸들러
        // 측 caller 게이트). 나머지 조회/해제/설정은 local_only(CLI/로컬 client).
        // register/unregister/sweep 는 웹훅 lifecycle 의미가 create/remove/clear
        // 보다 명확 — api-conventions "verb 화이트리스트" 정당화. sweep = 만료 정리.
        ("webhook.register", plugin(&[Network])),
        ("webhook.list", local_only()),
        ("webhook.info", local_only()),
        ("webhook.unregister", local_only()),
        ("webhook.sweep", local_only()),
        ("webhook.config", local_only()),
        // ── message (surface 간 메시지 큐) ─────────────────────────────
        ("message.send", plugin(&[SurfaceWrite])),
        ("message.read", plugin(&[SurfaceRead])),
        ("message.count", plugin(&[SurfaceRead])),
        ("message.clear", plugin(&[SurfaceWrite])),
        // ── image surface ─────────────────────────────────────────────
        // com.tasty.image plugin이 namespace를 점유하지만, 호스트 어댑터는
        // plugin 비활성 상태에서도 동작한다. plugin은 ipc.invoke:image 권한으로
        // 위 메서드들을 호출한다.
        ("image.open", plugin(&[SurfaceWrite, FsRead])),
        ("image.save", plugin(&[FsWrite])),
        ("image.export_png", plugin(&[FsWrite])),
        ("image.next", plugin(&[SurfaceWrite])),
        ("image.prev", plugin(&[SurfaceWrite])),
        ("image.paste", plugin(&[SurfaceWrite, ClipboardRead])),
        ("image.list", plugin(&[SurfaceRead])),
        // ── clipboard ──────────────────────────────────────────────────
        ("clipboard.set_text", plugin(&[ClipboardWrite])),
        // ── memory: regular (공유 네임스페이스, owner enforcement) ────
        ("memory.put", plugin(&[MemoryWrite])),
        ("memory.get", plugin(&[MemoryRead])),
        ("memory.delete", plugin(&[MemoryWrite])),
        ("memory.list", plugin(&[MemoryRead])),
        ("memory.exists", plugin(&[MemoryRead])),
        ("memory.count", plugin(&[MemoryRead])),
        ("memory.scopes", plugin(&[MemoryRead])),
        ("memory.stats", plugin(&[MemoryRead])),
        ("memory.query", plugin(&[MemoryRead])),
        ("memory.export", plugin(&[MemoryRead])),
        ("memory.import", plugin(&[MemoryWrite])),
        // ── memory: secret (plugin 별 사전 분할) ──────────────────────
        ("memory.secret.put", plugin(&[MemorySecret])),
        ("memory.secret.get", plugin(&[MemorySecret])),
        ("memory.secret.delete", plugin(&[MemorySecret])),
        ("memory.secret.list", plugin(&[MemorySecret])),
        ("memory.secret.exists", plugin(&[MemorySecret])),
        ("memory.secret.count", plugin(&[MemorySecret])),
        ("memory.secret.scopes", plugin(&[MemorySecret])),
        ("memory.secret.stats", plugin(&[MemorySecret])),
        // ── memory: 유지 보수 (host 전용) ─────────────────────────────
        ("memory.gc", local_only()),
        // ── memory: blackboard (workspace-scoped) ─────────────────────
        ("memory.bb_create", plugin(&[MemoryWrite])),
        ("memory.bb_put", plugin(&[MemoryWrite])),
        ("memory.bb_get", plugin(&[MemoryRead])),
        ("memory.bb_get_all", plugin(&[MemoryRead])),
        ("memory.bb_get_meta", plugin(&[MemoryRead])),
        ("memory.bb_delete_field", plugin(&[MemoryWrite])),
        ("memory.bb_delete", plugin(&[MemoryWrite])),
        ("memory.bb_list", plugin(&[MemoryRead])),
        ("memory.bb_exists", plugin(&[MemoryRead])),
        // ── memory: bb snapshot ────────────────────────────────────────
        ("memory.bb_snapshot", plugin(&[MemoryWrite])),
        ("memory.bb_snapshot_get", plugin(&[MemoryRead])),
        ("memory.bb_snapshot_list", plugin(&[MemoryRead])),
        ("memory.bb_snapshot_delete", plugin(&[MemoryWrite])),
        ("memory.bb_snapshot_restore", plugin(&[MemoryWrite])),
        // ── memory: plan (workspace-scoped) ───────────────────────────
        ("memory.plan_create", plugin(&[MemoryWrite])),
        ("memory.plan_get", plugin(&[MemoryRead])),
        ("memory.plan_list", plugin(&[MemoryRead])),
        ("memory.plan_delete", plugin(&[MemoryWrite])),
        ("memory.plan_add_step", plugin(&[MemoryWrite])),
        ("memory.plan_remove_step", plugin(&[MemoryWrite])),
        ("memory.plan_update_step", plugin(&[MemoryWrite])),
        // ── memory: cache (TTL 캐시) ───────────────────────────────────
        ("memory.cache_put", plugin(&[MemoryWrite])),
        ("memory.cache_get", plugin(&[MemoryRead])),
        ("memory.cache_invalidate", plugin(&[MemoryWrite])),
        ("memory.cache_clear", plugin(&[MemoryWrite])),
        ("memory.cache_list", plugin(&[MemoryRead])),
        // ── approval (휴먼 핸드오프) ──────────────────────────────────
        ("approval.request", plugin(&[Approval])),
        ("approval.respond", plugin(&[Approval])),
        // await 는 blocking + timeout 이라 main thread 가 막히면 안 됨.
        // process_ipc 에서 worker thread 로 분리 처리되며, plugin 호출은 미지원.
        ("approval.await", local_only()),
        ("approval.cancel", plugin(&[Approval])),
        ("approval.list", plugin(&[Approval])),
        ("approval.get", plugin(&[Approval])),
        ("approval.history", plugin(&[Approval])),
        // 세션 요약 — workspace 별 markdown 텍스트. memory.* 와 분리된 표면.
        ("approval.summary.set", plugin(&[Approval, MemoryWrite])),
        ("approval.summary.get", plugin(&[Approval, MemoryRead])),
        // ── telemetry (관측 / 비용) ───────────────────────────────────
        ("telemetry.record", plugin(&[Telemetry])),
        ("telemetry.record_batch", plugin(&[Telemetry])),
        ("telemetry.summary", plugin(&[Telemetry])),
        ("telemetry.timeseries", plugin(&[Telemetry])),
        ("telemetry.top", plugin(&[Telemetry])),
        ("telemetry.cap.set", plugin(&[Telemetry])),
        ("telemetry.cap.list", plugin(&[Telemetry])),
        ("telemetry.cap.remove", plugin(&[Telemetry])),
        ("telemetry.cap.status", plugin(&[Telemetry])),
        ("telemetry.cap.reset", plugin(&[Telemetry])),
        ("telemetry.anomaly.list", plugin(&[Telemetry])),
        ("telemetry.session_summary", plugin(&[Telemetry])),
        // ── agent (협업 primitive) ────────────────────────────────────
        ("agent.task_create", plugin(&[AgentManage])),
        ("agent.task_list", plugin(&[AgentManage])),
        ("agent.task_get", plugin(&[AgentManage])),
        ("agent.task_await", plugin(&[AgentManage])),
        ("agent.task_cancel", plugin(&[AgentManage])),
        ("agent.task_retry", plugin(&[AgentManage])),
        ("agent.task_graph", plugin(&[AgentManage])),
        // 자동 시작이 없으므로(재시작 정화는 부팅 경로 전용) plugin 이 자기
        // workspace 의 runner 를 스스로 되살릴 수단이 필요하다 — start/stop 은
        // idempotent, status 는 순수 조회.
        ("agent.task_run", plugin(&[AgentManage])),
        ("agent.barrier_create", plugin(&[AgentManage])),
        ("agent.barrier_signal", plugin(&[AgentManage])),
        ("agent.barrier_await", plugin(&[AgentManage])),
        ("agent.barrier_state", plugin(&[AgentManage])),
        ("agent.semaphore_create", plugin(&[AgentManage])),
        ("agent.semaphore_acquire", plugin(&[AgentManage])),
        ("agent.semaphore_release", plugin(&[AgentManage])),
        ("agent.barrier_list", plugin(&[AgentManage])),
        ("agent.barrier_delete", plugin(&[AgentManage])),
        ("agent.semaphore_list", plugin(&[AgentManage])),
        ("agent.semaphore_delete", plugin(&[AgentManage])),
        ("agent.lease_acquire", plugin(&[AgentManage])),
        ("agent.lease_release", plugin(&[AgentManage])),
        ("agent.lease_list", plugin(&[AgentManage])),
        ("agent.task_reduce", plugin(&[AgentManage])),
        ("agent.rate_limit_set", plugin(&[AgentManage])),
        ("agent.rate_limit_list", plugin(&[AgentManage])),
        ("agent.rate_limit_remove", plugin(&[AgentManage])),
        ("agent.rate_limit_status", plugin(&[AgentManage])),
        // ── session.* (자식 agent 신원 토큰) ──────────────────────────
        // issue/revoke 는 plugin 도 호출 가능 (claude plugin 등이 자식에게
        // 토큰을 발급해야 하므로). list 는 host 전용 — 감사/디버깅 목적이라
        // plugin 노출 불필요.
        ("session.issue", plugin(&[AgentManage])),
        ("session.revoke", plugin(&[AgentManage])),
        ("session.list", local_only()),
        // ── attach.* (배타 attach 점유 제어 — attach/detach 단계 3·4) ──────
        // surface 단위 배타 점유 lock 제어. acquire/release 는 주로 stream 핸드셰이크
        // (stream.open{target})로 일어나 method_meta 게이트를 거치지 않는다.
        // force_detach/list 는 JSON-RPC 요청-응답 경로라 여기 등록이 필요하다.
        //
        // 권한: decision 5 — attach 보안은 **연결 경계(SSH + 127.0.0.1 loopback)**에
        // 위임한다. 자체 권한 레이어를 두지 않으므로 추가 Permission 을 요구하지
        // 않는다(`plugin(&[])`). Local(별도 인스턴스 client)·인증된 agent 모두
        // 소켓에 도달했다면 attach 제어를 호출할 수 있다.
        ("attach.acquire", plugin(&[])),
        ("attach.release", plugin(&[])),
        ("attach.force_detach", plugin(&[])),
        ("attach.force_detach_workspace", plugin(&[])),
        ("attach.into_gui", plugin(&[])),
        ("attach.list", plugin(&[])),
        // ── remote.profile.* (원격 접속 프로필 CRUD) ─────────────────────
        // 프로필은 비밀 없는 장비 인벤토리(passkey 를 이름으로 참조만). attach.* 와 동일하게
        // 연결 경계(소켓 도달)에 신뢰를 위임 — 추가 Permission 불요.
        // (구 tool.ssh.* / ssh.profile.* 는 alias.rs 로 한시 호환.)
        ("remote.profile.list", plugin(&[])),
        ("remote.profile.get", plugin(&[])),
        ("remote.profile.add", plugin(&[])),
        ("remote.profile.detect", plugin(&[])),
        ("remote.profile.remove", plugin(&[])),
        // ── remote.passkey.* (자격증명 CRUD) ─────────────────────────────
        // 값 마스킹은 핸들러가 보장(list/get 은 name+kind 만, 파일 내용 미반환). 등록은
        // 쓰기라 허용. 권한은 프로필과 동일 — 연결 경계 위임(ADR-0016 / decision 7).
        ("remote.passkey.list", plugin(&[])),
        ("remote.passkey.get", plugin(&[])),
        ("remote.passkey.add", plugin(&[])),
        ("remote.passkey.remove", plugin(&[])),
        // ── notification ──────────────────────────────────────────────
        ("notification.list", plugin(&[Notification])),
        ("notification.create", plugin(&[Notification])),
        // ── settings (plugin 이 자기 plugin_settings 값을 read-back) ──────
        // [[contributes.settings_pages]] 를 선언하려면 이미 UiSettingsPage 권한이
        // 필요하므로(위 permission variant 재사용), 그 값을 다시 읽는 IPC 도
        // 동일 권한으로 게이트한다. caller_plugin_id 는 요청 파라미터가 아니라
        // CallerContext 에서 강제 도출 — 다른 plugin 값 조회 불가.
        ("settings.get_plugin_setting", plugin(&[UiSettingsPage])),
        // ── settings.remote_transfer (07 원격 전송 저장 정책 get/set) ──────
        // general settings 전역 read/write 라 plugin 권한 모델에 대응 variant 가
        // 없다 — memory.gc / system.gpu_stats 처럼 local_only 로 두어 plugin 에는
        // 노출하지 않고 로컬 IPC(CLI·에이전트)만 조작한다. focus 독립(전역 설정,
        // 대상 ID 불요). set 은 핸들러가 UpdateSettings intent 로 태워 collapse/save
        // 파이프라인을 재사용한다.
        ("settings.get_remote_transfer", local_only()),
        ("settings.set_remote_transfer", local_only()),
        // ── file_handler.* (host config 관리 — local-only) ───────────
        // user TOML 변경 후 재로드. plugin 이 호출할 일은 없으며 (자기 manifest
        // 도 reload 영향 밖이라) local 전용.
        ("file_handler.reload", local_only()),
        // 임의 경로를 file_handler dispatch 흐름에 진입시킨다. 임의 path 를
        // 읽고 (handler 가 OpenSurface 면 surface 의 param 으로, System 이면 OS
        // opener 가 읽음) 처리하므로 FsRead 권한 요구. explorer plugin 더블클릭
        // 같은 사용처가 주된 caller.
        ("file_handler.dispatch", plugin(&[FsRead])),
        // ── hook_handler.* (공유 훅 핸들러 레지스트리 — local-only) ───────
        // 웹훅/훅이 공유하는 핸들러 레지스트리 조회(list)/user config 재로드(reload)/
        // id 로 수동 발화(dispatch). webhook.* 와 동일하게 지금은 전부 local_only —
        // plugin 이 HookHandler 권한으로 list/dispatch 를 호출하는 실배선은 후속(S11).
        // reload 는 user config 변경 후 재읽기라 애초에 plugin 무관(local 전용).
        ("hook_handler.list", local_only()),
        ("hook_handler.reload", local_only()),
        ("hook_handler.dispatch", local_only()),
        // ── completion_strategy.* (완료 판정 전략 레지스트리 — local-only) ──
        // hook_handler.list 미러 — 등록된 전략(비활성 포함) 조회만.
        // reload/dispatch 대응물 없음: "발화" 개념이 없고(판정 함수일 뿐),
        // user config 재로드는 아직 노출하지 않는다(Settings UI CRUD 표면 없음).
        ("completion_strategy.list", local_only()),
        // markdown surface 제자리 이동 (04) — 주어진 surface 를 새 파일의 markdown
        // 으로 교체한다. 임의 path 를 읽으므로 FsRead. 주소창(03) 플러그인이 caller.
        ("markdown.navigate", plugin(&[FsRead])),
        // generic per-kind 최근목록 조회 — 주소창(03) 드롭다운 데이터 공급원(plugin 이
        // kind 를 채워 호출). 임의 파일 read 가 아니라 이미 열었던 목록 반환뿐이라 더
        // 약한 SurfaceRead 권한. host 는 특정 kind 이름을 모른다(generic).
        ("recent.query", plugin(&[SurfaceRead])),
        // ── fs.* (native 파일시스템 자원 위임 — host 프로세스 전용) ─────
        // native OS 파일 선택 다이얼로그(rfd)를 host 프로세스에서 열고 선택 경로를
        // 회신한다. plugin 은 자기 프로세스에서 native 다이얼로그(host UI 스레드 자원)를
        // 못 여므로 host 에 위임한다 — generic 하게 "파일 선택" 만 대행하고 host 는 특정
        // kind/plugin 을 모른다. 사용자가 임의 경로를 고르는 read 관심사라 FsRead. filters
        // 는 caller(예: markdown plugin 이 md/markdown)가 채운다. ADR-0042.
        ("fs.pick_file", plugin(&[FsRead])),
        // ── git_viewer.* (docs/adr/0056-git-viewer-remote-attach-git-query-channel.md
        // — 원격 attach mirror git 조회 트리거) ─
        // git-viewer plugin 이 mirror workspace 에서 status/log/worktrees snapshot
        // 또는 diff 를 요청. host 는 즉시 request_id 만 회신하고(비동기 accept), 실제
        // 조회는 attach Control 채널 왕복 후 `event.dispatch` unicast 로 plugin 에
        // push 된다(popup.set_context 는 이 결과 전달에 쓰지 않는다 — context 필드가
        // 없음). 임의 원격 경로 read 라 FsRead(로컬 fs.pick_file 과 동일 근거).
        ("git_viewer.query", plugin(&[FsRead])),
        // ── file_picker.* (plugin 트리거 host 소유 file_picker popup) ─
        // plugin(현재는 markdown Browse)이 host 소유 `file_picker` popup(ADR-0053)을
        // 열도록 트리거한다. host 는 즉시 request_id 만 회신하고(비동기 accept,
        // ADR-0058), 실제 확정/취소 결과는 확정 지점에서 `event.dispatch` unicast
        // `"file_picker.result"` 로 plugin 에 push 된다. 파일을 고르는 read 관심사라
        // FsRead(로컬 fs.pick_file/git_viewer.query 와 동일 근거).
        ("file_picker.trigger", plugin(&[FsRead])),
        // ── popup (plugin → host) ─────────────────────────────────────
        // 자기 contribute popup 인스턴스를 명시적으로 닫는다. METHOD_POPUP_CLOSED
        // (host → plugin)와는 다른 방향. plugin은 자기 instance_id만 닫을 수 있다 —
        // 다른 plugin의 인스턴스 close 요청은 만들어진 응답에서 거부.
        ("popup.close", plugin(&[UiPopup])),
        // ── banner (plugin → host, A3) ────────────────────────────────
        // 자기 contribute banner 를 자기 surface 에 띄운다(D1 소유권 검증은 App).
        ("banner.open", plugin(&[UiBanner])),
        // 자기 배너 인스턴스를 명시적으로 닫는다.
        ("banner.close", plugin(&[UiBanner])),
        // ── input source (macOS) ──────────────────────────────────────
        ("surface.switch_input_source", plugin(&[TerminalWrite])),
        ("surface.raw_key", plugin(&[TerminalWrite])),
        // ── 호스트 자체 메서드 (plugin/window 관리) — local-only ──────
        ("plugin.list", local_only()),
        ("plugin.show", local_only()),
        ("plugin.extension.list", local_only()),
        ("plugin.install", local_only()),
        ("plugin.remove", local_only()),
        ("plugin.enable", local_only()),
        ("plugin.disable", local_only()),
        ("plugin.permissions", local_only()),
        ("plugin.grant", local_only()),
        ("plugin.revoke", local_only()),
        // agent 임시 grant. grant/revoke 는 user/operator 만, list 는
        // readonly 라 plugin/agent 도 self-introspection 가능.
        ("plugin.grant_agent_permission", local_only()),
        ("plugin.revoke_agent_permission", local_only()),
        ("plugin.list_agent_permissions", plugin(&[])),
        // agent 가 자기 권한 부족을 미리 알고 elevation 을 명시
        // 발행할 entry point. approval.request 와 동일한 의미이므로 Approval
        // 권한이 필요.
        ("plugin.request_permission", plugin(&[Approval])),
        // audit log 조회/집계/삭제. 운영자 전용.
        ("plugin.audit_query", local_only()),
        ("plugin.audit_summary", local_only()),
        ("plugin.audit_follow", local_only()),
        ("plugin.audit_clear", local_only()),
        ("window.create", local_only()),
        ("window.close", local_only()),
        ("window.list", local_only()),
        // ui.screenshot — 정식 focus-독립 캡처. 대상 window/surface 를 ID 로 지정하며
        // focused 창에 의존하지 않는다(원칙 3). 임의 경로 파일 쓰기 표면이라 local_only
        // (plugin 미노출) — CLI/로컬 client 만 호출.
        ("ui.screenshot", local_only()),
        // E.C.e (D1=b) — Tasty 내부 어휘 통일에 따른 view.* alias. 동작은 window.* 와 동등.
        // wire format 호환을 위해 양쪽 메서드 명 모두 살림. payload 의 `window_id`
        // 필드는 외부 wire format 이라 변경 X.
        ("view.create", local_only()),
        ("view.close", local_only()),
        ("view.list", local_only()),
        // window.focus / view.focus 는 debug 빌드 전용 (DEBUG_METHODS 참조).
        // CLAUDE.md: 포커스 전환은 사용자 단축키/마우스 입력 영역.
        // script.reload (init.lua 재로드) 는 ADR-0031 에서 제거됨 — 스크립트는
        // 등록 목록 + 명시 트리거(단축키)로만 실행되고 부팅 자동로드가 폐기됐다.
    ]
};

/// debug 빌드에서만 등록되는 메서드. release에서는 [`method_meta`]가 `None`을
/// 반환해 IPC 표면에서 완전히 사라진다. 핸들러 함수 본체와 라우터 분기는
/// 이미 `#[cfg(debug_assertions)]`로 보호되어 있으므로, 표 등록만 게이트하면
/// 일관된 release 표면이 된다.
///
/// 카테고리:
/// - `system.shutdown` — 호스트 종료 (사용자가 직접 종료해야 하는 동작)
/// - `ui.state` — UI 상태 dump (디버깅용)
/// - `debug.*` — 사용자 입력 재현 / 디버그 dump
///
/// (`ui.screenshot` 은 focus-독립 리팩토링으로 [`METHOD_TABLE`] 로 승격됨.)
#[cfg(debug_assertions)]
pub const DEBUG_METHODS: &[(&str, MethodMeta)] = &[
    ("system.shutdown", local_only()),
    ("ui.state", local_only()),
    ("debug.info", local_only()),
    ("debug.cell_info", local_only()),
    ("debug.screen_attrs", local_only()),
    ("debug.glyph_color", local_only()),
    ("debug.feed_bytes", local_only()),
    ("debug.inject_mouse", local_only()),
    ("debug.inject_key", local_only()),
    // 마우스 라우팅 회귀 안전망용 read-only dump — 관찰 전용(사용자 상태 불변). release 미노출.
    ("debug.selection", local_only()),
    ("debug.pending_menu", local_only()),
    ("debug.focused_surface", local_only()),
    ("debug.tool.list", local_only()),
    ("debug.tool.invoke", local_only()),
    ("debug.popup.list", local_only()),
    ("debug.popup.open", local_only()),
    ("debug.popup.close", local_only()),
    ("debug.host_popup.list", local_only()),
    ("debug.host_popup.open", local_only()),
    ("debug.host_popup.close", local_only()),
    // modifier-hint 오버레이 홀드 주입/상태 덤프 — 사용자 modifier 홀드 재현. release 미노출.
    ("debug.modifier_hint.hold", local_only()),
    ("debug.modifier_hint.state", local_only()),
    // 설정 모달 강제 open — 사용자 조작 재현. release 미노출. 시각 검증 자동화용.
    ("debug.settings.open", local_only()),
    // 런타임 설정 patch 적용 — 사용자 "설정 저장" 재현. release 미노출.
    ("debug.settings.apply", local_only()),
    // 배너 직접 발화/조회/닫기/카운트다운 — 사용자 조작 재현. release 미노출.
    ("debug.banner.list", local_only()),
    ("debug.banner.show", local_only()),
    ("debug.banner.close", local_only()),
    ("debug.banner.set_countdown", local_only()),
    ("debug.plugin_banner.open", local_only()),
    ("debug.plugin_banner.close", local_only()),
    ("debug.event_bus.list_subscribers", local_only()),
    ("debug.event_bus.publish", local_only()),
    ("debug.event_bus.trace", local_only()),
    ("debug.extension.invoke_hook", local_only()),
    // 사용자 입력 재현 — 포커스 전환은 단축키/마우스 영역.
    // view.focus 는 window.focus 의 alias (E.C.e, D1=b). debug 빌드 only.
    ("window.focus", local_only()),
    ("view.focus", local_only()),
];
#[cfg(not(debug_assertions))]
pub const DEBUG_METHODS: &[(&str, MethodMeta)] = &[];

/// prefix 기반 fallback. METHOD_TABLE에 없는 메서드를 prefix로 매칭한다.
/// - `surface.ime_*` — IME 메서드 (window 의존, 사용자 입력 영역).
///
/// plugin 이 매니페스트 `[[contributes.ipc_namespace]]` 로 점유한 prefix 는
/// [`register_plugin_prefix`] 로 *runtime* 등록되어 `method_meta()` 의 마지막
/// fallback 단계에서 해소된다. 정적 `PREFIX_RULES` 는 host 자체 메서드의
/// prefix-fallback 전용.
pub const PREFIX_RULES: &[(&str, MethodMeta)] = &[("surface.ime_", local_only())];

/// plugin 매니페스트의 `[[contributes.ipc_namespace]]` 가 등록한 prefix 의
/// runtime registry. `method_meta()` 의 마지막 fallback 단계에서 조회된다.
///
/// host-plugin 의 plugin lifecycle (`start_plugin_internal` / `disable` /
/// `pump`) 가 [`register_plugin_prefix`] / [`unregister_plugin_prefix`] 로
/// 동기 갱신한다.
static PLUGIN_PREFIXES: OnceLock<RwLock<HashMap<String, MethodMeta>>> = OnceLock::new();

fn plugin_prefixes() -> &'static RwLock<HashMap<String, MethodMeta>> {
    PLUGIN_PREFIXES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// plugin 매니페스트의 `[[contributes.ipc_namespace]]` prefix 를 runtime 등록.
/// 등록 후 `<prefix>.<method>` 형식의 모든 IPC 메서드가 plugin/agent caller 에게
/// `plugin_callable=true, required=[]` 메타로 노출된다. 세부 권한은 host-plugin
/// 의 `validate_namespace_call` (`IpcInvoke(prefix)`) 가 분배.
///
/// 동일 prefix 재등록은 silent no-op (entry().or_insert(...) 시맨틱 — 첫
/// 등록자 유지). 한 번 unregister 로 완전 제거.
pub fn register_plugin_prefix(prefix: &str) {
    if let Ok(mut map) = plugin_prefixes().write() {
        map.entry(prefix.to_string()).or_insert(MethodMeta {
            plugin_callable: true,
            required: &[],
        });
    }
}

/// plugin unload / disable / restart 시 호출. 미등록 prefix 입력은 noop.
pub fn unregister_plugin_prefix(prefix: &str) {
    if let Ok(mut map) = plugin_prefixes().write() {
        map.remove(prefix);
    }
}

/// **WARNING**: runtime invariant 를 강제로 비움. 운영 호출 금지 — tests-only.
/// 외부 crate (host-plugin) 의 integration test 가 호출할 수 있도록
/// `doc(hidden) pub` 으로 노출.
#[doc(hidden)]
pub fn clear_plugin_prefixes_for_tests() {
    if let Ok(mut map) = plugin_prefixes().write() {
        map.clear();
    }
}

/// `prefix` 가 어떤 plugin 의 `[[contributes.ipc_namespace]]` 로 runtime 등록돼
/// 있는지 조회. host/user 소유 완료 판정 전략이 `_host` 권한으로 남의
/// plugin namespace 를 호출하는 권한 우회를 막는 데 쓰인다 — register/unregister
/// 는 기존에 있었으나 read 전용 조회가 없어 추가.
pub fn is_registered_plugin_prefix(prefix: &str) -> bool {
    plugin_prefixes()
        .read()
        .map(|map| map.contains_key(prefix))
        .unwrap_or(false)
}

/// 알려진 메서드의 메타. 미등록 메서드는 `None`.
pub fn method_meta(method: &str) -> Option<MethodMeta> {
    for (name, meta) in METHOD_TABLE {
        if *name == method {
            return Some(*meta);
        }
    }
    for (name, meta) in DEBUG_METHODS {
        if *name == method {
            return Some(*meta);
        }
    }
    for (prefix, meta) in PREFIX_RULES {
        if method.starts_with(prefix) {
            return Some(*meta);
        }
    }
    if let Some(dot) = method.find('.') {
        let prefix = &method[..dot];
        if let Ok(map) = plugin_prefixes().read()
            && let Some(meta) = map.get(prefix)
        {
            return Some(*meta);
        }
    }
    None
}

#[cfg(test)]
#[path = "method_meta_tests.rs"]
mod tests;
