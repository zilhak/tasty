//! File Detectors 서브탭 — 등록 목록(두 줄 행)과 user 추가 카드.
//!
//! detector 하나에 host·plugin·user가 함께 기여한다. 행은 출처마다 Tag를 달고, 사용자 기여가
//! 있으면(규칙 없이 켜기만 바꾼 patch 포함) Remove를 둔다. Remove는 사용자 기여만 지운다.
//! 이미 있는 id로 추가하면 오류가 아니라 사용자 기여를 더하거나 바꾸는 patch다.

use crate::runtime::file_catalog::FormatCatalog as FileFormatRegistry;

use crate::file::format::{
    DetectorDecl, DetectorId, DetectorRuleDecl, DetectorRuleKind, DetectorSnapshot, RuleOrigin,
};
use crate::i18n::{t, t_fmt};
use tasty_ui_widgets::{
    FhCardLabels, FhLine, FhOrigin, FhRowView, FhSummarySeg, FhTrailing, Input,
};

use super::{AddDetectorForm, FileHandlerEditDraft, RowLabels, draw_fh_intro};

/// 저장 전 행의 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetPending {
    /// 새 id 추가.
    Add,
    /// 있는 id에 사용자 기여를 더하거나 바꾼다. `replaces_user`면 이미 사용자 기여가 있었다.
    Patch { replaces_user: bool },
    /// 사용자 기여만 있던 detector 전체가 사라진다.
    Remove,
    /// 다른 출처가 남고 사용자 기여만 사라진다.
    Unpatch,
}

/// 한 출처의 기여. 요약 조각과 출처 Tag를 만든다.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Contribution {
    origin: RuleOrigin,
    rules: Vec<String>,
}

/// 목록 한 행의 자료.
#[derive(Debug, Clone)]
struct DetRow {
    id: DetectorId,
    /// 병합 순서(Host → Plugin → User).
    contributions: Vec<Contribution>,
    /// 사용자 기여가 있는지 — 규칙이 없어도 켜기만 바꾼 기여면 true다.
    has_user: bool,
    /// 지금 초안이 저장할 켜기 상태.
    want_on: bool,
    /// 사용자 기여를 뺀 켜기 상태. 사용자 기여 삭제 대기 행의 Switch가 보인다.
    base_on: bool,
    pending: Option<DetPending>,
}

impl DetRow {
    fn others(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in &self.contributions {
            let label = c.origin.label();
            if !matches!(c.origin, RuleOrigin::User) && !c.rules.is_empty() && !out.contains(&label)
            {
                out.push(label);
            }
        }
        out
    }

    fn user_rules(&self) -> bool {
        self.contributions
            .iter()
            .any(|c| matches!(c.origin, RuleOrigin::User) && !c.rules.is_empty())
    }
}

fn origin_rank(origin: &RuleOrigin) -> u8 {
    match origin {
        RuleOrigin::HostDefault => 0,
        RuleOrigin::Plugin(_) => 1,
        RuleOrigin::User => 2,
    }
}

/// 요약 줄에 쓰는 rule 표기. ext와 path는 값을, 나머지는 rule 종류만 적는다.
fn rule_kind_text(rule: &DetectorRuleKind) -> String {
    match rule {
        DetectorRuleKind::Extension { values } => format!("ext {}", values.join(", ")),
        DetectorRuleKind::PathGlob { pattern } => format!("path {pattern}"),
        DetectorRuleKind::Mime { types } => format!("mime {}", types.join(", ")),
        DetectorRuleKind::Magic { .. } => "magic".into(),
        DetectorRuleKind::IsDirectory => "dir".into(),
        DetectorRuleKind::Lua { .. } => "lua".into(),
        DetectorRuleKind::StructureCheck { .. } => "structure".into(),
        DetectorRuleKind::Unknown { kind_name, .. } => kind_name.clone(),
    }
}

