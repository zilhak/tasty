//! 설정 › 단축키 › Plugins 서브탭 예제. 본체와 같은 `tasty_ui_widgets::kb_plugins_subtab` 을 부른다.
//!
//! 시안 `kb_plugins_subtab.jsx` 의 견본 넷(기본 · 초안과 해석 실패 · 녹화 버튼 대안 · 빈 상태)을
//! 위에서부터 쌓는다. 플러그인과 명령은 시안 `KBP_PLUGINS` 와 같은 고정 데이터이고, 초안 규칙
//! (mode 전환 시 시작값, Reset 이 override 를 지움)도 시안 동작을 따른다. 키 해석 실패는 본체와 같은
//! `tasty_key_match::binding_key_recognized` 로 정한다.

use std::cell::RefCell;
use std::collections::BTreeMap;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    KB_PLUGIN_MODE_CUSTOM, KB_PLUGIN_MODE_INHERIT, KbPluginLabels, KbPluginRowView, KbPluginSlot,
    KbPluginsView, kb_plugins_subtab,
};

use crate::i18n::{t, t_fmt};

/// 상속 소스 후보 — 시안 `KBP_SOURCES`.
const SOURCES: &[&str] = &[
    "clipboard.copy",
    "clipboard.paste",
    "clipboard.cut",
    "select_all",
];

/// 상속 소스별 해석된 호스트 키 — 시안 `KBP_RESOLVED`. `None` 이면 호스트 동작에 키가 없다.
fn resolved(source: &str) -> Option<&'static str> {
    match source {
        "clipboard.copy" => Some("Ctrl+Shift+C"),
        "clipboard.paste" => Some("Ctrl+Shift+V"),
        "select_all" => Some("Ctrl+Shift+A"),
        _ => None,
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Value {
    Inherit(&'static str),
    Custom(String),
    None,
}

struct Command {
    id: &'static str,
    title: &'static str,
    manifest: Value,
}

struct Plugin {
    id: &'static str,
    name: &'static str,
    commands: Vec<Command>,
}

/// 시안 `KBP_PLUGINS`. 이름순으로 둔다.
fn plugins() -> Vec<Plugin> {
    let cmd = |id, title, manifest| Command {
        id,
        title,
        manifest,
    };
    vec![
        Plugin {
            id: "clipboard-viewer",
            name: "Clipboard Viewer",
            commands: vec![
                cmd(
                    "open",
                    "Open clipboard viewer",
                    Value::Custom("ctrl+shift+h".into()),
                ),
                cmd(
                    "paste-plain",
                    "Paste last entry as plain text",
                    Value::Inherit("clipboard.paste"),
                ),
                cmd("clear", "Clear history", Value::None),
            ],
        },
        Plugin {
            id: "git-helper",
            name: "Git Helper",
            commands: vec![
                cmd("stage", "Stage hunk", Value::Custom("ctrl+alt+g".into())),
                cmd(
                    "copy-sha",
                    "Copy commit SHA",
                    Value::Inherit("clipboard.copy"),
                ),
            ],
        },
    ]
}

/// 견본 하나의 상태. `saved` 는 마지막 Save 가 쓴 값, `drafts` 는 화면의 초안(처음엔 saved 와 같다).
struct Specimen {
    plugin: usize,
    saved: BTreeMap<String, Value>,
    drafts: BTreeMap<String, Value>,
    record_alt: bool,
}

impl Specimen {
    fn new(saved: &[(&str, Value)], seed: &[(&str, Value)], record_alt: bool) -> Self {
        let saved: BTreeMap<String, Value> = saved
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        let mut drafts = saved.clone();
        drafts.extend(seed.iter().map(|(k, v)| (k.to_string(), v.clone())));
        Self {
            plugin: 0,
            saved,
            drafts,
            record_alt,
        }
    }
}

thread_local! {
    static SPECIMENS: RefCell<[Specimen; 3]> = RefCell::new([
        Specimen::new(&[("clipboard-viewer/clear", Value::Custom("ctrl+alt+x".into()))], &[], false),
        Specimen::new(
            &[],
            &[
                ("clipboard-viewer/open", Value::Custom("ctrl+shft+h".into())),
                ("clipboard-viewer/paste-plain", Value::None),
            ],
            false,
        ),
        Specimen::new(&[("clipboard-viewer/clear", Value::Custom("ctrl+alt+x".into()))], &[], true),
    ]);
}

/// 견본 위의 설명 줄 — 시안 각 견본의 caption.
const CAPTIONS: [&str; 4] = [
    "default (proposal) — text key entry · row 1 Custom · row 2 Inherit + caption · row 3 overridden (draft = saved, Reset enabled)",
    "draft + invalid — row 1 edited to an unparsable key (dot + error) · row 2 switched to None (dot)",
    "alternative (needs user decision) — Custom slot = record button, like the other subtabs",
    "empty",
];

fn labels() -> KbPluginLabels<'static> {
    KbPluginLabels {
        plugin: t("settings.keybindings.plugins.plugin_label"),
        empty: t("settings.keybindings.plugins.no_plugins_with_commands"),
        modes: [
            t("settings.keybindings.plugins.mode_inherit"),
            t("settings.keybindings.plugins.mode_custom"),
            t("settings.keybindings.plugins.mode_none"),
        ],
        unassigned: t("settings.keybindings.plugins.mode_none_label"),
        reset: t("settings.keybindings.plugins.reset_button"),
        reset_hint: t("settings.keybindings.plugins.reset_hint"),
        key_placeholder: "ctrl+f5",
        draft_hint: t("settings.keybindings.plugins.draft_hint"),
        unrecognized: t("settings.keybindings.plugins.unrecognized_key"),
        // 녹화 버튼 대안은 사용자 결정 전 견본이라 본체 문구가 없다 — 시안 문구.
        record: "Record shortcut",
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let width = theme.settings_content_max_width().value();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xl.value();
        for (i, caption) in CAPTIONS.iter().enumerate() {
            ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                ui.label(
                    egui::RichText::new(*caption)
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
                if i == 3 {
                    kb_plugins_subtab(
                        ui,
                        theme,
                        ("kb_plugins_specimen", i),
                        KbPluginsView {
                            plugins: &[],
                            selected: 0,
                            rows: Vec::new(),
                            labels: labels(),
                        },
                    );
                } else {
                    SPECIMENS.with(|s| specimen(ui, theme, i, &mut s.borrow_mut()[i]));
                }
            });
        }
    });
}

