//! 카탈로그 모델 — 디자인(4) gallery 의 **page > section > spec** 문서 계층.
//!
//! 한 페이지(`Page`)는 1차 분류(`Category`) 하나에 대응하고, 여러 `Section` 을
//! 가진다. 각 `Section` 은 여러 `Spec` 을 묶는다. `Spec::draw` 는 선택된 한
//! specimen 의 라이브 데모(stage/cluster/meta)를 그린다.
//!
//! 셸(`host_shell`)은 활성 페이지의 전 Section/Spec 을 한 문서로 스크롤 렌더하고,
//! 좌측 nav 의 "On this page" 앵커는 이 Section 목록에서 도출한다.
//!
//! 이 단계(인프라)는 모델/셸/헬퍼의 토대만 만든다. 각 `Spec::draw` 는 기존
//! specimen 의 `draw` 함수를 그대로 연결해 컴파일/실행을 유지하며, 페이지별
//! specimen 콘텐츠의 디자인 정합 재작성은 Round 2 의 책임이다.

pub mod components;
pub mod foundations_shape;
pub mod foundations_uiscale;
pub mod icons;
pub mod popup_frame;
pub mod spacing;
pub mod spec;
pub mod specimen;
pub mod theme;
pub mod toast_card;
pub mod typography;
pub mod widgets;

use tasty_type_appearance::theme::Theme;

/// 카탈로그 1차 분류 = 문서 페이지 하나. 상단 crumb + 좌측 nav Catalog 그룹에 사용.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// 토큰·기초 (색/타입/간격/형태/스케일).
    Foundations,
    /// 위젯·컴포넌트 (단일 UI primitive).
    Components,
    /// canonical 글리프 세트.
    Icons,
    /// 모달·팝업 레이어.
    Overlays,
    /// 구조 셸 (사이드바/탭바/분할/하이라이트).
    Layouts,
}

impl Category {
    /// 페이지 라벨 (crumb / nav 링크 lbl).
    pub fn label(self) -> &'static str {
        match self {
            Category::Foundations => "Foundations",
            Category::Components => "Components",
            Category::Icons => "Icons",
            Category::Overlays => "Overlays",
            Category::Layouts => "Layouts",
        }
    }

    /// nav 링크 우측 desc (research §1.2 의 5 페이지 메타).
    pub fn desc(self) -> &'static str {
        match self {
            Category::Foundations => "tokens",
            Category::Components => "primitives",
            Category::Icons => "glyphs",
            Category::Overlays => "modals",
            Category::Layouts => "shells",
        }
    }

    /// 페이지 헤더 intro 산문 (pagehead `p`).
    pub fn intro(self) -> &'static str {
        match self {
            Category::Foundations => {
                "The token layer — surface ramp, text hierarchy, accent roles, type, \
                 the 4px spacing grid, shape and motion. Everything else is built from these."
            }
            Category::Components => {
                "Single-purpose primitives — buttons, chips, form controls, navigation rows, \
                 status feedback. Each maps to one Theme-driven widget."
            }
            Category::Icons => {
                "The canonical glyph set — 24×24, 2px stroke, round caps, no fill, currentColor. \
                 One family across every surface."
            }
            Category::Overlays => {
                "Modal and popup layers — palettes, dialogs, pickers, agent approval. \
                 Scrim, frame, and lift composed from semantic tokens."
            }
            Category::Layouts => {
                "Structural shells — sidebars, tab strips, list→detail, dividers, surface focus. \
                 How panes and workspaces are framed."
            }
        }
    }

    /// pagehead 의 HowTo 3컬럼 배너 노출 여부 (디자인은 Foundations 만 `howto:true`).
    pub fn howto(self) -> bool {
        matches!(self, Category::Foundations)
    }

    pub fn all() -> &'static [Category] {
        &[
            Category::Foundations,
            Category::Components,
            Category::Icons,
            Category::Overlays,
            Category::Layouts,
        ]
    }
}

/// 카탈로그 한 항목 — 한 specimen 의 헤딩 메타 + 라이브 데모 draw.
pub struct Spec {
    /// 앵커/스크롤 식별자.
    pub id: &'static str,
    /// 항목 제목 (h3).
    pub title: &'static str,
    /// "언제 쓰나" 한 줄 설명 (선택).
    pub when: Option<&'static str>,
    /// 라이브 데모 draw — `Theme` 만 받아 egui 위젯을 그린다.
    pub draw: fn(&mut egui::Ui, &Theme),
}