fn rule_decl_text(rule: &DetectorRuleDecl) -> String {
    match rule {
        DetectorRuleDecl::Extension { values } => format!("ext {}", values.join(", ")),
        DetectorRuleDecl::PathGlob { pattern } => format!("path {pattern}"),
        DetectorRuleDecl::Mime { types } => format!("mime {}", types.join(", ")),
        DetectorRuleDecl::Magic { .. } => "magic".into(),
        DetectorRuleDecl::IsDirectory => "dir".into(),
        DetectorRuleDecl::Lua { .. } => "lua".into(),
        DetectorRuleDecl::StructureCheck { .. } => "structure".into(),
        DetectorRuleDecl::Unknown { kind_name, .. } => kind_name.clone(),
    }
}

fn decl_contribution(decl: &DetectorDecl) -> Contribution {
    Contribution {
        origin: RuleOrigin::User,
        rules: decl.rule.iter().map(rule_decl_text).collect(),
    }
}

/// 등록 detector와 초안을 합쳐 목록 행을 만든다. 등록 순서(id 순) 뒤에 새 id 추가를 붙인다.
fn detector_rows(snapshots: &[DetectorSnapshot], fh: &FileHandlerEditDraft) -> Vec<DetRow> {
    let mut rows: Vec<DetRow> = Vec::new();
    for snap in snapshots {
        let id = snap.detector.id.clone();
        let mut contribs: Vec<&crate::file::format::ContributionSnapshot> =
            snap.contributions.iter().collect();
        contribs.sort_by_key(|c| origin_rank(&c.origin));
        let has_user_now = contribs
            .iter()
            .any(|c| matches!(c.origin, RuleOrigin::User));
        let base_on = !contribs
            .iter()
            .any(|c| !matches!(c.origin, RuleOrigin::User) && c.disabled == Some(true));
        let mut contributions: Vec<Contribution> = contribs
            .iter()
            .map(|c| Contribution {
                origin: c.origin.clone(),
                rules: c.rules.iter().map(rule_kind_text).collect(),
            })
            .collect();
        let added = fh.add_detector.iter().find(|d| d.id == id.as_str());
        let (pending, has_user, registry_on) = if let Some(decl) = added {
            contributions.retain(|c| !matches!(c.origin, RuleOrigin::User));
            contributions.push(decl_contribution(decl));
            let on = base_on && decl.disabled != Some(true);
            (
                Some(DetPending::Patch {
                    replaces_user: has_user_now,
                }),
                true,
                on,
            )
        } else {
            let pending = fh.remove_detector.contains(&id).then(|| {
                let others = contribs
                    .iter()
                    .any(|c| !matches!(c.origin, RuleOrigin::User) && !c.rules.is_empty());
                if others {
                    DetPending::Unpatch
                } else {
                    DetPending::Remove
                }
            });
            (pending, has_user_now, !snap.detector.disabled)
        };
        let want_on = fh.detector_enabled.get(&id).copied().unwrap_or(registry_on);
        rows.push(DetRow {
            id,
            contributions,
            has_user,
            want_on,
            base_on,
            pending,
        });
    }
    for decl in &fh.add_detector {
        if rows.iter().any(|r| r.id.as_str() == decl.id) {
            continue;
        }
        let id = DetectorId(decl.id.clone());
        let registry_on = decl.disabled != Some(true);
        rows.push(DetRow {
            want_on: fh.detector_enabled.get(&id).copied().unwrap_or(registry_on),
            id,
            contributions: vec![decl_contribution(decl)],
            has_user: true,
            base_on: true,
            pending: Some(DetPending::Add),
        });
    }
    rows
}

/// 행 자료를 화면 값으로 옮긴다. 시안 `detRowProps`와 같은 판단이다.
struct DetRowDisplay {
    origins: Vec<FhOrigin>,
    pending: Option<&'static str>,
    strike_id: bool,
    on: bool,
    switch_on: bool,
    switch_disabled: bool,
    trailing: FhTrailing,
    locked_tooltip: String,
    summary: Vec<FhSummarySeg>,
}

