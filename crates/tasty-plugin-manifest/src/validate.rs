//! `Manifest::load` + `Manifest::validate` + 각종 sub-validation 메서드.
//!
//! 자유 함수 형태의 형식 검사 헬퍼는 [`super::validators`] 모듈에 있고,
//! 본 모듈은 `impl Manifest` 와 `[extends]` hook 검증만 담당한다.

use std::collections::HashSet;
use std::path::Path;

use super::types::{
    AutoWaitDecl, BindingMode, CliCommandDecl, CliSubcommandDecl, CommandDecl, CommandScope,
    HOOK_TIMEOUT_MS_MAX, HOST_API_VERSION, HookMode, MANIFEST_VERSION, Manifest, Permission,
    PopupTrigger, PresetFieldInputType, SettingsItemDecl, SettingsPageContribute, SurfaceKindDecl,
    ToolAction, ToolContribute,
};
use super::validators::{
    event_pattern_covers, event_pattern_namespace, is_reserved_cli_name,
    is_reserved_event_namespace, is_reserved_hook_event_key, is_reserved_ipc_prefix,
    is_valid_cli_name, is_valid_command_id, is_valid_event_key, is_valid_event_pattern,
    is_valid_hook_event_key, is_valid_hook_handler_id, is_valid_ipc_prefix, is_valid_kind,
    is_valid_plugin_id, is_valid_settings_id, is_valid_simple_id, is_valid_tool_id,
};

