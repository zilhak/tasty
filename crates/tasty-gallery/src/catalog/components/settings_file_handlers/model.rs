//! 시안 kit의 시드와 상태 전이를 옮긴 갤러리 전용 모델.

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Rule {
    pub(super) src: String,
    pub(super) parts: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetPending {
    Add,
    Remove,
    Unpatch,
    Patch,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Det {
    pub(super) id: String,
    pub(super) rules: Vec<Rule>,
    pub(super) on: bool,
    pub(super) base_on: Option<bool>,
    pub(super) user_meta: bool,
    pub(super) pending: Option<DetPending>,
    pub(super) prev: Option<Box<Det>>,
}

impl Det {
    pub(super) fn new(id: &str, rules: &[(&str, &str)], on: bool) -> Self {
        Self {
            id: id.into(),
            rules: rules
                .iter()
                .map(|(src, text)| Rule {
                    src: (*src).into(),
                    parts: vec![(*text).into()],
                })
                .collect(),
            on,
            base_on: None,
            user_meta: false,
            pending: None,
            prev: None,
        }
    }
    pub(super) fn others(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in &self.rules {
            if r.src != "user" && !out.contains(&r.src) {
                out.push(r.src.clone());
            }
        }
        out
    }
    pub(super) fn user_rules(&self) -> bool {
        self.rules.iter().any(|r| r.src == "user")
    }
    pub(super) fn has_user(&self) -> bool {
        self.user_rules() || self.user_meta
    }
    pub(super) fn strip(&self) -> Self {
        Self {
            pending: None,
            prev: None,
            ..self.clone()
        }
    }
}

pub(super) fn det_seed() -> Vec<Det> {
    let mut notebook = Det::new("notebook", &[("plugin:md-plus", "ext ipynb")], false);
    notebook.base_on = Some(true);
    notebook.user_meta = true;
    vec![
        Det::new(
            "markdown",
            &[("host", "ext md, markdown"), ("user", "ext mdx")],
            true,
        ),
        Det::new(
            "image",
            &[(
                "host",
                "ext png, jpg, jpeg, gif, webp, svg, bmp, ico, tiff, avif",
            )],
            true,
        ),
        Det::new(
            "log",
            &[
                ("host", "ext log"),
                ("plugin:logkit", "path *.log.[0-9]*"),
                ("user", "path /var/log/**/*.out"),
            ],
            true,
        ),
        Det::new("$directory", &[("host", "any directory")], true),
        notebook,
        Det::new("dockerfile", &[("user", "path Dockerfile")], true),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HdlPending {
    Add,
    Remove,
    Replace,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Hdl {
    pub(super) id: String,
    pub(super) prio: i32,
    pub(super) origin: String,
    pub(super) det: String,
    pub(super) act: String,
    pub(super) on: bool,
    pub(super) pending: Option<HdlPending>,
    pub(super) prev: Option<Box<Hdl>>,
}

impl Hdl {
    pub(super) fn new(id: &str, prio: i32, origin: &str, det: &str, act: &str, on: bool) -> Self {
        Self {
            id: id.into(),
            prio,
            origin: origin.into(),
            det: det.into(),
            act: act.into(),
            on,
            pending: None,
            prev: None,
        }
    }
}

pub(super) fn hdl_seed() -> Vec<Hdl> {
    vec![
        Hdl::new(
            "host/image-view",
            10,
            "host",
            "image",
            "surface:image_view",
            true,
        ),
        Hdl::new(
            "host/markdown-view",
            10,
            "host",
            "markdown",
            "surface:markdown_view",
            true,
        ),
        Hdl::new("host/log-view", 20, "host", "log", "surface:log_view", true),
        Hdl::new(
            "md-plus/notebook-open",
            50,
            "plugin:md-plus",
            "notebook",
            "ipc:com.mdplus.notebook.open_in_preview_surface",
            false,
        ),
        Hdl::new(
            "user/dockerfile-system",
            100,
            "user",
            "dockerfile",
            "system",
            true,
        ),
    ]
}

/// 시안 `TIE_HDL` — `markdown` priority 10에 출처마다 handler 하나. host/markdown-view와 같은 자리다.
fn tie_hdl() -> Vec<Hdl> {
    vec![
        Hdl::new(
            "com.example.md/preview",
            10,
            "plugin:com.example.md",
            "markdown",
            "surface:md_preview",
            true,
        ),
        Hdl::new("user/my-viewer", 10, "user", "markdown", "system", true),
    ]
}

/// 시안 `OWNER_RANK` — 같은 priority에서 user > plugin > host.
fn owner_rank(origin: &str) -> u8 {
    if origin == "user" {
        0
    } else if origin.starts_with("plugin:") {
        1
    } else {
        2
    }
}

/// 시안 `sortHdl` — 목록 순서가 실행 순서다. priority ↑ → owner user > plugin > host → id.
pub(super) fn run_order(xs: &[Hdl]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..xs.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&xs[a], &xs[b]);
        (x.prio, owner_rank(&x.origin), &x.id).cmp(&(y.prio, owner_rank(&y.origin), &y.id))
    });
    order
}

/// 시안 Pane의 초기 상태 prop.
#[derive(Clone, Copy, Default)]
pub(super) struct Seed {
    pub(super) de: bool,
    /// 같은 detector·priority에 출처별 handler를 더한다(시안 `tie`).
    pub(super) tie: bool,
    /// 좁은 열의 대기 행 셋 — markdown·notebook 사용자 기여 삭제, log 사용자 규칙 추가(시안 `narrowSet`).
    pub(super) narrow_set: bool,
    pub(super) narrow: bool,
    pub(super) empty: bool,
    pub(super) pending: bool,
    pub(super) unpatch: bool,
    pub(super) patched: bool,
    pub(super) adding: bool,
    pub(super) attempted: bool,
    pub(super) draft: Draft,
}

/// 카드 초안. 시안 `draft` prop과 같은 키다. kind 0 surface · 1 ipc · 2 system.
#[derive(Clone, Copy, Default)]
pub(super) struct Draft {
    pub(super) id: &'static str,
    pub(super) ext: &'static str,
    pub(super) glob: &'static str,
    pub(super) short: &'static str,
    pub(super) det: &'static str,
    pub(super) prio: &'static str,
    pub(super) kind: usize,
    pub(super) surface: &'static str,
    pub(super) method: &'static str,
}

#[derive(Default)]
pub(super) struct DetState {
    pub(super) xs: Vec<Det>,
    pub(super) adding: bool,
    pub(super) tried: bool,
    pub(super) id: String,
    pub(super) ext: String,
    pub(super) glob: String,
}

#[derive(Default)]
pub(super) struct HdlState {
    pub(super) xs: Vec<Hdl>,
    pub(super) adding: bool,
    pub(super) tried: bool,
    pub(super) short: String,
    pub(super) det: Option<usize>,
    pub(super) prio: String,
    pub(super) kind: usize,
    pub(super) surface: String,
    pub(super) param: String,
    pub(super) method: String,
}

pub(super) fn det_state(seed: &Seed) -> DetState {
    let mut xs = if seed.empty { Vec::new() } else { det_seed() };
    if seed.pending {
        for x in &mut xs {
            if x.has_user() && x.others().is_empty() {
                x.pending = Some(DetPending::Remove);
            }
        }
        xs.push(Det {
            pending: Some(DetPending::Add),
            ..Det::new("notes", &[("user", "ext note, notes")], true)
        });
    }
    if seed.unpatch {
        for x in &mut xs {
            if x.has_user() && !x.others().is_empty() {
                x.pending = Some(DetPending::Unpatch);
            }
        }
    }
    if seed.narrow_set {
        for x in &mut xs {
            match x.id.as_str() {
                "markdown" | "notebook" => x.pending = Some(DetPending::Unpatch),
                "log" => {
                    let mut prev = x.clone();
                    prev.rules.retain(|r| r.src != "user");
                    x.pending = Some(DetPending::Patch);
                    x.prev = Some(Box::new(prev));
                }
                _ => {}
            }
        }
    }
    if seed.patched {
        for x in &mut xs {
            let rule = match x.id.as_str() {
                "image" => Some("ext heic"),
                "markdown" => Some("ext mdx, mdown"),
                _ => None,
            };
            if let Some(r) = rule {
                let prev = x.clone();
                x.rules.retain(|r| r.src != "user");
                x.rules.push(Rule {
                    src: "user".into(),
                    parts: vec![r.into()],
                });
                x.pending = Some(DetPending::Patch);
                x.prev = Some(Box::new(prev));
            }
        }
    }
    DetState {
        xs,
        adding: seed.adding,
        tried: seed.attempted,
        id: seed.draft.id.into(),
        ext: seed.draft.ext.into(),
        glob: seed.draft.glob.into(),
    }
}

pub(super) fn hdl_state(seed: &Seed) -> HdlState {
    let mut xs = if seed.empty { Vec::new() } else { hdl_seed() };
    if seed.tie && !seed.empty {
        xs.extend(tie_hdl());
    }
    if seed.pending {
        for x in &mut xs {
            if x.origin == "user" {
                x.pending = Some(HdlPending::Remove);
            }
        }
        xs.push(Hdl {
            pending: Some(HdlPending::Add),
            ..Hdl::new(
                "user/notes-view",
                30,
                "user",
                "notes",
                "surface:markdown_view",
                true,
            )
        });
    }
    let dets: Vec<String> = det_seed().into_iter().map(|d| d.id).collect();
    HdlState {
        xs,
        adding: seed.adding,
        tried: seed.attempted,
        short: seed.draft.short.into(),
        det: dets.iter().position(|d| d == seed.draft.det),
        prio: seed.draft.prio.into(),
        kind: seed.draft.kind,
        surface: seed.draft.surface.into(),
        param: String::new(),
        method: seed.draft.method.into(),
    }
}

pub(super) fn valid_chars(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub(super) fn dotted(m: &str) -> bool {
    let parts: Vec<&str> = m.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}