fn det_row_display(row: &DetRow) -> DetRowDisplay {
    let others = row.others();
    let user_rules = row.user_rules();
    let unpatch = row.pending == Some(DetPending::Unpatch);
    let remove = row.pending == Some(DetPending::Remove);
    let mut origins: Vec<FhOrigin> = others
        .iter()
        .map(|label| FhOrigin {
            label: label.clone(),
            plugin: label.starts_with("plugin:"),
            struck: false,
        })
        .collect();
    if row.has_user {
        origins.push(FhOrigin {
            label: if user_rules {
                "user".to_string()
            } else {
                t("settings.file_handler.detectors.origin_user_override").to_string()
            },
            plugin: false,
            struck: unpatch,
        });
    }
    let pending = row.pending.map(|p| match p {
        DetPending::Add => t("settings.file_handler.common.pending_added"),
        DetPending::Remove => t("settings.file_handler.common.pending_removed"),
        DetPending::Patch {
            replaces_user: true,
        } => t("settings.file_handler.detectors.pending_yours_replaced"),
        DetPending::Patch {
            replaces_user: false,
        } => t("settings.file_handler.detectors.pending_yours_added"),
        DetPending::Unpatch if user_rules => {
            t("settings.file_handler.detectors.pending_yours_removed")
        }
        DetPending::Unpatch => t("settings.file_handler.detectors.pending_override_removed"),
    });
    let summary = row
        .contributions
        .iter()
        .flat_map(|c| {
            let struck = unpatch && matches!(c.origin, RuleOrigin::User);
            c.rules.iter().map(move |text| FhSummarySeg {
                text: text.clone(),
                struck,
            })
        })
        .collect();
    DetRowDisplay {
        origins,
        pending,
        strike_id: remove,
        on: row.want_on && !remove,
        switch_on: if unpatch { row.base_on } else { row.want_on },
        switch_disabled: remove || unpatch,
        trailing: if !row.has_user {
            FhTrailing::Lock
        } else if row.pending.is_some() {
            FhTrailing::Undo
        } else {
            FhTrailing::Remove
        },
        locked_tooltip: t_fmt("settings.file_handler.common.locked", &others.join(", ")),
        summary,
    }
}

/// Detectors sub-tab — 머리, 추가 카드, 등록 목록.
pub(super) fn draw_detectors(
    ui: &mut egui::Ui,
    fh: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
) {
    let th = crate::theme::theme();
    ui.spacing_mut().item_spacing.y = th.spacing_md.value();
    if draw_fh_intro(
        ui,
        "settings.file_handler.detectors.description",
        "settings.file_handler.detectors.add",
        fh.add_detector_form.open,
    ) {
        fh.add_detector_form = AddDetectorForm {
            open: true,
            ..AddDetectorForm::default()
        };
    }
    let rows = detector_rows(&file_format.detector_snapshots(), fh);
    if fh.add_detector_form.open {
        draw_add_detector_card(ui, fh, &rows);
    }
    tasty_ui_widgets::fh_list_head(
        ui,
        &th,
        &t_fmt(
            "settings.file_handler.detectors.registered",
            &rows.len().to_string(),
        ),
    );
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if rows.is_empty() {
            tasty_ui_widgets::fh_empty(ui, &th, t("settings.file_handler.detectors.empty"));
        }
        let labels = RowLabels::get();
        for row in &rows {
            let d = det_row_display(row);
            let sources = t_fmt(
                "settings.file_handler.common.sources",
                &d.origins.len().to_string(),
            );
            let view = FhRowView {
                id: row.id.as_str(),
                origins: d.origins,
                prio: None,
                pending: d.pending,
                strike_id: d.strike_id,
                on: d.on,
                switch_on: d.switch_on,
                switch_disabled: d.switch_disabled,
                trailing: d.trailing,
                locked_tooltip: &d.locked_tooltip,
                summary: d.summary,
                undo_label: labels.undo,
                remove_label: labels.remove,
                sources_label: &sources,
            };
            let ev = ui
                .push_id(("fh_detector_row", row.id.as_str()), |ui| {
                    tasty_ui_widgets::fh_row(ui, &th, &view)
                })
                .inner;
            if let Some(on) = ev.toggled {
                fh.detector_enabled.insert(row.id.clone(), on);
            }
            if ev.remove {
                fh.remove_detector.insert(row.id.clone());
            }
            if ev.undo {
                undo_detector(fh, row);
            }
        }
    });
}

