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
    /// 플러그인 유래 컴포넌트 (네이티브와 분리된 플러그인 전용 섹션).
    Plugins,
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
            Category::Plugins => "Plugins",
        }
    }

    /// nav 링크 우측 desc (research §1.2 의 페이지 메타).
    pub fn desc(self) -> &'static str {
        match self {
            Category::Foundations => "tokens",
            Category::Components => "primitives",
            Category::Icons => "glyphs",
            Category::Overlays => "modals",
            Category::Layouts => "shells",
            Category::Plugins => "plugins",
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
            Category::Plugins => {
                "UI contributed by plugins, kept apart from the native surfaces — clipboard and \
                 git viewers, plus the markdown / image / html content surfaces. Each ships with \
                 its plugin and is themed by the host."
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
            Category::Plugins,
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
                    "terminal",
                    "Color — terminal / ANSI palette",
                    vec![spec(
                        "terminal",
                        "The colors a terminal cell paints with — not UI chrome",
                        Some("ANSI 16 (SGR 30–37 / 90–97) + selection · vi cursor · search fills"),
                        theme::terminal,
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
                        spec(
                            "button",
                            "Button",
                            Some(
                                "Primary action and its variants — primary, secondary, ghost, danger, agent",
                            ),
                            components::prim_button::draw,
                        ),
                        spec(
                            "icon-button",
                            "IconButton",
                            Some("Square icon-only control for toolbars and row affordances"),
                            components::prim_icon_button::draw,
                        ),
                    ],
                ),
                section(
                    "chips",
                    "Badge · Tag · Kbd",
                    vec![
                        spec(
                            "badge",
                            "Badge",
                            Some("Count or short status pill"),
                            components::prim_chips::draw_badge,
                        ),
                        spec(
                            "tag",
                            "Tag",
                            Some("Outlined label for surface kind or state"),
                            components::prim_chips::draw_tag,
                        ),
                        spec(
                            "kbd",
                            "Kbd",
                            Some("Keyboard shortcut keycaps"),
                            components::prim_chips::draw_kbd,
                        ),
                    ],
                ),
                section(
                    "forms",
                    "Form controls",
                    vec![
                        spec(
                            "input",
                            "Input",
                            Some("Single-line text field — icon, addon, mono, invalid, block"),
                            components::prim_input::draw,
                        ),
                        spec(
                            "forms",
                            "Select · Checkbox · Switch",
                            Some("Choice and toggle controls"),
                            components::prim_forms::draw,
                        ),
                    ],
                ),
                section(
                    "plugin-settings",
                    "Plugin settings page",
                    vec![spec(
                        "plugin-settings",
                        "Plugin-contributed settings rows",
                        Some("label · control rows — toggle / select / number"),
                        components::plugin_settings::draw,
                    )],
                ),
                section(
                    "nav",
                    "Tab · TreeRow · MenuItem",
                    vec![
                        spec(
                            "tab",
                            "Tab",
                            Some("A surface tab and its status — active, idle, notification"),
                            components::prim_tab::draw,
                        ),
                        spec(
                            "tree-row",
                            "TreeRow",
                            Some("Disclosure row in the sidebar tree"),
                            components::prim_nav::draw_tree_row,
                        ),
                        spec(
                            "menu-item",
                            "MenuItem",
                            Some("Row in a context or command menu"),
                            components::prim_nav::draw_menu_item,
                        ),
                    ],
                ),
                section(
                    "feedback",
                    "StatusDot · Status resolution · Spinner · Toast · Toast stack",
                    vec![
                        spec(
                            "status-dot",
                            "StatusDot",
                            Some("One dot for a surface's live state"),
                            components::prim_status_dot::draw,
                        ),
                        spec(
                            "status-resolution",
                            "Status resolution",
                            Some("How owner and activity collapse to a single dot"),
                            components::prim_status_resolution::draw,
                        ),
                        spec(
                            "spinner",
                            "Spinner",
                            Some("Indeterminate progress — sizes and reduced-motion fallback"),
                            components::prim_spinner::draw,
                        ),
                        spec(
                            "toast",
                            "Toast",
                            Some("Transient notification card"),
                            widgets::toast::draw,
                        ),
                        spec(
                            "toast-stack",
                            "Toast stack",
                            Some("Bottom-right stack with newest-on-top and +N more overflow"),
                            widgets::toast::draw_stack,
                        ),
                    ],
                ),
                section(
                    "text",
                    "Hint text",
                    vec![spec(
                        "hint",
                        "Hint text",
                        Some("Helper text below a field"),
                        widgets::hint_text::draw,
                    )],
                ),
                section(
                    "data",
                    "Table",
                    vec![spec(
                        "table",
                        "Table",
                        Some("Sticky-header data grid — the shared Table widget"),
                        components::prim_table::draw,
                    )],
                ),
                section(
                    "segmented",
                    "Segmented control",
                    vec![spec(
                        "segmented",
                        "Segmented",
                        Some("Mutually-exclusive toggle — explorer's grid/list/detail switch"),
                        components::segmented::draw,
                    )],
                ),
                section(
                    "explorer-cells",
                    "Explorer view cells",
                    vec![spec(
                        "explorer-cells",
                        "Explorer view cells",
                        Some(
                            "grid cell (new) · list row (tree_row) · detail (Table + sort header)",
                        ),
                        components::explorer_view_cells::draw,
                    )],
                ),
            ],
        },
        // ── Icons ────────────────────────────────────────────────────
        // 디자인(4) §2.3 — system-rules Section 1개 + 6 job 그룹 Section.
        // 모든 글리프 24×24, 2px stroke, round, no fill, currentColor.
        Page {
            category: Category::Icons,
            sections: vec![
                section(
                    "system-rules",
                    "The icon system",
                    vec![spec(
                        "system-rules",
                        "One geometry, recolored by context",
                        Some(
                            "24×24 viewBox · 2px stroke round · no fill · currentColor — sized via prop (26/20/16/14/12)",
                        ),
                        icons::draw_system_rules,
                    )],
                ),
                section(
                    "actions",
                    "Actions",
                    vec![spec(
                        "actions",
                        "The verbs",
                        Some(
                            "What an IconButton wraps in a toolbar or row — close/refresh in every overlay header, edit/trash/copy in list rows",
                        ),
                        icons::draw_actions,
                    )],
                ),
                section(
                    "nav",
                    "Navigation & disclosure",
                    vec![spec(
                        "nav",
                        "Movement and open/closed state",
                        Some(
                            "Single chevrons = tree-row disclosure; doubled = collapse/expand the sidebar rail",
                        ),
                        icons::draw_nav,
                    )],
                ),
                section(
                    "surfaces",
                    "Surfaces & workspace",
                    vec![spec(
                        "surfaces",
                        "The nouns of the workspace",
                        Some(
                            "What a tab, tree row, or new-surface button shows — terminal/markdown are the two core surface kinds",
                        ),
                        icons::draw_surfaces,
                    )],
                ),
                section(
                    "view",
                    "View modes & favorites",
                    vec![spec(
                        "view",
                        "Explorer view switch & bookmark marker",
                        Some(
                            "grid / list / detail toggle glyphs + star — the explorer toolbar and favorites sidebar",
                        ),
                        icons::draw_view,
                    )],
                ),
                section(
                    "visibility",
                    "Visibility",
                    vec![spec(
                        "visibility",
                        "Reveal toggle on secret values",
                        Some(
                            "Passkeys, env — eye when hidden, eyeOff when shown; swap in place on the same IconButton",
                        ),
                        icons::draw_visibility,
                    )],
                ),
                section(
                    "status",
                    "Status & alerts",
                    vec![spec(
                        "status",
                        "Inline meaning markers",
                        Some(
                            "Tinted by the line they sit in (warning amber, success green, danger red) via currentColor — not state dots",
                        ),
                        icons::draw_status,
                    )],
                ),
                section(
                    "system",
                    "Tools & system",
                    vec![spec(
                        "system",
                        "Sidebar footer & global tools",
                        Some("Each anchors a menu or window — tools, settings, plug, rocket"),
                        icons::draw_system,
                    )],
                ),
            ],
        },
        // ── Overlays ─────────────────────────────────────────────────
        // 디자인(4) §2.4 의 14 Spec 을 1:1 Section 으로 — 모든 모달이 공유하는
        // scrim/frame 레시피(scrim)부터 2-tier settings 까지.
        Page {
            category: Category::Overlays,
            sections: vec![
                section(
                    "scrim",
                    "Scrim & frame",
                    vec![spec(
                        "scrim",
                        "Dismiss on scrim or Esc",
                        Some(
                            "The shared recipe — bg-panel frame, 1px border-strong, modal shadow, scrim + blur",
                        ),
                        widgets::dialog::draw,
                    )],
                ),
                section(
                    "banner",
                    "Banner — the floating top notice",
                    vec![
                        spec(
                            "banner-shell",
                            "Banner shell — the 4th overlay family",
                            Some(
                                "Floats at content-top below the tab bar · 100% − 16px · 8px radius · user-action only",
                            ),
                            widgets::banner::draw,
                        ),
                        spec(
                            "banner-dismiss",
                            "Dismiss & TTL countdown",
                            Some(
                                "Top-right one slot — × hidden until hover · TTL seconds → × on hover",
                            ),
                            widgets::banner::draw_dismiss,
                        ),
                        spec(
                            "banner-stack",
                            "Queue & stacking",
                            Some(
                                "One per scope, rest queue (max 5) · lower scope dimmed to 40% behind",
                            ),
                            widgets::banner::draw_stack,
                        ),
                        spec(
                            "banner-hit-zone",
                            "Position & hit-zone",
                            Some(
                                "Card rect consumes the mouse · the surface body below passes clicks through",
                            ),
                            widgets::banner::draw_hit_zone,
                        ),
                        spec(
                            "banner-blacklist",
                            "Capture blacklist (Settings › Terminal)",
                            Some(
                                "List editor — rows (pattern + ×) + Add field · empty state is neutral",
                            ),
                            widgets::banner::draw_blacklist,
                        ),
                    ],
                ),
                section(
                    "palette",
                    "Command palette",
                    vec![spec(
                        "palette",
                        "Top-anchored, fuzzy, keyboard-first",
                        Some("480px · surface-raised · spawns under the title bar"),
                        components::command_palette::draw,
                    )],
                ),
                section(
                    "tools",
                    "Tools menu",
                    vec![spec(
                        "tools",
                        "Anchored to the sidebar, no scrim",
                        Some("160px popover · builtin actions + plugins"),
                        components::tools_menu::draw,
                    )],
                ),
                section(
                    "ports",
                    "Listening ports",
                    vec![spec(
                        "ports",
                        "Live listeners, copy address",
                        Some("660×520 · 7-column table · sticky header"),
                        components::port_scanner::draw,
                    )],
                ),
                section(
                    "remote",
                    "Remote connections",
                    vec![spec(
                        "remote",
                        "Profiles & passkeys",
                        Some("520×460 · two tabs · SSH targets, identity at the boundary"),
                        components::remote::draw,
                    )],
                ),
                section(
                    "search",
                    "Search bar",
                    vec![spec(
                        "search",
                        "Headless, sticky, top-right",
                        Some("360×28 · find bar on the focused surface, no scrim"),
                        components::search_bar::draw,
                    )],
                ),
                section(
                    "switch",
                    "Switch-number overlay",
                    vec![
                        spec(
                            "switch-tab",
                            "Tab switch — modifier held",
                            Some("Ctrl held · number keycap replaces each tab icon, in place"),
                            components::switch_overlay::draw_tab,
                        ),
                        spec(
                            "switch-ws",
                            "Workspace switch — modifier held",
                            Some("Alt held · keycap replaces status dot / letter avatar"),
                            components::switch_overlay::draw_workspace,
                        ),
                    ],
                ),
                section(
                    "approval",
                    "Agent approval",
                    vec![spec(
                        "approval",
                        "Review the command before it runs",
                        Some("440px · the command and its grants, verbatim"),
                        components::approval::draw,
                    )],
                ),
                section(
                    "convert",
                    "Convert surface",
                    vec![spec(
                        "convert",
                        "Swap a surface's type in place",
                        Some("400px · From → To, scrollback preserved"),
                        components::convert::draw,
                    )],
                ),
                section(
                    "filehandler",
                    "File handler picker",
                    vec![spec(
                        "filehandler",
                        "Pick who opens this file",
                        Some("420px · built-in + plugin handlers, Always for type"),
                        components::file_handler_picker::draw,
                    )],
                ),
                section(
                    "preset",
                    "Apply preset",
                    vec![spec(
                        "preset",
                        "Apply a saved layout",
                        Some("440px · Workspace / Tab / Pane scope"),
                        components::apply_preset::draw,
                    )],
                ),
                section(
                    "preseteditor",
                    "Preset demo-layout preview",
                    vec![spec(
                        "preseteditor",
                        "Demo-layout preview — read-only",
                        Some("Workspace / Tab / Pane · pane card + tab strip + surface hairline"),
                        components::preset_editor::draw,
                    )],
                ),
                section(
                    "markdown",
                    "Markdown open",
                    vec![
                        spec(
                            "markdown",
                            "Edit or preview",
                            Some("420px · two choice cards"),
                            components::markdown_open::draw,
                        ),
                        spec(
                            "markdown-large-file",
                            "Confirm large file",
                            Some("360px · size tag + Cancel/Open"),
                            components::md_large_file::draw,
                        ),
                    ],
                ),
                section(
                    "rename",
                    "Rename popup",
                    vec![spec(
                        "rename",
                        "One field, autofocused",
                        Some("360px · workspace / subtitle / tab — one view"),
                        components::rename_popup::draw,
                    )],
                ),
                section(
                    "explorer-context",
                    "Explorer context menu",
                    vec![spec(
                        "explorer-context",
                        "Right-click — four targets",
                        Some("empty · file · folder · multi-select — menu_item reuse"),
                        components::explorer_context_menu::draw,
                    )],
                ),
                section(
                    "explorer-favorite",
                    "Add to favorites popup",
                    vec![spec(
                        "explorer-favorite",
                        "Name a global favorite",
                        Some("≈280px · path caption + seeded input · anchored popup"),
                        components::explorer_favorite_popup::draw,
                    )],
                ),
                section(
                    "explorer-rename",
                    "Rename popup (explorer)",
                    vec![spec(
                        "explorer-rename",
                        "Rename a file or folder",
                        Some("≈280px · same skeleton as Add to favorites"),
                        components::explorer_rename_popup::draw,
                    )],
                ),
                section(
                    "settings",
                    "Settings window",
                    vec![spec(
                        "settings",
                        "Three-tier: tabs over sidebar over content",
                        Some("1100×700 · 7 L1 tabs · L2 sidebar · content · footer"),
                        components::settings::draw,
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
                        Some("Full 212 expands names; collapsed 52 rail keeps icon slots"),
                        components::sidebar::draw,
                    )],
                ),
                section(
                    "tabs",
                    "Tab strips",
                    vec![
                        spec(
                            "tabbar",
                            "Pane tab strip",
                            Some(
                                "24×150 tabs on bg-sidebar; active lifts to bg-panel + accent bar",
                            ),
                            components::tab_bar::draw,
                        ),
                        spec(
                            "multitab",
                            "Multi-tier tabs",
                            Some("Workspace tier + pane tier, two levels max"),
                            widgets::multi_tab_layout::draw,
                        ),
                        spec(
                            "explorer-tabs",
                            "Explorer internal tab strip",
                            Some(
                                "Surface-local · 24px · bottom accent underline (vs pane top bar)",
                            ),
                            components::explorer_tab_bar::draw,
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
                            Some("Fixed list selects, detail fills the rest"),
                            widgets::layout_1depth::draw,
                        ),
                        spec(
                            "twodepth",
                            "2-depth (Settings idiom)",
                            Some("L1 tabs (underline) + L2 sections (surface-active)"),
                            widgets::layout_2depth::draw,
                        ),
                    ],
                ),
                section(
                    "surfaces",
                    "Dividers & surfaces",
                    vec![
                        spec(
                            "divider",
                            "Pane divider",
                            Some("1px line, ~7px hit-band, accent on hover, both axes"),
                            widgets::divider::draw,
                        ),
                        spec(
                            "surface",
                            "Surface focus states",
                            Some("Focused #000, unfocused 0.92, agent dot"),
                            components::surface_highlights::draw,
                        ),
                    ],
                ),
            ],
        },
        // ── Plugins ──────────────────────────────────────────────────
        // 플러그인 유래 specimen 을 네이티브와 분리한 전용 페이지. 각 플러그인을
        // 하나의 Section 으로 묶는다(clipboard / git / markdown / image / html).
        Page {
            category: Category::Plugins,
            sections: vec![
                section(
                    "clipboard-viewer",
                    "Clipboard viewer popup",
                    vec![spec(
                        "clipboard-viewer",
                        "Current clipboard, master-detail",
                        Some("480×360 · splitter 0.3 · type list → preview · empty / read-failed"),
                        components::clipboard_viewer::draw,
                    )],
                ),
                section(
                    "git-viewer",
                    "Git worktree viewer popup",
                    vec![spec(
                        "git-viewer",
                        "Worktree rail + status / log / diff",
                        Some("≈960 · splitter H 0.25 · splitter V 0.5 · rail → status/log/diff"),
                        components::git_viewer::draw,
                    )],
                ),
                section(
                    "markdown-viewer",
                    "Markdown surface",
                    vec![spec(
                        "markdown-viewer",
                        "Markdown surface",
                        Some(
                            "6-level prose hierarchy · line-height-prose body · element catalog · load/empty states",
                        ),
                        components::markdown_viewer::draw,
                    )],
                ),
                section(
                    "image-viewer",
                    "Image surface / canvas",
                    vec![spec(
                        "image-viewer",
                        "Image surface / canvas",
                        Some("Toolbar + zoom · canvas=bg-sidebar · loaded / no-image fallback"),
                        components::image_viewer::draw,
                    )],
                ),
                section(
                    "html-chrome",
                    "HTML (webview) chrome",
                    vec![spec(
                        "html-chrome",
                        "HTML (webview) chrome",
                        Some(
                            "Native overlay · thin chrome · boundary / placeholder / loading / error",
                        ),
                        components::html_chrome::draw,
                    )],
                ),
            ],
        },
    ]
}
