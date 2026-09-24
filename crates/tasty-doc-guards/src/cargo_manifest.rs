//! 추가 TOML 의존성 없이 매니페스트의 의존 선언을 읽는다.
//! 완전한 TOML 파서가 아니며 지원하는 형태는 소비자의 합성 입력으로 확인한다.

/// 의존 선언을 (절 종류, 크레이트 이름)으로 반환한다.
/// 일반 대입·인라인 테이블·dependencies.<이름>·target 조건부 선언을 지원한다.
/// target 조건은 반환값에 포함하지 않고 dependencies/dev-dependencies/build-dependencies로 구분한다.
pub fn declared_deps(manifest: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut section: Option<String> = None;
    for line in manifest.lines() {
        let t = line.trim();
        if let Some(header) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let leaf = header.rsplit('.').next().unwrap_or(header);
            if is_dep_section(leaf) {
                section = Some(leaf.to_string());
                continue;
            }
            // `[dependencies.foo]` · `[target.'cfg(unix)'.build-dependencies.foo]` 는
            // 헤더 자신이 이름을 담는다.
            let mut parts = header.rsplitn(2, '.');
            let last = parts.next().unwrap_or_default();
            let head = parts.next().unwrap_or_default();
            let head_leaf = head.rsplit('.').next().unwrap_or(head);
            if is_dep_section(head_leaf) && !last.is_empty() {
                out.push((head_leaf.to_string(), last.to_string()));
            }
            section = None;
            continue;
        }
        let Some(sec) = section.as_deref() else {
            continue;
        };
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = t.split_once('=') {
            let name = name.trim().trim_matches('"');
            if !name.is_empty() {
                out.push((sec.to_string(), name.to_string()));
            }
        }
    }
    out
}

/// 그 이름을 선언한 절들. 없으면 빈 벡터다.
pub fn sections_declaring(manifest: &str, name: &str) -> Vec<String> {
    declared_deps(manifest)
        .into_iter()
        .filter(|(_, n)| n == name)
        .map(|(s, _)| s)
        .collect()
}

fn is_dep_section(leaf: &str) -> bool {
    matches!(
        leaf,
        "dependencies" | "dev-dependencies" | "build-dependencies"
    )
}

/// 지정 feature의 dep: 항목에서 optional 의존 이름을 선언 순서대로 추출한다.
/// 다른 크레이트의 feature를 켜는 항목은 제외한다. 해당 feature나 dep: 항목이 없으면 빈 벡터다.
pub fn feature_enabled_deps(manifest: &str, feature: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_features = false;
    let mut in_array = false;
    for line in manifest.lines() {
        let t = line.split('#').next().unwrap_or("").trim();
        if !in_array {
            if let Some(header) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                in_features = header.trim() == "features";
                continue;
            }
            if !in_features {
                continue;
            }
            let Some((name, rest)) = t.split_once('=') else {
                continue;
            };
            if name.trim().trim_matches('"') != feature {
                continue;
            }
            in_array = true;
            collect_dep_entries(rest, &mut out);
            if rest.contains(']') {
                return out;
            }
            continue;
        }
        collect_dep_entries(t, &mut out);
        if t.contains(']') {
            return out;
        }
    }
    out
}

fn collect_dep_entries(text: &str, out: &mut Vec<String>) {
    for piece in text.split('"').skip(1).step_by(2) {
        if let Some(dep) = piece.strip_prefix("dep:") {
            out.push(dep.to_string());
        }
    }
}