/// 대기를 되돌린다. 새 id 추가를 되돌리면 그 id의 켜기 변경도 버린다.
fn undo_detector(fh: &mut FileHandlerEditDraft, row: &DetRow) {
    match row.pending {
        Some(DetPending::Add) => {
            fh.add_detector.retain(|d| d.id != row.id.as_str());
            fh.detector_enabled.remove(&row.id);
        }
        Some(DetPending::Patch { .. }) => {
            fh.add_detector.retain(|d| d.id != row.id.as_str());
        }
        Some(DetPending::Remove | DetPending::Unpatch) => {
            fh.remove_detector.remove(&row.id);
        }
        None => {}
    }
}

/// 카드의 검증 결과. 오류는 Add를 누른 뒤부터 보이고 안내는 입력하는 동안 보인다.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DetectorCheck {
    id: String,
    /// 이미 목록에 있는 id다(patch).
    exists: bool,
    err_id: Option<&'static str>,
    info: Option<String>,
    err_rules: Option<&'static str>,
}

impl DetectorCheck {
    fn can_add(&self) -> bool {
        !self.id.is_empty()
    }
    fn valid(&self) -> bool {
        self.err_id.is_none() && self.err_rules.is_none()
    }
}

fn check_detector_form(form: &AddDetectorForm, rows: &[DetRow]) -> DetectorCheck {
    let id = form.id_input.trim().to_string();
    let row = rows.iter().find(|r| r.id.as_str() == id);
    let exists = !id.is_empty() && row.is_some();
    let err_id = if id.is_empty() {
        None
    } else if !crate::file::format::is_valid_detector_id(&id) {
        Some(t("settings.file_handler.detectors.err_id_chars"))
    } else if id.starts_with('$') && !exists {
        Some(t("settings.file_handler.detectors.err_reserved"))
    } else {
        None
    };
    let info = match row {
        Some(r) if err_id.is_none() => {
            let replaces = match r.pending {
                Some(DetPending::Add) => true,
                Some(DetPending::Patch { replaces_user }) => replaces_user,
                _ => r.has_user,
            };
            Some(if replaces {
                t_fmt("settings.file_handler.detectors.info_replace", &id)
            } else {
                t_fmt("settings.file_handler.detectors.info_patch", &id)
            })
        }
        _ => None,
    };
    let err_rules = (parse_extensions(&form.extensions_input).is_empty()
        && form.path_glob_input.trim().is_empty())
    .then(|| t("settings.file_handler.detectors.err_no_rules"));
    DetectorCheck {
        id,
        exists,
        err_id,
        info,
        err_rules,
    }
}

/// 콤마나 공백으로 나누고 앞의 점을 떼고 소문자로 바꾼다.
fn parse_extensions(input: &str) -> Vec<String> {
    input
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|s| {
            let v = s.trim().trim_start_matches('.').to_ascii_lowercase();
            (!v.is_empty()).then_some(v)
        })
        .collect()
}