/// 페이지 내 한 구역 — nav "On this page" 앵커의 단위.
pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub specs: Vec<Spec>,
}

/// 문서 페이지 하나 = 한 `Category`.
pub struct Page {
    pub category: Category,
    pub sections: Vec<Section>,
}

fn spec(
    id: &'static str,
    title: &'static str,
    when: Option<&'static str>,
    draw: fn(&mut egui::Ui, &Theme),
) -> Spec {
    Spec {
        id,
        title,
        when,
        draw,
    }
}

fn section(id: &'static str, title: &'static str, specs: Vec<Spec>) -> Section {
    Section { id, title, specs }
}

/// 모든 페이지의 Section/Spec 트리.
///
/// 기존 specimen `draw` 들을 디자인 분류(research §3.1)에 따라 page/section 으로
/// 임시 매핑한다 — 34 개 기존 draw 전수 연결. 콘텐츠 재작성은 Round 2.
pub fn pages() -> Vec<Page> {
    vec![
        // ── Foundations ──────────────────────────────────────────────
        Page {
            category: Category::Foundations,
            sections: vec![
                section(
                    "elevation",
                    "Color — elevation (surface ramp)",
                    vec![spec(
                        "elevation",
                        "Depth reads through surface tint, never shadow",
                        Some("bg-app → sidebar → panel → surface-raised, one tint step apart"),
                        theme::elevation,
                    )],
                ),
                section(
                    "text",
                    "Color — text",
                    vec![spec(
                        "text",
                        "Hierarchy by text color, on any surface",
                        Some("primary → secondary → muted → disabled → placeholder"),
                        theme::text,
                    )],
                ),
                section(
                    "accents",
                    "Color — accent roles",
                    vec![spec(
                        "accents",
                        "Accents map to roles, not decoration",
                        Some("primary · info · success · warning · danger · agent"),
                        theme::accents,
                    )],
                ),
                section(
                    "type",
                    "Type",
                    vec![spec(
                        "type",
                        "Two families, hard 14px cap, hierarchy by weight",
                        Some("heading 13/600 · body 13 · caption 11 · mono 14"),
                        typography::draw,
                    )],
                ),
                section(
                    "spacing",
                    "Spacing — the 4px grid, in use",
                    vec![spec(
                        "spacing",
                        "Five steps, each with a job",
                        Some("xs chip · sm pair · md card · lg column · xl region"),
                        spacing::draw,
                    )],
                ),
                section(
                    "shape",
                    "Radius · border · motion",
                    vec![spec(
                        "shape",
                        "Crisp and rectilinear — it's a terminal",
                        Some("radius 4/2 · 1px border · UI 90–120ms · terminal 0ms"),
                        foundations_shape::draw,
                    )],
                ),
                section(
                    "uiscale",
                    "UI scale — sidebar zoom",
                    vec![spec(
                        "uiscale",
                        "One multiplier scales the sidebar; everything else stays fixed",
                        Some("stops 0.8 / 1.0 / 1.2 — sidebar root zoom only"),
                        foundations_uiscale::draw,
                    )],
                ),
            ],
        },
        // ── Components ───────────────────────────────────────────────
        Page {
            category: Category::Components,
            sections: vec![
                section(
                    "buttons",
                    "Buttons",
                    vec![
                        spec("button", "Button", None, components::prim_button::draw),
                        spec(
                            "icon-button",
                            "IconButton",
                            None,
                            components::prim_icon_button::draw,
                        ),
                    ],
                ),
                section(
                    "chips",
                    "Badge · Tag · Kbd",
                    vec![spec(
                        "chips",
                        "Badge · Tag · Kbd",
                        None,
                        components::prim_chips::draw,
                    )],
                ),
                section(
                    "forms",
                    "Form controls",
                    vec![
                        spec("input", "Input", None, components::prim_input::draw),
                        spec(
                            "forms",
                            "Select · Checkbox · Switch",
                            None,
                            components::prim_forms::draw,
                        ),
                    ],
                ),
                section(
                    "nav",
                    "MenuItem · TreeRow",
                    vec![spec(
                        "nav",
                        "MenuItem · TreeRow",
                        None,
                        components::prim_nav::draw,
                    )],
                ),
                section(
                    "feedback",
                    "StatusDot · Spinner · Toast",
                    vec![
                        spec(
                            "status-dot",
                            "StatusDot",
                            None,
                            components::prim_status_dot::draw,
                        ),
                        spec("spinner", "Spinner", None, components::prim_spinner::draw),
                        spec("toast", "Toast", None, widgets::toast::draw),
                    ],
                ),
                section(
                    "text",
                    "Hint text",
                    vec![spec("hint", "Hint text", None, widgets::hint_text::draw)],
                ),
            ],
        },
        // ── Icons ────────────────────────────────────────────────────
        Page {
            category: Category::Icons,
            sections: vec![section(
                "glyphs",
                "Icon set",
                vec![spec(
                    "glyphs",
                    "Canonical glyphs",
                    Some("24×24, 2px stroke, round, no fill, currentColor"),
                    icons::draw,
                )],
            )],
        },
        // ── Overlays ─────────────────────────────────────────────────
        Page {
            category: Category::Overlays,
            sections: vec![
                section(
                    "command",
                    "Command & menus",
                    vec![
                        spec(
                            "palette",
                            "Command palette",
                            None,
                            components::command_palette::draw,
                        ),
                        spec("tools", "Tools menu", None, components::tools_menu::draw),
                    ],
                ),
                section(
                    "dialogs",
                    "Dialogs & pickers",
                    vec![
                        spec("dialog", "Dialog frame", None, widgets::dialog::draw),
                        spec(
                            "rename",
                            "Rename popup",
                            None,
                            components::rename_popup::draw,
                        ),
                        spec("convert", "Convert surface", None, components::convert::draw),
                        spec(
                            "markdown",
                            "Markdown open",
                            None,
                            components::markdown_open::draw,
                        ),
                        spec(
                            "preset",
                            "Apply preset",
                            None,
                            components::apply_preset::draw,
                        ),
                        spec(
                            "filehandler",
                            "File handler picker",
                            None,
                            components::file_handler_picker::draw,
                        ),
                        spec("update", "Update (Tier 3)", None, components::update::draw),
                    ],
                ),
                section(
                    "agent",
                    "Agent approval",
                    vec![spec(
                        "approval",
                        "Agent approval",
                        None,
                        components::approval::draw,
                    )],
                ),
                section(
                    "ports",
                    "Ports & search",
                    vec![
                        spec(
                            "ports",
                            "Listening ports",
                            None,
                            components::port_scanner::draw,
                        ),
                        spec("search", "Search bar", None, components::search_bar::draw),
                    ],
                ),
                section(
                    "toast-stack",
                    "Toast stack",
                    vec![spec(
                        "toast-stack",
                        "Toast stack (Tier 3)",
                        None,
                        components::toast::draw,
                    )],
                ),
            ],
        },
        // ── Layouts ──────────────────────────────────────────────────
        Page {
            category: Category::Layouts,
            sections: vec![
                section(
                    "sidebar",
                    "Sidebar & rail",
                    vec![spec(
                        "sidebar",
                        "Sidebar (Full / Collapsed)",
                        None,
                        components::sidebar::draw,
                    )],
                ),
                section(
                    "tabs",
                    "Tab strips",
                    vec![
                        spec("tabbar", "Pane tab strip", None, components::tab_bar::draw),
                        spec(
                            "multitab",
                            "Multi-tier tabs",
                            None,
                            widgets::multi_tab_layout::draw,
                        ),
                    ],
                ),
                section(
                    "depth",
                    "List → detail",
                    vec![
                        spec(
                            "onedepth",
                            "1-depth (Plugins idiom)",
                            None,
                            widgets::layout_1depth::draw,
                        ),
                        spec(
                            "twodepth",
                            "2-depth (Settings idiom)",
                            None,
                            widgets::layout_2depth::draw,
                        ),
                    ],
                ),
                section(
                    "surfaces",
                    "Dividers & surfaces",
                    vec![
                        spec("divider", "Pane divider", None, widgets::divider::draw),
                        spec(
                            "surface",
                            "Surface focus states",
                            None,
                            components::surface_highlights::draw,
                        ),
                    ],
                ),
            ],
        },
    ]
}