impl Manifest {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join("tasty-plugin.toml");
        let s = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;
        let manifest: Manifest = toml::from_str(&s)
            .map_err(|e| anyhow::anyhow!("invalid manifest at {}: {}", path.display(), e))?;
        manifest.validate()?;
        // F.B.2/F.B.6 — opaque detector/handler payload 의 concrete schema 검증은
        // crate 외부 (본 바이너리 `plugin_bridge::manifest_validate`) 에서 수행.
        // 본 crate 의 `load` 는 schema-agnostic 검증까지만 수행하고, 호출처가 추가
        // 검증을 chain 한다.
        Ok(manifest)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.manifest_version != MANIFEST_VERSION {
            anyhow::bail!(
                "unsupported manifest_version: {} (expected {})",
                self.manifest_version,
                MANIFEST_VERSION
            );
        }
        if self.api_version != HOST_API_VERSION {
            anyhow::bail!(
                "plugin api_version '{}' incompatible with host '{}'",
                self.api_version,
                HOST_API_VERSION
            );
        }
        if !is_valid_plugin_id(&self.id) {
            anyhow::bail!(
                "invalid plugin id: '{}' (must be lowercase reverse-domain like com.example.x)",
                self.id
            );
        }
        for kind in &self.surface_kinds {
            if !is_valid_kind(&kind.kind) {
                anyhow::bail!(
                    "invalid surface kind: '{}' (must be lowercase ascii + '_' + digits)",
                    kind.kind
                );
            }
            self.validate_preset_fields(kind)?;
        }
        for raw in &self.permissions {
            if Permission::from_token(raw).is_none() {
                anyhow::bail!(
                    "unknown permission '{}' in manifest (host may be older than plugin)",
                    raw
                );
            }
        }
        self.validate_contributes()?;
        self.validate_event_patterns()?;
        self.validate_events_emitted()?;
        self.validate_hook_events()?;
        self.validate_extends()?;
        Ok(())
    }

    /// `[[surface_kinds.preset_fields]]` 검증.
    ///
    /// - `id`: settings id 규칙(소문자/숫자/`_`/`-`, 1..=64), kind 안에서 유일.
    /// - `label_key` / `param_key`: 비어있지 않음.
    /// - `derive_cwd`: `input_type = file_path` 에서만 유효(url/text/dir 은 경로 파생
    ///   의미가 없어 거부).
    /// - `required_params` 와의 정합(단일화): `preset_fields[].required=true` 가 진실원.
    ///   `required_params` 를 함께 선언하면 그 각 항목이 `required=true` 인 필드의
    ///   `param_key` 와 일치해야 한다(어긋나면 진실원 둘 → 거부).
    fn validate_preset_fields(&self, kind: &SurfaceKindDecl) -> anyhow::Result<()> {
        let mut seen_ids = HashSet::new();
        let mut required_param_keys = HashSet::new();
        for field in &kind.preset_fields {
            if !is_valid_settings_id(&field.id) {
                anyhow::bail!(
                    "invalid surface_kinds '{}' preset_field id '{}': must be lowercase ascii + digits + '_' + '-', length 1..=64",
                    kind.kind,
                    field.id
                );
            }
            if !seen_ids.insert(field.id.clone()) {
                anyhow::bail!(
                    "surface_kinds '{}' preset_field id '{}' declared twice",
                    kind.kind,
                    field.id
                );
            }
            if field.label_key.is_empty() {
                anyhow::bail!(
                    "surface_kinds '{}' preset_field '{}': label_key must not be empty",
                    kind.kind,
                    field.id
                );
            }
            if field.param_key.is_empty() {
                anyhow::bail!(
                    "surface_kinds '{}' preset_field '{}': param_key must not be empty",
                    kind.kind,
                    field.id
                );
            }
            if field.derive_cwd && field.input_type != PresetFieldInputType::FilePath {
                anyhow::bail!(
                    "surface_kinds '{}' preset_field '{}': derive_cwd is only valid for input_type = file_path",
                    kind.kind,
                    field.id
                );
            }
            if field.required {
                required_param_keys.insert(field.param_key.clone());
            }
        }
        // required_params 정합 — preset_fields 로 마이그레이션한 kind 에서 둘이 갈리는
        // 것을 막는다. preset_fields 가 진실원이므로, **preset_fields 를 선언한 경우에만**
        // required_params 각 항목이 `required=true` 필드의 param_key 와 일치하는지 검사한다.
        // (preset_fields 미선언 kind 의 legacy `required_params` 단독 선언은 그대로 허용 —
        // 미사용 메타라 대조할 진실원이 없다.)
        if !kind.preset_fields.is_empty() {
            for rp in &kind.required_params {
                if !required_param_keys.contains(rp) {
                    anyhow::bail!(
                        "surface_kinds '{}': required_params entry '{}' does not match any preset_field with required = true (preset_fields is the single source of truth)",
                        kind.kind,
                        rp
                    );
                }
            }
        }
        Ok(())
    }

    /// `[extends]` 블록 검증.
    ///
    /// - `plugin_id`는 유효한 plugin id 형식, 자기 자신을 가리키면 거부.
    /// - `version_req`는 semver 문법 (`semver::VersionReq::parse`로 검사).
    /// - `api_version`은 호스트의 `HOST_API_VERSION`과 일치.
    /// - hook 항목 최소 1개 필수 (zero hooks면 `[extends]` 무의미).
    /// - 각 hook: `timeout_ms ∈ [1, HOOK_TIMEOUT_MS_MAX]`.
    /// - event hook의 `event` 키는 정확한 키 (와일드카드 금지).
    /// - IPC hook의 `method`는 비어 있지 않은 정규 메서드 이름.
    ///
    /// 권한 매칭(`ext.modify_output:*`, `ext.modify_input:*`)과 target 호환성 검증은
    /// ExtensionRegistry 활성화 단계에서 수행한다 (target 매니페스트가 필요하므로).
    fn validate_extends(&self) -> anyhow::Result<()> {
        let Some(decl) = &self.extends else {
            return Ok(());
        };
        if !is_valid_plugin_id(&decl.plugin_id) {
            anyhow::bail!(
                "invalid extends.plugin_id '{}': must be lowercase reverse-domain",
                decl.plugin_id
            );
        }
        if decl.plugin_id == self.id {
            anyhow::bail!("extends.plugin_id must differ from this plugin's own id");
        }
        if let Err(e) = semver::VersionReq::parse(&decl.version_req) {
            anyhow::bail!("invalid extends.version_req '{}': {}", decl.version_req, e);
        }
        if decl.api_version != HOST_API_VERSION {
            anyhow::bail!(
                "extends.api_version '{}' incompatible with host '{}'",
                decl.api_version,
                HOST_API_VERSION
            );
        }
        let required_token = format!("ext:{}", decl.plugin_id);
        if !self.permissions.iter().any(|p| p == &required_token) {
            anyhow::bail!(
                "[extends] requires permission '{required_token}' to be declared in manifest permissions[]"
            );
        }
        let total =
            decl.pre_event.len() + decl.post_event.len() + decl.pre_ipc.len() + decl.post_ipc.len();
        if total == 0 {
            anyhow::bail!(
                "[extends] block must declare at least one hook (pre_event / post_event / pre_ipc / post_ipc)"
            );
        }

        for h in decl.pre_event.iter().chain(decl.post_event.iter()) {
            validate_hook_timeout(h.timeout_ms, &h.event)?;
            if !is_valid_event_key(&h.event) {
                anyhow::bail!(
                    "extends event hook key '{}' must be a concrete event key (no '*')",
                    h.event
                );
            }
            validate_hook_mode_modifies(h.mode, &h.modifies, &h.event)?;
        }
        for h in decl.pre_ipc.iter().chain(decl.post_ipc.iter()) {
            validate_hook_timeout(h.timeout_ms, &h.method)?;
            if h.method.is_empty() || h.method.contains('*') {
                anyhow::bail!(
                    "extends ipc hook method '{}' must be a concrete method name",
                    h.method
                );
            }
            validate_hook_mode_modifies(h.mode, &h.modifies, &h.method)?;
        }
        Ok(())
    }

    /// `events_emitted` 카탈로그 검증.
    ///
    /// - key는 정확한 이벤트 키여야 한다 (와일드카드 불가).
    /// - key는 예약 네임스페이스를 쓸 수 없다.
    /// - key는 매니페스트의 `event_publish` 패턴 중 하나에 의해 *cover*되어야 한다.
    ///   (실제 publish 시점에도 같은 검사가 적용되므로 일관성 보장)
    /// - 같은 key를 두 번 선언하면 거부 (의미 없는 중복).
    fn validate_events_emitted(&self) -> anyhow::Result<()> {
        let mut seen: HashSet<&str> = HashSet::new();
        for decl in &self.events_emitted {
            if !is_valid_event_key(&decl.key) {
                anyhow::bail!(
                    "invalid events_emitted key '{}': must be a concrete event key (no '*')",
                    decl.key
                );
            }
            let ns = event_pattern_namespace(&decl.key);
            if is_reserved_event_namespace(ns) {
                anyhow::bail!(
                    "events_emitted key '{}' uses reserved namespace '{}' — \
                     only the host may publish in this namespace",
                    decl.key,
                    ns
                );
            }
            let covered = self
                .event_publish
                .iter()
                .any(|p| event_pattern_covers(p, &decl.key));
            if !covered {
                anyhow::bail!(
                    "events_emitted key '{}' is not covered by any event_publish pattern",
                    decl.key
                );
            }
            if !seen.insert(decl.key.as_str()) {
                anyhow::bail!("events_emitted key '{}' declared twice", decl.key);
            }
        }
        Ok(())
    }

    /// `[[contributes.hook_events]]` surface hook 이벤트 카탈로그 검증.
    ///
    /// - key는 유효한 hook 이벤트 키 형식이어야 한다 (kebab-case, 와일드카드 불가).
    /// - key는 내장 이벤트(`process-exit`/`bell`/`notification`/`output-match:`/
    ///   `idle-timeout:`)와 충돌할 수 없다 (코어 parse 가 내장 변형으로 먼저 가로채
    ///   plugin 선언이 죽으므로).
    /// - 같은 key를 두 번 선언하면 거부.
    fn validate_hook_events(&self) -> anyhow::Result<()> {
        let mut seen: HashSet<&str> = HashSet::new();
        for decl in &self.contributes.hook_events {
            if !is_valid_hook_event_key(&decl.key) {
                anyhow::bail!(
                    "invalid contributes.hook_events key '{}': must be lowercase ascii + digits + '-', \
                     start with a letter, length ≤ 64, no '*'/':'/'.'",
                    decl.key
                );
            }
            if is_reserved_hook_event_key(&decl.key) {
                anyhow::bail!(
                    "contributes.hook_events key '{}' collides with a built-in hook event — \
                     built-in events (process-exit, bell, notification, output-match:, idle-timeout:) \
                     cannot be declared by plugins",
                    decl.key
                );
            }
            if !seen.insert(decl.key.as_str()) {
                anyhow::bail!("contributes.hook_events key '{}' declared twice", decl.key);
            }
        }
        Ok(())
    }

    /// `event_subscribe`/`event_publish` 패턴 검증.
    ///
    /// 규칙:
    /// - 빈 문자열, 단독 `"*"` 거부 (모든 이벤트 일괄 매칭 금지)
    /// - 와일드카드는 끝의 `.<segment>` 자리에만 허용 (`foo.*`, `foo.bar.*`)
    /// - 중간/시작 와일드카드(`*.bar`, `f*`) 거부
    /// - 각 세그먼트: 소문자 ascii + 숫자 + `_`. 알파벳으로 시작.
    /// - `event_publish`는 예약 네임스페이스(`surface`, `system`, `tab`, ...)를 거부.
    fn validate_event_patterns(&self) -> anyhow::Result<()> {
        for p in &self.event_subscribe {
            if !is_valid_event_pattern(p) {
                anyhow::bail!(
                    "invalid event_subscribe pattern '{}': must be a key or '<ns>.*' \
                     (segments: lowercase ascii + digits + '_', start with a letter)",
                    p
                );
            }
        }
        for p in &self.event_publish {
            if !is_valid_event_pattern(p) {
                anyhow::bail!(
                    "invalid event_publish pattern '{}': must be a key or '<ns>.*'",
                    p
                );
            }
            let ns = event_pattern_namespace(p);
            if is_reserved_event_namespace(ns) {
                anyhow::bail!(
                    "event_publish pattern '{}' uses reserved namespace '{}' — \
                     only the host may publish in this namespace",
                    p,
                    ns
                );
            }
        }
        Ok(())
    }

    fn validate_contributes(&self) -> anyhow::Result<()> {
        let seen_prefixes = self.validate_contributed_ipc_namespaces()?;
        self.validate_contributed_cli(&seen_prefixes)?;
        self.validate_contributed_tools()?;
        self.validate_contributed_commands()?;
        self.validate_contributed_popups()?;
        self.validate_contributed_banners()?;
        self.validate_contributed_windows()?;
        self.validate_contributed_settings_pages()?;
        self.validate_contributed_detectors()?;
        self.validate_contributed_handlers()?;
        self.validate_contributed_detector_permissions()?;
        self.validate_contributed_hook_handlers()?;
        Ok(())
    }

    fn validate_contributed_ipc_namespaces(&self) -> anyhow::Result<HashSet<String>> {
        let mut seen_prefixes = HashSet::new();
        for ns in &self.contributes.ipc_namespace {
            if !is_valid_ipc_prefix(&ns.prefix) {
                anyhow::bail!(
                    "invalid ipc_namespace prefix '{}': must be lowercase ascii + digits + '_', \
                     start with a letter, length ≤ 32, no '.'",
                    ns.prefix
                );
            }
            if is_reserved_ipc_prefix(&ns.prefix) {
                anyhow::bail!(
                    "ipc_namespace prefix '{}' is reserved by the host",
                    ns.prefix
                );
            }
            if !seen_prefixes.insert(ns.prefix.clone()) {
                anyhow::bail!(
                    "ipc_namespace prefix '{}' declared twice in this manifest",
                    ns.prefix
                );
            }
        }
        Ok(seen_prefixes)
    }

    fn validate_contributed_cli(&self, seen_prefixes: &HashSet<String>) -> anyhow::Result<()> {
        let mut seen_cli_names = HashSet::new();
        for cli in &self.contributes.cli {
            if !is_valid_cli_name(&cli.name) {
                anyhow::bail!(
                    "invalid cli name '{}': must be lowercase ascii + digits + '-', \
                     start with a letter, length ≤ 32",
                    cli.name
                );
            }
            if is_reserved_cli_name(&cli.name) {
                anyhow::bail!("cli name '{}' is reserved by the host", cli.name);
            }
            if !seen_cli_names.insert(cli.name.clone()) {
                anyhow::bail!("cli name '{}' declared twice in this manifest", cli.name);
            }
            Self::validate_cli_subcommands(cli, seen_prefixes, &self.id)?;
            Self::validate_cli_arg_groups(cli)?;
        }
        Ok(())
    }

    fn validate_cli_subcommands(
        cli: &CliCommandDecl,
        seen_prefixes: &HashSet<String>,
        plugin_id: &str,
    ) -> anyhow::Result<()> {
        let mut seen_sub_names = HashSet::new();
        for sub in &cli.subcommands {
            if !is_valid_cli_name(&sub.name) {
                anyhow::bail!(
                    "invalid cli subcommand name '{}' under '{}'",
                    sub.name,
                    cli.name
                );
            }
            if !seen_sub_names.insert(sub.name.clone()) {
                anyhow::bail!(
                    "cli subcommand name '{}' declared twice under '{}'",
                    sub.name,
                    cli.name
                );
            }
            if !cli.arg_groups.contains_key(&sub.args) {
                anyhow::bail!(
                    "cli subcommand '{} {}' references unknown arg group '{}'",
                    cli.name,
                    sub.name,
                    sub.args
                );
            }
            if sub.polling.is_some() && sub.auto_wait.is_some() {
                anyhow::bail!(
                    "cli subcommand '{} {}' declares both 'polling' and 'auto_wait' \
                         — choose one (polling = self-poll, auto_wait = chain to another method)",
                    cli.name,
                    sub.name
                );
            }
            if let Some(aw) = &sub.auto_wait {
                Self::validate_auto_wait_strategy(cli, sub, aw, plugin_id)?;
            }
            // ipc_method는 plugin 자기 namespace로 시작해야 한다.
            let Some(dot) = sub.ipc_method.find('.') else {
                anyhow::bail!(
                    "cli subcommand '{} {}' ipc_method '{}' has no namespace prefix",
                    cli.name,
                    sub.name,
                    sub.ipc_method
                );
            };
            let prefix = &sub.ipc_method[..dot];
            if !seen_prefixes.contains(prefix) {
                anyhow::bail!(
                    "cli subcommand '{} {}' ipc_method '{}' uses prefix '{}' \
                         which is not declared in this plugin's ipc_namespace",
                    cli.name,
                    sub.name,
                    sub.ipc_method,
                    prefix
                );
            }
        }
        Ok(())
    }

    /// `AutoWaitDecl.polling`/`.strategy` 는 정확히 하나만 선언해야 한다. `strategy`
    /// 를 선언했으면 값은 `<plugin_id>/<short-name>` 형식이고, prefix 는 반드시
    /// 이 매니페스트 자신의 `id` 와 같아야 한다 — 다른 plugin 의 completion
    /// strategy 를 이름으로 훔쳐 참조하는 것을 여기서 막는다(CLI 는 어차피 같은
    /// 매니페스트 안에서만 이름을 해석하므로, 이 검증은 조기 실패용 방어선).
    fn validate_auto_wait_strategy(
        cli: &CliCommandDecl,
        sub: &CliSubcommandDecl,
        aw: &AutoWaitDecl,
        plugin_id: &str,
    ) -> anyhow::Result<()> {
        match (&aw.polling, &aw.strategy) {
            (Some(_), Some(_)) => anyhow::bail!(
                "cli subcommand '{} {}' auto_wait declares both 'polling' and 'strategy' \
                     — choose one",
                cli.name,
                sub.name
            ),
            (None, None) => anyhow::bail!(
                "cli subcommand '{} {}' auto_wait declares neither 'polling' nor 'strategy'",
                cli.name,
                sub.name
            ),
            (Some(_), None) => Ok(()),
            (None, Some(strategy)) => {
                let Some(slash) = strategy.find('/') else {
                    anyhow::bail!(
                        "cli subcommand '{} {}' auto_wait.strategy '{}' has no owner prefix \
                             (expected '<plugin_id>/<short-name>')",
                        cli.name,
                        sub.name,
                        strategy
                    );
                };
                let prefix = &strategy[..slash];
                if prefix != plugin_id {
                    anyhow::bail!(
                        "cli subcommand '{} {}' auto_wait.strategy '{}' prefix '{}' does not \
                             match this plugin's id '{}' — a plugin can only reference its own \
                             completion strategies",
                        cli.name,
                        sub.name,
                        strategy,
                        prefix,
                        plugin_id
                    );
                }
                let short_name = &strategy[slash + 1..];
                if !is_valid_hook_handler_id(short_name) {
                    anyhow::bail!(
                        "cli subcommand '{} {}' auto_wait.strategy '{}' short-name '{}' is \
                             invalid (must match [a-z0-9-]{{1,32}})",
                        cli.name,
                        sub.name,
                        strategy,
                        short_name
                    );
                }
                Ok(())
            }
        }
    }

    fn validate_cli_arg_groups(cli: &CliCommandDecl) -> anyhow::Result<()> {
        // arg group 내부 정합성: flag는 flags에만, positional은 flag 필드 없음.
        for (group_name, group) in &cli.arg_groups {
            for arg in &group.positional {
                if arg.flag.is_some() {
                    anyhow::bail!(
                        "arg group '{}.{}' positional arg '{}' must not have a 'flag' field",
                        cli.name,
                        group_name,
                        arg.name
                    );
                }
            }
            for arg in &group.flags {
                let Some(flag) = &arg.flag else {
                    anyhow::bail!(
                        "arg group '{}.{}' flag arg '{}' is missing 'flag' field",
                        cli.name,
                        group_name,
                        arg.name
                    );
                };
                if !flag.starts_with("--") {
                    anyhow::bail!(
                        "arg group '{}.{}' flag '{}' must start with '--'",
                        cli.name,
                        group_name,
                        flag
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_contributed_tools(&self) -> anyhow::Result<()> {
        // [[contributes.tool]] 검증.
        if !self.contributes.tool.is_empty() {
            if !self.permissions.iter().any(|p| p == "ui.tool_item") {
                anyhow::bail!(
                    "[[contributes.tool]] requires permission 'ui.tool_item' to be declared in manifest permissions[]"
                );
            }
            let mut seen_tool_ids = HashSet::new();
            let surface_kinds: HashSet<&str> =
                self.surface_kinds.iter().map(|k| k.kind.as_str()).collect();
            for tool in &self.contributes.tool {
                if !is_valid_tool_id(&tool.id) {
                    anyhow::bail!(
                        "invalid contributes.tool id '{}': must be lowercase ascii + digits + '-', \
                         start with a letter, length ≤ 64",
                        tool.id
                    );
                }
                if !seen_tool_ids.insert(tool.id.clone()) {
                    anyhow::bail!(
                        "contributes.tool id '{}' declared twice in this manifest",
                        tool.id
                    );
                }
                if tool.label_i18n_key.is_empty() {
                    anyhow::bail!(
                        "contributes.tool '{}': label_i18n_key must not be empty",
                        tool.id
                    );
                }
                Self::validate_tool_action(tool, &surface_kinds)?;
            }
        }
        Ok(())
    }

    fn validate_tool_action(
        tool: &ToolContribute,
        surface_kinds: &HashSet<&str>,
    ) -> anyhow::Result<()> {
        Self::validate_action("contributes.tool", &tool.id, &tool.action, surface_kinds)
    }

    /// `ToolAction`(`[[contributes.tool]].action` 및 `[[contributes.commands]].action`이
    /// 공유하는 타입) 형식 검증. `owner_label`은 에러 메시지에 쓰이는 소속 표기
    /// (`"contributes.tool"` / `"contributes.commands"`).
    fn validate_action(
        owner_label: &str,
        id: &str,
        action: &ToolAction,
        surface_kinds: &HashSet<&str>,
    ) -> anyhow::Result<()> {
        match action {
            ToolAction::Event { event_key } => {
                if !is_valid_event_key(event_key) {
                    anyhow::bail!(
                        "{} '{}': action.event_key '{}' must be a concrete event key",
                        owner_label,
                        id,
                        event_key
                    );
                }
            }
            ToolAction::OpenSurface { surface_kind } => {
                if !surface_kinds.contains(surface_kind.as_str()) {
                    anyhow::bail!(
                        "{} '{}': action.surface_kind '{}' is not declared in this plugin's [[surface_kinds]]",
                        owner_label,
                        id,
                        surface_kind
                    );
                }
            }
            ToolAction::OpenPopup { popup_id } => {
                // popup contribute는 phase2-popup에서 도입. 그 전까지 형식만 검사.
                if popup_id.is_empty() {
                    anyhow::bail!(
                        "{} '{}': action.popup_id must not be empty",
                        owner_label,
                        id
                    );
                }
            }
        }
        Ok(())
    }

    /// `[[contributes.commands]]` 검증: id 형식/중복, `binding_mode = "inherit:<action>"`
    /// 대상이 호스트 화이트리스트에 있는지, `scope = "global"` 커맨드가 단일 키를
    /// 쓰지 않는지(문서상 "조합키만 권장"을 여기서 강제), `action`(있다면) 형식.
    fn validate_contributed_commands(&self) -> anyhow::Result<()> {
        if self.contributes.commands.is_empty() {
            return Ok(());
        }
        let has_popup_action = self
            .contributes
            .commands
            .iter()
            .any(|c| matches!(&c.action, Some(ToolAction::OpenPopup { .. })));
        if has_popup_action && !self.permissions.iter().any(|p| p == "ui.popup") {
            anyhow::bail!(
                "[[contributes.commands]] with action.kind = 'open_popup' requires permission \
                 'ui.popup' to be declared in manifest permissions[]"
            );
        }
        let surface_kinds: HashSet<&str> =
            self.surface_kinds.iter().map(|k| k.kind.as_str()).collect();
        let mut seen_command_ids = HashSet::new();
        for cmd in &self.contributes.commands {
            Self::validate_command(cmd, &surface_kinds)?;
            if !seen_command_ids.insert(cmd.id.clone()) {
                anyhow::bail!(
                    "contributes.commands id '{}' declared twice in this manifest",
                    cmd.id
                );
            }
        }
        Ok(())
    }

    fn validate_command(cmd: &CommandDecl, surface_kinds: &HashSet<&str>) -> anyhow::Result<()> {
        if !is_valid_command_id(&cmd.id) {
            anyhow::bail!(
                "invalid contributes.commands id '{}': must be '.'-separated lowercase ascii + \
                 digits + '_' segments (e.g. 'explorer.refresh'), length ≤ 64",
                cmd.id
            );
        }
        if cmd.title_i18n_key.is_empty() {
            anyhow::bail!(
                "contributes.commands '{}': title_i18n_key must not be empty",
                cmd.id
            );
        }
        if let BindingMode::InheritHost(action) = &cmd.binding_mode
            && !crate::host_actions::is_inheritable(action)
        {
            anyhow::bail!(
                "contributes.commands '{}': binding_mode 'inherit:{}' refers to a host action \
                 not in the inherit whitelist",
                cmd.id,
                action
            );
        }
        if cmd.scope == CommandScope::Global
            && let Some(kb) = &cmd.default_keybinding
            && !kb.is_empty()
            && !kb.contains('+')
        {
            anyhow::bail!(
                "contributes.commands '{}': scope 'global' commands must bind a combination key \
                 (default_keybinding '{}' has no modifier) — single keys are reserved for scope 'surface'",
                cmd.id,
                kb
            );
        }
        if let Some(action) = &cmd.action {
            Self::validate_action("contributes.commands", &cmd.id, action, surface_kinds)?;
        }
        Ok(())
    }

    fn validate_contributed_popups(&self) -> anyhow::Result<()> {
        // [[contributes.popup]] 검증.
        if !self.contributes.popup.is_empty() {
            if !self.permissions.iter().any(|p| p == "ui.popup") {
                anyhow::bail!(
                    "[[contributes.popup]] requires permission 'ui.popup' to be declared in manifest permissions[]"
                );
            }
            self.validate_popup_defs()?;
            self.validate_tool_popup_refs()?;
        }
        Ok(())
    }

    fn validate_popup_defs(&self) -> anyhow::Result<()> {
        let mut seen_popup_ids = HashSet::new();
        for popup in &self.contributes.popup {
            if !is_valid_tool_id(&popup.id) {
                anyhow::bail!(
                    "invalid contributes.popup id '{}': must be lowercase ascii + digits + '-', \
                         start with a letter, length ≤ 64",
                    popup.id
                );
            }
            if !seen_popup_ids.insert(popup.id.clone()) {
                anyhow::bail!(
                    "contributes.popup id '{}' declared twice in this manifest",
                    popup.id
                );
            }
            if let PopupTrigger::Event { event_key } = &popup.trigger
                && !is_valid_event_key(event_key)
            {
                anyhow::bail!(
                    "contributes.popup '{}': trigger.event_key '{}' must be a concrete event key",
                    popup.id,
                    event_key
                );
            }
            if let Some(sz) = &popup.size_hint
                && (sz.width == 0 || sz.height == 0)
            {
                anyhow::bail!(
                    "contributes.popup '{}': size_hint width/height must be > 0",
                    popup.id
                );
            }
        }
        Ok(())
    }

    fn validate_tool_popup_refs(&self) -> anyhow::Result<()> {
        // [[contributes.tool]]/[[contributes.commands]] action.open_popup이 이 plugin의
        // popup id를 가리킬 때 해당 id가 실제로 존재해야 한다.
        let popup_ids: HashSet<&str> = self
            .contributes
            .popup
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        for tool in &self.contributes.tool {
            Self::validate_popup_ref(
                "contributes.tool",
                &tool.id,
                &tool.action,
                &self.id,
                &popup_ids,
            )?;
        }
        for cmd in &self.contributes.commands {
            if let Some(action) = &cmd.action {
                Self::validate_popup_ref(
                    "contributes.commands",
                    &cmd.id,
                    action,
                    &self.id,
                    &popup_ids,
                )?;
            }
        }
        Ok(())
    }

    fn validate_popup_ref(
        owner_label: &str,
        id: &str,
        action: &ToolAction,
        plugin_id: &str,
        popup_ids: &HashSet<&str>,
    ) -> anyhow::Result<()> {
        if let ToolAction::OpenPopup { popup_id } = action
            && let Some(local_id) = popup_id.strip_prefix(&format!("{plugin_id}/"))
            && !popup_ids.contains(local_id)
        {
            anyhow::bail!(
                "{} '{}': action.popup_id '{}' references unknown popup in this plugin",
                owner_label,
                id,
                popup_id
            );
        }
        Ok(())
    }

    fn validate_contributed_banners(&self) -> anyhow::Result<()> {
        // [[contributes.banner]] 검증 (A3).
        if !self.contributes.banner.is_empty() {
            if !self.permissions.iter().any(|p| p == "ui.banner") {
                anyhow::bail!(
                    "[[contributes.banner]] requires permission 'ui.banner' to be declared in manifest permissions[]"
                );
            }
            let mut seen_banner_ids = HashSet::new();
            for banner in &self.contributes.banner {
                if !is_valid_tool_id(&banner.id) {
                    anyhow::bail!(
                        "invalid contributes.banner id '{}': must be lowercase ascii + digits + '-', \
                         start with a letter, length ≤ 64",
                        banner.id
                    );
                }
                if !seen_banner_ids.insert(banner.id.clone()) {
                    anyhow::bail!(
                        "contributes.banner id '{}' declared twice in this manifest",
                        banner.id
                    );
                }
                if let Some(sz) = &banner.size_hint
                    && sz.height == 0
                {
                    anyhow::bail!(
                        "contributes.banner '{}': size_hint height must be > 0",
                        banner.id
                    );
                }
                if let Some(ttl) = banner.ttl_seconds
                    && ttl == 0
                {
                    anyhow::bail!(
                        "contributes.banner '{}': ttl_seconds must be > 0 (omit for a persistent banner)",
                        banner.id
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_contributed_windows(&self) -> anyhow::Result<()> {
        // [[contributes.window]] 검증.
        if !self.contributes.window.is_empty() {
            if !self.permissions.iter().any(|p| p == "window.spawn") {
                anyhow::bail!(
                    "[[contributes.window]] requires permission 'window.spawn' to be declared in manifest permissions[]"
                );
            }
            let mut seen_window_ids = HashSet::new();
            for w in &self.contributes.window {
                if !is_valid_kind(&w.id) {
                    anyhow::bail!(
                        "invalid contributes.window id '{}': must be lowercase ascii + '_' + digits",
                        w.id
                    );
                }
                if !seen_window_ids.insert(w.id.clone()) {
                    anyhow::bail!(
                        "contributes.window id '{}' declared twice in this manifest",
                        w.id
                    );
                }
                if w.display_name_i18n_key.is_empty() {
                    anyhow::bail!(
                        "contributes.window '{}': display_name_i18n_key must not be empty",
                        w.id
                    );
                }
                if let Some(sz) = &w.default_size
                    && (sz.width == 0 || sz.height == 0)
                {
                    anyhow::bail!(
                        "contributes.window '{}': default_size width/height must be > 0",
                        w.id
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_contributed_settings_pages(&self) -> anyhow::Result<()> {
        // [[contributes.settings_pages]] 검증.
        if !self.contributes.settings_pages.is_empty() {
            if !self.permissions.iter().any(|p| p == "ui.settings_page") {
                anyhow::bail!(
                    "contributes.settings_pages requires the 'ui.settings_page' permission"
                );
            }
            let mut seen_page_ids = HashSet::new();
            for page in &self.contributes.settings_pages {
                if !is_valid_settings_id(&page.id) {
                    anyhow::bail!(
                        "invalid contributes.settings_pages id '{}': must be lowercase ascii + digits + '_' + '-', length 1..=64",
                        page.id
                    );
                }
                if !seen_page_ids.insert(page.id.clone()) {
                    anyhow::bail!(
                        "contributes.settings_pages id '{}' declared twice in this manifest",
                        page.id
                    );
                }
                if page.title_key.is_empty() {
                    anyhow::bail!(
                        "contributes.settings_pages '{}': title_key must not be empty",
                        page.id
                    );
                }
                Self::validate_settings_page_items(page)?;
            }
        }
        Ok(())
    }

    fn validate_settings_page_items(page: &SettingsPageContribute) -> anyhow::Result<()> {
        let mut seen_item_ids = HashSet::new();
        for item in &page.items {
            // 공통 형식 검사 (모든 variant): id·storage_key 는 settings id 규칙
            // (소문자/숫자/`_`/`-`, 1..=64), label_key 비어있지 않음, id 중복 금지.
            let (id, label_key, storage_key) = item.common();
            if !is_valid_settings_id(id) {
                anyhow::bail!(
                    "invalid contributes.settings_pages '{}' item id '{}': must be lowercase ascii + digits + '_' + '-', length 1..=64",
                    page.id,
                    id
                );
            }
            if !seen_item_ids.insert(id.to_string()) {
                anyhow::bail!(
                    "contributes.settings_pages '{}' item id '{}' declared twice",
                    page.id,
                    id
                );
            }
            if label_key.is_empty() {
                anyhow::bail!(
                    "contributes.settings_pages '{}' item '{}': label_key must not be empty",
                    page.id,
                    id
                );
            }
            if !is_valid_settings_id(storage_key) {
                anyhow::bail!(
                    "invalid contributes.settings_pages '{}' item '{}' storage_key '{}': must be lowercase ascii + digits + '_' + '-', length 1..=64",
                    page.id,
                    id,
                    storage_key
                );
            }
            Self::validate_settings_item_variant(page, id, item)?;
        }
        Ok(())
    }

    fn validate_settings_item_variant(
        page: &SettingsPageContribute,
        id: &str,
        item: &SettingsItemDecl,
    ) -> anyhow::Result<()> {
        // variant 별 추가 검사.
        match item {
            SettingsItemDecl::FontOverride { .. } | SettingsItemDecl::Toggle { .. } => {}
            SettingsItemDecl::Select {
                options, default, ..
            } => {
                // default 는 options.value 중 하나여야 한다.
                if !options.iter().any(|o| &o.value == default) {
                    anyhow::bail!(
                        "contributes.settings_pages '{}' item '{}': select default '{}' is not among options",
                        page.id,
                        id,
                        default
                    );
                }
            }
            SettingsItemDecl::Number {
                default, min, max, ..
            } => {
                // min/max 둘 다 주어지면 min ≤ max, default 는 [min,max] 안.
                if let (Some(mn), Some(mx)) = (min, max)
                    && mn > mx
                {
                    anyhow::bail!(
                        "contributes.settings_pages '{}' item '{}': number min ({}) > max ({})",
                        page.id,
                        id,
                        mn,
                        mx
                    );
                }
                if let Some(mn) = min
                    && default < mn
                {
                    anyhow::bail!(
                        "contributes.settings_pages '{}' item '{}': number default ({}) < min ({})",
                        page.id,
                        id,
                        default,
                        mn
                    );
                }
                if let Some(mx) = max
                    && default > mx
                {
                    anyhow::bail!(
                        "contributes.settings_pages '{}' item '{}': number default ({}) > max ({})",
                        page.id,
                        id,
                        default,
                        mx
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_contributed_detectors(&self) -> anyhow::Result<()> {
        // [[contributes.detector]] 검증 — schema-agnostic 만 (host file 도메인 결합 제거).
        // concrete detector rule 검증은 본 바이너리
        // `plugin_bridge::manifest_validate::validate_detector_actual` 에서 수행.
        let mut seen_detector_ids = HashSet::new();
        for v in &self.contributes.detector {
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
            if id.is_empty() {
                anyhow::bail!("contributes.detector entry missing required 'id' string field");
            }
            if !is_valid_simple_id(id) {
                anyhow::bail!(
                    "invalid contributes.detector id '{id}': must be lowercase ascii + digits + '-', length ≤ 64"
                );
            }
            if id.starts_with('$') {
                anyhow::bail!(
                    "contributes.detector '{id}': plugin cannot define reserved ($-prefixed) detector ids"
                );
            }
            if !seen_detector_ids.insert(id.to_string()) {
                anyhow::bail!("contributes.detector id '{id}' declared twice in this manifest");
            }
        }
        Ok(())
    }

    fn validate_contributed_handlers(&self) -> anyhow::Result<()> {
        // [[contributes.handler]] 검증 — schema-agnostic 만. 본문 (action/detector ref)
        // 은 본 바이너리 측에서 install 시점에 reject (file::handler::install_plugin_handlers).
        let mut seen_handler_ids = HashSet::new();
        for v in &self.contributes.handler {
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
            if id.is_empty() {
                anyhow::bail!("contributes.handler entry missing required 'id' string field");
            }
            if !seen_handler_ids.insert(id.to_string()) {
                anyhow::bail!("contributes.handler id '{id}' declared twice in this manifest");
            }
            // 권한 매칭: file_handler.handle:<detector>
            let detector = v.get("detector").and_then(|x| x.as_str()).unwrap_or("");
            if detector.is_empty() {
                anyhow::bail!(
                    "contributes.handler '{id}' missing required 'detector' string field"
                );
            }
            let needs = format!("file_handler.handle:{detector}");
            if !self.permissions.iter().any(|p| p == &needs) {
                anyhow::bail!(
                    "contributes.handler '{id}' on detector '{detector}' requires permission '{needs}'"
                );
            }
        }
        Ok(())
    }

    fn validate_contributed_detector_permissions(&self) -> anyhow::Result<()> {
        // detector contribute 권한: 신규 정의면 define, 기존 id 재선언이면 extend.
        // host/다른 plugin 의 detector 목록은 manifest 만으로는 모른다. install 시점에
        // 더 엄격하게 확인하되, manifest 차원에서는 최소한 둘 중 하나는 가져야 한다고
        // 강제한다 — 사용자가 plugin install 시 권한 부여 UI 가 의미를 가지도록.
        for v in &self.contributes.detector {
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
            let has_define = self.permissions.iter().any(|p| p == "file_handler.define");
            let needs_extend = format!("file_handler.extend:{id}");
            let has_extend = self.permissions.iter().any(|p| p == &needs_extend);
            if !has_define && !has_extend {
                anyhow::bail!(
                    "contributes.detector '{id}' requires either 'file_handler.define' (new id) \
                     or '{needs_extend}' (extending existing id)"
                );
            }
        }

        Ok(())
    }

    fn validate_contributed_hook_handlers(&self) -> anyhow::Result<()> {
        // [[contributes.hook_handler]] 검증 — schema-agnostic 만. concrete action
        // (IpcSequence calls 등) 은 host 측 `HookHandlerRegistryPort::install_plugin_hook_handlers`
        // 가 install 시점에 deserialize·검증한다. plugin 은 셸(`ShellCommand`) 을
        // 타입 레벨에서 쓸 수 없으므로(PluginHookHandlerActionDecl) 여기선 id 형식 +
        // 유일성 + define 권한만 확인한다.
        let has_define = self.permissions.iter().any(|p| p == "hook_handler.define");
        let mut seen_ids = HashSet::new();
        for v in &self.contributes.hook_handler {
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
            if id.is_empty() {
                anyhow::bail!("contributes.hook_handler entry missing required 'id' string field");
            }
            if !is_valid_hook_handler_id(id) {
                anyhow::bail!(
                    "invalid contributes.hook_handler id '{id}': must be lowercase ascii + digits + '-', length ≤ 32"
                );
            }
            if !seen_ids.insert(id.to_string()) {
                anyhow::bail!("contributes.hook_handler id '{id}' declared twice in this manifest");
            }
            // 훅 핸들러 정의는 base 권한 `hook_handler.define` 을 요구한다(파일 핸들러
            // detector 가 `file_handler.define` 을 요구하는 것과 동일 지위). 사용자가
            // plugin install 시 이 권한을 grant 해야 훅 핸들러가 설치된다.
            if !has_define {
                anyhow::bail!(
                    "contributes.hook_handler '{id}' requires permission 'hook_handler.define'"
                );
            }
        }
        Ok(())
    }

    /// 매니페스트에 선언된 권한을 파싱한 set으로 반환.
    /// `validate()`가 통과한 매니페스트에 대해 호출되면 절대 실패하지 않는다.
    pub fn parsed_permissions(&self) -> anyhow::Result<HashSet<Permission>> {
        let mut out = HashSet::with_capacity(self.permissions.len());
        for raw in &self.permissions {
            match Permission::from_token(raw) {
                Some(p) => {
                    out.insert(p);
                }
                None => anyhow::bail!("unknown permission '{}'", raw),
            }
        }
        Ok(out)
    }
}

fn validate_hook_timeout(timeout_ms: u32, target: &str) -> anyhow::Result<()> {
    if timeout_ms == 0 {
        anyhow::bail!("extends hook for '{target}': timeout_ms must be > 0");
    }
    if timeout_ms > HOOK_TIMEOUT_MS_MAX {
        anyhow::bail!(
            "extends hook for '{target}': timeout_ms {timeout_ms} exceeds maximum {HOOK_TIMEOUT_MS_MAX}"
        );
    }
    Ok(())
}

fn validate_hook_mode_modifies(
    mode: HookMode,
    modifies: &[String],
    target: &str,
) -> anyhow::Result<()> {
    match mode {
        HookMode::Transform => {
            if modifies.is_empty() {
                anyhow::bail!(
                    "extends hook for '{target}': mode=transform requires non-empty 'modifies'"
                );
            }
        }
        HookMode::Filter | HookMode::Observe => {
            // filter는 bool 반환만 하므로 modifies 무시. observe도 변경 권한 없음.
            // 매니페스트에 적혀 있어도 거부하지는 않지만 silently 무시되지 않게 경고 로깅.
            if !modifies.is_empty() {
                tracing::warn!(
                    "extends hook for '{target}': mode={:?} ignores 'modifies' field",
                    mode
                );
            }
        }
    }
    Ok(())
}