fn draw_add_detector_card(ui: &mut egui::Ui, fh: &mut FileHandlerEditDraft, rows: &[DetRow]) {
    let th = crate::theme::theme();
    let check = check_detector_form(&fh.add_detector_form, rows);
    let tried = fh.add_detector_form.tried;
    let add_label = if check.exists && check.err_id.is_none() {
        t("settings.file_handler.detectors.update")
    } else {
        t("settings.file_handler.detectors.add")
    };
    let labels = FhCardLabels {
        title: t("settings.file_handler.detectors.card_title"),
        cancel: t("button.cancel"),
        add: add_label,
    };
    let rules_invalid = tried && check.err_rules.is_some();
    let form = &mut fh.add_detector_form;
    let ev = tasty_ui_widgets::fh_card(
        ui,
        &th,
        labels,
        if tried { check.err_rules } else { None },
        check.can_add(),
        |ui| {
            tasty_ui_widgets::fh_field(
                ui,
                &th,
                t("settings.file_handler.detectors.field_id"),
                FhLine::pick(
                    if tried { check.err_id } else { None },
                    check.info.as_deref(),
                    Some(t("settings.file_handler.detectors.id_hint")),
                ),
                |ui| {
                    Input::new()
                        .mono(true)
                        .placeholder("my-detector")
                        .invalid(tried && check.err_id.is_some())
                        .show(ui, &th, &mut form.id_input);
                },
            );
            tasty_ui_widgets::fh_field(
                ui,
                &th,
                t("settings.file_handler.detectors.field_extensions"),
                FhLine::Caption(t("settings.file_handler.detectors.ext_hint")),
                |ui| {
                    Input::new()
                        .mono(true)
                        .placeholder("md, mdx")
                        .invalid(rules_invalid)
                        .show(ui, &th, &mut form.extensions_input);
                },
            );
            tasty_ui_widgets::fh_field(
                ui,
                &th,
                t("settings.file_handler.detectors.field_path_glob"),
                FhLine::None,
                |ui| {
                    Input::new()
                        .mono(true)
                        .placeholder("Dockerfile")
                        .invalid(rules_invalid)
                        .show(ui, &th, &mut form.path_glob_input);
                },
            );
        },
    );
    if ev.cancel {
        fh.add_detector_form = AddDetectorForm::default();
    } else if ev.add {
        commit_detector(fh, rows);
    }
}

/// Add를 누른다. 오류가 있으면 표시만 켜고, 없으면 초안에 넣고 카드를 닫는다.
/// 같은 id의 앞선 추가는 바꾸고, 같은 id의 삭제 대기는 이 추가가 대신한다.
fn commit_detector(fh: &mut FileHandlerEditDraft, rows: &[DetRow]) {
    fh.add_detector_form.tried = true;
    let check = check_detector_form(&fh.add_detector_form, rows);
    if !check.can_add() || !check.valid() {
        return;
    }
    let Ok(mut decl) = build_add_detector_decl(&fh.add_detector_form) else {
        return;
    };
    // 사용자 기여를 통째로 바꾸므로 지금 꺼져 보이는 detector는 꺼진 채로 둔다.
    if rows.iter().any(|r| r.id.as_str() == decl.id && !r.want_on) {
        decl.disabled = Some(true);
    }
    let id = DetectorId(decl.id.clone());
    fh.remove_detector.remove(&id);
    fh.add_detector.retain(|d| d.id != decl.id);
    fh.add_detector.push(decl);
    fh.add_detector_form = AddDetectorForm::default();
}

/// 카드 입력을 저장할 선언으로 바꾼다. 저장 id는 입력값 그대로다(접두사 없음).
fn build_add_detector_decl(form: &AddDetectorForm) -> Result<DetectorDecl, &'static str> {
    let id = form.id_input.trim().to_string();
    if !crate::file::format::is_valid_detector_id(&id) {
        return Err(t("settings.file_handler.detectors.err_id_chars"));
    }
    let mut rules = Vec::new();
    let exts = parse_extensions(&form.extensions_input);
    if !exts.is_empty() {
        rules.push(DetectorRuleDecl::Extension { values: exts });
    }
    let glob = form.path_glob_input.trim();
    if !glob.is_empty() {
        rules.push(DetectorRuleDecl::PathGlob {
            pattern: glob.to_string(),
        });
    }
    if rules.is_empty() {
        return Err(t("settings.file_handler.detectors.err_no_rules"));
    }
    Ok(DetectorDecl {
        id,
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: rules,
    })
}

#[cfg(test)]
mod tests;
