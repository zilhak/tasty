//! 설정 › 단축키 › Plugins 서브탭 예제. 본체와 같은 `tasty_ui_widgets::kb_plugins_subtab` 을 부른다.
//!
//! 시안 `kb_plugins_subtab.jsx` 의 견본 넷(기본 · 초안과 해석 실패 · 녹화 중 · 빈 상태)을 위에서부터
//! 쌓는다. Custom 키는 사용자 결정대로 다른 단축키 서브탭과 같은 녹화 슬롯이다. 플러그인과 명령은
//! 시안 `KBP_PLUGINS` 와 같은 고정 데이터이고, 초안 규칙(mode 전환 시 시작값, Reset 이 override 를
//! 지움)도 시안 동작을 따른다. 키 해석 실패와 OS 키 이름은 본체와 같은
//! `tasty_key_match::first_text_key_problem` 으로 정한다. 갤러리는 키를 캡처하지 않으므로 슬롯을 누르면 녹화 중 모양만 보인다.

use std::cell::RefCell;
use std::collections::BTreeMap;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    KB_PLUGIN_MODE_CUSTOM, KB_PLUGIN_MODE_INHERIT, KbPluginKeyProblem, KbPluginLabels,
    KbPluginRowView, KbPluginSlot, KbPluginsView, kb_plugins_subtab,
};

use tasty_key_match::TextKeyProblem;

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
    /// 쉼표로 나눈 키 목록.
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
    /// 녹화 중인 (명령 키, 슬롯).
    recording: Option<(String, usize)>,
}

impl Specimen {
    fn new(
        saved: &[(&str, Value)],
        seed: &[(&str, Value)],
        recording: Option<(&str, usize)>,
    ) -> Self {
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
            recording: recording.map(|(k, i)| (k.to_string(), i)),
        }
    }
}

thread_local! {
    static SPECIMENS: RefCell<[Specimen; 3]> = RefCell::new([
        Specimen::new(&[("clipboard-viewer/clear", Value::Custom("ctrl+alt+x".into()))], &[], None),
        Specimen::new(
            &[],
            &[
                ("clipboard-viewer/open", Value::Custom("ctrl+shft+h".into())),
                ("clipboard-viewer/paste-plain", Value::None),
                ("clipboard-viewer/clear", Value::Custom("cmd+k".into())),
            ],
            None,
        ),
        Specimen::new(
            &[],
            &[
                ("clipboard-viewer/open", Value::Custom("ctrl+shift+h, ctrl+alt+v".into())),
                ("clipboard-viewer/clear", Value::Custom(String::new())),
            ],
            Some(("clipboard-viewer/open", 2)),
        ),
    ]);
}

/// 견본 위의 설명 줄 — 시안 각 견본의 caption.
const CAPTIONS: [&str; 4] = [
    "default — row 1 Custom (record slot + add) · row 2 Inherit + caption · row 3 overridden (draft = saved, Reset enabled)",
    "draft + invalid — row 1 holds an unparsable key from the file (dot + error) · row 2 switched to None (dot) · row 3 holds an OS key name (how to write it)",
    "recording — row 1 has two keys and is recording a third · row 3 Custom with no key (None slot)",
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
        draft_hint: t("settings.keybindings.plugins.draft_hint"),
        unrecognized: t("settings.keybindings.plugins.unrecognized_key"),
        os_key_name: t("keys.os_key_name"),
        press_key: t("settings.keybindings.hint_press_key"),
        no_key: t("settings.keybindings.hint_none"),
        add_hint: t("settings.keybindings.add_binding_button"),
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

fn split_custom(keys: &str) -> Vec<&str> {
    keys.split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect()
}

/// 조합의 단어 첫 글자를 대문자로 — 본체 녹화 슬롯의 사용자 표기와 같은 모양.
fn display_key(key: &str) -> String {
    key.split('+')
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
    let bufs: Vec<Vec<&str>> = values
        .iter()
        .map(|v| match v {
            Value::Custom(k) => split_custom(k),
            _ => Vec::new(),
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
    let problems: Vec<Option<KbPluginKeyProblem<'_>>> = bufs
        .iter()
        .map(|b| {
            let (slot, why) = tasty_key_match::first_text_key_problem(b)?;
            let raw = b[slot];
            Some(match why {
                TextKeyProblem::OsKeyName => KbPluginKeyProblem::OsKeyName { slot, raw },
                TextKeyProblem::Unrecognized => KbPluginKeyProblem::Unrecognized { slot, raw },
            })
        })
        .collect();
    let displays: Vec<Vec<String>> = bufs
        .iter()
        .map(|b| b.iter().map(|k| display_key(k)).collect())
        .collect();
    let display_refs: Vec<Vec<&str>> = displays
        .iter()
        .map(|d| d.iter().map(String::as_str).collect())
        .collect();

    let rows = plugin
        .commands
        .iter()
        .enumerate()
        .map(|(i, c)| KbPluginRowView {
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
                Value::Custom(_) => KbPluginSlot::Custom {
                    keys: &display_refs[i],
                    recording: s
                        .recording
                        .as_ref()
                        .and_then(|(k, idx)| (*k == keys[i]).then_some(*idx)),
                    can_record: true,
                    problem: problems[i],
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
    // 본체와 같이 플러그인·mode·Reset 이 바뀌면 녹화 중 모양을 거둔다.
    if out.plugin.is_some() || out.rows.iter().any(|o| o.mode.is_some() || o.reset) {
        s.recording = None;
    }
    for ((c, k), o) in plugin.commands.iter().zip(&keys).zip(&out.rows) {
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
        if let Some(idx) = o.record {
            s.recording = Some((k.clone(), idx));
        }
        if o.reset {
            s.drafts.remove(k);
        }
    }
}