/// 첫 조합의 단어 첫 글자를 대문자로 — 시안 녹화 버튼의 Kbd 표기.
fn record_keys(keys: &str) -> String {
    let first = keys.split(',').next().unwrap_or("").trim();
    first
        .split('+')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(h) => h.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn specimen(ui: &mut egui::Ui, theme: &Theme, index: usize, s: &mut Specimen) {
    let plugins = plugins();
    let names: Vec<&str> = plugins.iter().map(|p| p.name).collect();
    let plugin = &plugins[s.plugin.min(plugins.len() - 1)];
    let keys: Vec<String> = plugin
        .commands
        .iter()
        .map(|c| format!("{}/{}", plugin.id, c.id))
        .collect();
    let values: Vec<Value> = plugin
        .commands
        .iter()
        .zip(&keys)
        .map(|(c, k)| {
            s.drafts
                .get(k)
                .cloned()
                .unwrap_or_else(|| c.manifest.clone())
        })
        .collect();
    let mut bufs: Vec<String> = values
        .iter()
        .map(|v| match v {
            Value::Custom(k) => k.clone(),
            _ => String::new(),
        })
        .collect();
    let captions: Vec<String> = values
        .iter()
        .map(|v| match v {
            Value::Inherit(src) => match resolved(src) {
                Some(k) => t_fmt("settings.keybindings.plugins.inherited", k),
                None => t("settings.keybindings.hint_none").to_string(),
            },
            _ => String::new(),
        })
        .collect();
    let errors: Vec<Option<String>> = bufs
        .iter()
        .map(|b| {
            b.split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .find(|k| !tasty_key_match::binding_key_recognized(k))
                .map(str::to_string)
        })
        .collect();
    let records: Vec<String> = bufs.iter().map(|b| record_keys(b)).collect();

    let rows = plugin
        .commands
        .iter()
        .enumerate()
        .zip(bufs.iter_mut())
        .map(|((i, c), buf)| KbPluginRowView {
            id: egui::Id::new(("kb_plugins_specimen", index, &keys[i])),
            title: c.title,
            dirty: s.drafts.get(&keys[i]) != s.saved.get(&keys[i]),
            overridden: s.drafts.contains_key(&keys[i]),
            slot: match &values[i] {
                Value::Inherit(src) => KbPluginSlot::Inherit {
                    sources: SOURCES,
                    selected: SOURCES.iter().position(|x| x == src).unwrap_or(0),
                    caption: &captions[i],
                },
                Value::Custom(_) if s.record_alt => KbPluginSlot::Record {
                    keys: (!buf.is_empty()).then_some(records[i].as_str()),
                },
                Value::Custom(_) => KbPluginSlot::Custom {
                    keys: buf,
                    error: errors[i].as_deref(),
                },
                Value::None => KbPluginSlot::Unassigned,
            },
        })
        .collect();

    let out = kb_plugins_subtab(
        ui,
        theme,
        ("kb_plugins_specimen", index),
        KbPluginsView {
            plugins: &names,
            selected: s.plugin,
            rows,
            labels: labels(),
        },
    );

    if let Some(p) = out.plugin {
        s.plugin = p;
    }
    for (((c, k), buf), o) in plugin.commands.iter().zip(&keys).zip(&bufs).zip(&out.rows) {
        if let Some(m) = o.mode {
            let draft_keys = match s.drafts.get(k) {
                Some(Value::Custom(v)) => Some(v.clone()),
                _ => None,
            };
            let next = match m {
                KB_PLUGIN_MODE_CUSTOM => {
                    Value::Custom(draft_keys.unwrap_or_else(|| match &c.manifest {
                        Value::Custom(v) => v.clone(),
                        _ => String::new(),
                    }))
                }
                KB_PLUGIN_MODE_INHERIT => Value::Inherit(match c.manifest {
                    Value::Inherit(src) => src,
                    _ => SOURCES[0],
                }),
                _ => Value::None,
            };
            s.drafts.insert(k.clone(), next);
        }
        if let Some(i) = o.source {
            s.drafts.insert(k.clone(), Value::Inherit(SOURCES[i]));
        }
        if o.keys_changed {
            s.drafts.insert(k.clone(), Value::Custom(buf.clone()));
        }
        if o.reset {
            s.drafts.remove(k);
        }
    }
}
