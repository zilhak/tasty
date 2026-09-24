//! Markdown을 호스트 WebView에 표시할 HTML 문서로 만든다.
//!
//! 사용자 본문은 pulldown-cmark로 파싱하고 ammonia의 허용 목록으로 정리한다.
//! Markdown 링크는 #tasty-nav 마커로 바꿔 호스트에 열기를 요청하며,
//! 문서 안 앵커는 nav_script에서 스크롤한다. 마커 자체가 사용자 입력을 증명하지는 않는다.
//! 테마 CSS와 플러그인이 제공하는 스크립트는 본문 정리 후 추가한다.
//!
//! 로컬 이미지는 inline_local_images에서 읽어 data URI로 바꾼다. <base href>는
//! 넣지 않는다. file URL의 base를 넣으면 #slug도 다른 문서로 해석되기 때문이다.
//! Raw HTML의 상대 링크는 본문을 정리한 뒤에도 남을 수 있다.
//!
//! 상세: docs/plugins/markdown/index.md#내부-동작.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag,
    TagEnd,
};
use tasty_plugin_sdk::Translator;
use tasty_type_appearance::theme::Theme;

/// 내부 이동 URL의 마커. 생성 코드와 parse_nav_fragment에서 함께 사용한다.
pub const NAV_FRAGMENT_MARKER: &str = "tasty-nav:";

/// 새로고침 마커. 같은 hash를 다시 지정해도 이동 이벤트가 생기지 않아 nonce를 붙인다.
const NAV_REFRESH_PREFIX: &str = "refresh:";

/// 호스트에 요청할 링크 대상. File은 기준 폴더에서 해석한 경로이고,
/// External은 OS의 기본 앱으로 열 URL이다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkClick {
    File(PathBuf),
    External(String),
}

/// #tasty-nav 마커에서 해석한 요청 종류.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavIntent {
    /// A content link (`<a>`) was clicked — `dest` is the original, un-rewritten href.
    Link(String),
    /// The address bar's Go action (click or Enter) fired — `path` is the input's raw value.
    Addr(String),
    /// attach mirror 문서의 새로고침 버튼이 눌렸다 — 원격 원문을 다시 요청한다.
    Refresh,
}

/// 본문과 사전 탐색에 함께 사용하는 파서 옵션.
/// 표, 작업 목록, 취소선, 각주, 정의 목록, 콜아웃과 수식을 처리한다.
/// 문서 시작의 --- 또는 +++ 메타데이터 블록은 본문에 표시하지 않는다.
/// 스마트 문장부호 변환은 항상 켜져 있으며 별도 설정은 없다.
fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_GFM
        | Options::ENABLE_MATH
}

/// HTML 문서 생성에 필요한 본문, 테마와 표시 상태.
pub struct DocumentInput<'a> {
    pub theme: &'a Theme,
    pub tr: &'a Translator,
    pub file_path: &'a str,
    pub source: &'a str,
    pub load_error: Option<&'a str>,
    pub base_dir: Option<&'a Path>,
    /// Newest-first recent paths (already capped upstream by `recent.query`) — baked into the
    /// address bar's `<datalist>` at generation time (no async JS fetch / native message
    /// bridge exists for a webview surface, see module doc).
    pub recent: &'a [String],
    /// attach mirror 문서면 `Some` — 원문이 로컬 파일이 아니라 원격에서 주입된 것이다
    /// (`docs/dev-guide/attach-behavior.md#markdown-content-채널`). 주소창은
    /// 읽기 전용이 되고 우측 상단에 새로고침 버튼이 붙는다.
    pub remote: Option<RemoteView>,
}

/// attach mirror 문서의 표시 상태.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemoteView {
    /// 원문을 아직 한 번도 받지 못했다 — 본문 자리에 로딩 상태를 그린다.
    pub loading: bool,
    /// 받은 뒤 원격 파일이 바뀌었다는 신호가 왔다 — 새로고침 버튼 색이 바뀐다.
    pub stale: bool,
    /// attach 연결이 끊겼다 — 본문 자리에 끊김을 그린다. 로딩·실패·원문보다 앞선다:
    /// 끊긴 뒤의 원문은 연결이 살아 있는 화면과 구분되지 않는다.
    pub disconnected: bool,
}

/// Build the complete, self-contained HTML5 document for the markdown webview surface.
pub(crate) fn render_document(input: DocumentInput) -> String {
    let DocumentInput {
        theme,
        tr,
        file_path,
        source,
        load_error,
        base_dir,
        recent,
        remote,
    } = input;

    let (body_html, headings) = if remote.is_some_and(|r| r.disconnected) {
        (
            format!(
                r#"<div class="tasty-state tasty-state-error"><div class="tasty-state-title">{}</div></div>"#,
                html_escape(tr.t("markdown.remote.disconnected"))
            ),
            Vec::new(),
        )
    } else if remote.is_some_and(|r| r.loading) && load_error.is_none() {
        (
            format!(
                r#"<div class="tasty-state">{}</div>"#,
                html_escape(tr.t("markdown.remote.loading"))
            ),
            Vec::new(),
        )
    } else if let Some(err) = load_error {
        (
            format!(
                r#"<div class="tasty-state tasty-state-error"><div class="tasty-state-title">{}</div><pre class="tasty-state-detail">{}</pre></div>"#,
                html_escape(tr.t("markdown.state.failed")),
                html_escape(err)
            ),
            Vec::new(),
        )
    } else if source.trim().is_empty() {
        (
            format!(
                r#"<div class="tasty-state">{}</div>"#,
                html_escape(tr.t("markdown.state.empty"))
            ),
            Vec::new(),
        )
    } else {
        (
            inline_local_images(
                &sanitize_html(&unsafe_content_html_in_dir(source, tr, base_dir)),
                base_dir,
            ),
            collect_headings(source),
        )
    };

    // heading 이 하나도 없으면 TOC 영역 자체를 렌더하지 않는다(빈 nav 로 깨지지 않게).
    let toc_html = if headings.is_empty() {
        String::new()
    } else {
        toc_nav_html(tr, &headings)
    };

    let mermaid = if body_html.contains("language-mermaid") {
        mermaid_script(theme.is_light)
    } else {
        String::new()
    };

    let highlight = if body_html.contains("class=\"language-") {
        highlight_script()
    } else {
        String::new()
    };

    let copy_buttons = if body_html.contains("<pre><code") {
        copy_button_script(tr)
    } else {
        String::new()
    };

    // Same convention again — most documents have no images at all, so skip the attach script
    // entirely when there's nothing for it to watch.
    let image_errors = if body_html.contains("<img") {
        image_error_script(tr)
    } else {
        String::new()
    };

    // Same convention again — the KaTeX bundle+fonts are ~1MB embedded, so most documents (no
    // `$...$`/`$$...$$` math) skip it entirely. `class="math math-` matches both
    // `math math-inline` and `math math-display` (pulldown-cmark's `ENABLE_MATH` HTML writer's
    // only two possible class values), so one substring check covers both.
    let math = if body_html.contains("class=\"math math-") {
        katex_script()
    } else {
        String::new()
    };

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><style>{css}</style></head><body>{addr_bar}{find_bar}{toc_html}<div id="tasty-md-body">{body_html}</div><script>{script}</script><script>{find_script}</script>{highlight}{mermaid}{copy_buttons}{image_errors}{math}</body></html>"#,
        css = theme_css(theme),
        addr_bar = addr_bar_html(tr, file_path, recent, remote),
        find_bar = find_bar_html(tr),
        toc_html = toc_html,
        body_html = body_html,
        script = nav_script(file_path),
        find_script = find_in_page_script(tr),
        highlight = highlight,
        mermaid = mermaid,
        copy_buttons = copy_buttons,
        image_errors = image_errors,
        math = math,
    )
}

/// URL fragment에서 내부 이동 요청을 해석한다.
pub(crate) fn parse_nav_fragment(url: &str) -> Option<NavIntent> {
    let idx = url.rfind(NAV_FRAGMENT_MARKER)?;
    let payload = &url[idx + NAV_FRAGMENT_MARKER.len()..];
    if let Some(enc) = payload.strip_prefix("link:") {
        return Some(NavIntent::Link(percent_decode(enc)));
    }
    if let Some(enc) = payload.strip_prefix("addr:") {
        return Some(NavIntent::Addr(percent_decode(enc)));
    }
    if payload.starts_with(NAV_REFRESH_PREFIX) {
        return Some(NavIntent::Refresh);
    }
    None
}

/// 링크를 파일 경로나 외부 URL로 분류한다. 빈 값, 문서 안 앵커와 javascript:는 제외한다.
pub(crate) fn classify_link(dest: &str, base_dir: Option<&Path>) -> Option<LinkClick> {
    let dest = dest.trim();
    if dest.is_empty() || dest.starts_with('#') {
        return None;
    }
    if dest.to_ascii_lowercase().starts_with("javascript:") {
        return None;
    }
    if dest.contains("://") || dest.starts_with("mailto:") || dest.starts_with("data:") {
        return Some(LinkClick::External(dest.to_string()));
    }
    let path = Path::new(dest);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir?.join(path)
    };
    // 현재 작업 폴더 기준 절대경로로 바꾼 뒤 ..를 정리한다.
    let abs = std::path::absolute(&joined).unwrap_or(joined);
    let abs = lexically_normalize(&abs);
    let abs = PathBuf::from(strip_verbatim_prefix(&abs.to_string_lossy()));
    Some(LinkClick::File(abs))
}

/// Strip a Windows verbatim (`\\?\`) prefix so resolved paths read cleanly cross-platform.
fn strip_verbatim_prefix(s: &str) -> String {
    s.strip_prefix(r"\\?\")
        .map(|rest| {
            rest.strip_prefix(r"UNC\")
                .map(|u| format!(r"\\{u}"))
                .unwrap_or_else(|| rest.to_string())
        })
        .unwrap_or_else(|| s.to_string())
}

/// 경로의 .과 ..를 파일 시스템 조회 없이 정리한다.
/// 심볼릭 링크를 해석하거나 경로 접근 권한을 확인하는 함수는 아니다.
fn lexically_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ── heading ids + TOC ───────────────────────────────────────────────────────────

/// 목차와 제목 ID에 함께 사용하는 제목 정보.
#[derive(Clone, Debug)]
struct HeadingInfo {
    level: HeadingLevel,
    /// 제목 안의 텍스트. 명시적 {#id} 문법은 지원하지 않는다.
    text: String,
    /// Slugger에서 만든 제목 ID.
    slug: String,
}

/// 제목 안의 텍스트를 모아 목차 항목과 ID를 만든다.
/// 인라인 코드와 강조 안의 텍스트도 포함하며 제목이 닫힐 때 항목을 저장한다.
fn collect_headings(source: &str) -> Vec<HeadingInfo> {
    let mut headings = Vec::new();
    let mut current: Option<(HeadingLevel, String)> = None;
    let mut slugger = Slugger::default();
    for event in Parser::new_ext(source, parser_options()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => current = Some((level, String::new())),
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = current.take() {
                    let slug = slugger.slug(&text);
                    headings.push(HeadingInfo { level, text, slug });
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, text)) = current.as_mut() {
                    text.push_str(&t);
                }
            }
            _ => {}
        }
    }
    headings
}

/// 정규화한 제목별 사용 횟수. 같은 이름이 반복되면 숫자 접미사를 붙인다.
#[derive(Default)]
struct Slugger {
    seen: HashMap<String, u32>,
}

impl Slugger {
    /// 제목을 소문자로 바꾸고 공백을 하이픈으로 치환한 뒤 허용 문자를 남긴다.
    /// 같은 기본 이름이 반복되면 -1, -2를 붙인다. 서로 다른 기본 이름이
    /// 생성된 접미사와 겹치는 경우까지 전역 유일성을 보장하지는 않는다.
    fn slug(&mut self, text: &str) -> String {
        let mut base = String::with_capacity(text.len());
        for c in text.chars() {
            if c.is_whitespace() {
                if !base.ends_with('-') && !base.is_empty() {
                    base.push('-');
                }
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                base.extend(c.to_lowercase());
            }
            // else: drop (punctuation/symbols/markup residue).
        }
        let base = base.trim_matches('-');
        let base = if base.is_empty() {
            "heading".to_string()
        } else {
            base.to_string()
        };

        let count = self.seen.entry(base.clone()).or_insert(0);
        let slug = if *count == 0 {
            base
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        slug
    }
}

/// 사전 탐색에서 만든 ID를 제목 이벤트에 순서대로 넣는다.
fn assign_heading_ids<'a>(
    events: impl Iterator<Item = Event<'a>> + 'a,
    headings: &[HeadingInfo],
) -> impl Iterator<Item = Event<'a>> + 'a {
    let mut slugs = headings
        .iter()
        .map(|h| h.slug.clone())
        .collect::<Vec<_>>()
        .into_iter();
    events.map(move |event| match event {
        Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) => {
            let id = slugs.next().map(Into::into).or(id);
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            })
        }
        other => other,
    })
}

/// 목차 링크를 만든다. 문서 안 앵커를 사용하므로 호스트에 파일 열기를 요청하지 않는다.
fn toc_nav_html(tr: &Translator, headings: &[HeadingInfo]) -> String {
    let items: String = headings
        .iter()
        .map(|h| {
            format!(
                r##"<li class="tasty-toc-l{level}"><a href="#{slug}">{text}</a></li>"##,
                level = h.level as u8,
                slug = attr_escape(&h.slug),
                text = html_escape(&h.text),
            )
        })
        .collect();
    format!(
        r#"<nav id="tasty-toc" aria-label="{aria}"><button id="tasty-toc-toggle" type="button" aria-expanded="true">{label}</button><ul id="tasty-toc-list">{items}</ul></nav>"#,
        aria = attr_escape(tr.t("markdown.toc.aria_label")),
        label = html_escape(tr.t("markdown.toc.label")),
        items = items,
    )
}

// ── HTML generation ───────────────────────────────────────────────────────────

/// 기준 폴더 없이 Markdown을 정리 전 HTML로 변환하는 테스트용 함수.
#[cfg(test)]
fn unsafe_content_html(source: &str, tr: &Translator) -> String {
    unsafe_content_html_in_dir(source, tr, None)
}

/// Markdown을 정리 전 HTML로 바꾼다. 코드·각주·콜아웃을 변환한 뒤
/// 독립 이미지, 위키링크와 URL을 처리하고 링크 마커 및 제목 ID를 넣는다.
/// 링크를 추가하는 처리는 rewrite_link_event보다 먼저 실행해야 한다.
fn unsafe_content_html_in_dir(source: &str, tr: &Translator, base_dir: Option<&Path>) -> String {
    let headings = collect_headings(source);
    let footnote_ref_totals = footnote_reference_totals(source);
    let mut footnote_state = FootnoteState::default();
    let events: Vec<Event> = Parser::new_ext(source, parser_options())
        .map(rewrite_code_block_event)
        .map(|event| rewrite_footnote_event(event, tr, &footnote_ref_totals, &mut footnote_state))
        .collect();
    let events = rewrite_callout_events(events, tr);
    let events = figurize_solo_image_paragraphs(events);
    let events = resolve_wikilinks(events, base_dir);
    let events = autolink_bare_urls(events)
        .into_iter()
        .map(rewrite_link_event);
    let events = assign_heading_ids(events, &headings);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html
}

// ── image captions (solo-image paragraphs → <figure>/<figcaption>) ─────────────

/// 문단 전체가 alt가 있는 이미지 하나일 때 figure와 figcaption으로 바꾼다.
/// 본문이나 다른 이미지, 링크가 섞여 있으면 그대로 둔다. p 안에 figure를 넣지
/// 않도록 이미지뿐 아니라 문단 전체를 바꾼다.
fn figurize_solo_image_paragraphs(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    let mut iter = events.into_iter();
    while let Some(event) = iter.next() {
        match event {
            Event::Start(Tag::Paragraph) => {
                let mut buf = Vec::new();
                for inner in iter.by_ref() {
                    if matches!(inner, Event::End(TagEnd::Paragraph)) {
                        break;
                    }
                    buf.push(inner);
                }
                out.extend(figurize_paragraph_buffer(buf));
            }
            other => out.push(other),
        }
    }
    out
}

fn wrap_as_paragraph(buf: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(buf.len() + 2);
    out.push(Event::Start(Tag::Paragraph));
    out.extend(buf);
    out.push(Event::End(TagEnd::Paragraph));
    out
}

/// One buffered paragraph's events → either the `<figure>` promotion or the original paragraph
/// passed through unchanged.
fn figurize_paragraph_buffer(buf: Vec<Event<'_>>) -> Vec<Event<'_>> {
    // Find at most one top-level `Tag::Image` span, matching its end via nest-counting — the
    // same generic Start/End balancing algorithm pulldown-cmark's own `html.rs::raw_text()` uses
    // to build the `alt` attribute, so this locates exactly the span that writer will consume.
    let mut image_span: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < buf.len() {
        if matches!(buf[i], Event::Start(Tag::Image { .. })) {
            if image_span.is_some() {
                return wrap_as_paragraph(buf); // more than one image in this paragraph — bail.
            }
            let mut nest = 0i32;
            let mut end_idx = None;
            let mut j = i + 1;
            while j < buf.len() {
                match &buf[j] {
                    Event::Start(_) => nest += 1,
                    Event::End(TagEnd::Image) if nest == 0 => {
                        end_idx = Some(j);
                        break;
                    }
                    Event::End(_) => nest -= 1,
                    _ => {}
                }
                j += 1;
            }
            let Some(end_idx) = end_idx else {
                return wrap_as_paragraph(buf); // unmatched — defensive, shouldn't happen.
            };
            image_span = Some((i, end_idx));
            i = end_idx;
        }
        i += 1;
    }
    let Some((start_idx, end_idx)) = image_span else {
        return wrap_as_paragraph(buf); // no image at all — an ordinary text paragraph.
    };

    // Everything outside the image span must be whitespace-only text — any other inline content
    // (more text, a link wrapping the image, a second image, …) disqualifies promotion.
    let outside_is_blank = buf.iter().enumerate().all(|(idx, ev)| {
        (start_idx..=end_idx).contains(&idx) || matches!(ev, Event::Text(t) if t.trim().is_empty())
    });
    if !outside_is_blank {
        return wrap_as_paragraph(buf);
    }

    // Alt text: markup-stripped concatenation of Text/Code inside the image span — same "keep
    // the text, drop the markup" collection [`collect_headings`] uses for heading text.
    let mut alt = String::new();
    for ev in &buf[start_idx + 1..end_idx] {
        match ev {
            Event::Text(t) | Event::Code(t) => alt.push_str(t),
            _ => {}
        }
    }
    if alt.trim().is_empty() {
        return wrap_as_paragraph(buf); // no alt text — nothing to caption, leave it as-is.
    }

    let mut buf = buf;
    let image_events: Vec<Event<'_>> = buf.drain(start_idx..=end_idx).collect();
    let mut out = Vec::with_capacity(image_events.len() + 4);
    out.push(Event::Html("<figure>".into()));
    out.extend(image_events);
    out.push(Event::Html("<figcaption>".into()));
    out.push(Event::Text(alt.into()));
    out.push(Event::Html("</figcaption></figure>".into()));
    out
}

// ── Bare `http(s)://` autolinking ───────────────────────────────────────────────

/// 검색 결과 배경의 투명도. 테마 색에 적용한다.
const FIND_HIT_BG_ALPHA: u8 = 90;

/// diff 추가/삭제 줄 배경의 알파 — `gamma_multiply(0.12)` 과 같은 비율을
/// 알파 공간으로 옮긴 값이다. 대응 토큰 없음.
const DIFF_LINE_BG_ALPHA: u8 = 31;

const AUTOLINK_SCHEMES: &[&str] = &["https://", "http://"];

/// Trailing characters stripped one at a time from the end of a matched URL run — mirrors GFM's
/// extended-autolink "trailing punctuation" rule so a URL at the end of a sentence/quote doesn't
/// swallow the punctuation with it (`https://example.com.` → link stops before the `.`). `)` is
/// handled separately below via paren-balance, not blanket-stripped, since a balanced trailing
/// `)` (e.g. a wiki URL) is legitimately part of the URL.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"', '*', '_', '~'];

/// 일반 텍스트의 URL을 링크 이벤트로 바꾼다. 기존 링크와 코드 블록은 제외한다.
/// 이어지는 Text 이벤트를 모아 URL이 이벤트 경계에서 잘리지 않게 한다.
fn autolink_bare_urls(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    let mut link_depth: u32 = 0;
    let mut code_block_depth: u32 = 0;
    let mut run = String::new();

    for event in events {
        match event {
            Event::Text(ref text) if link_depth == 0 && code_block_depth == 0 => {
                run.push_str(text);
            }
            other => {
                if !run.is_empty() {
                    out.extend(split_bare_urls(std::mem::take(&mut run)));
                }
                match &other {
                    Event::Start(Tag::Link { .. }) => link_depth += 1,
                    Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
                    Event::Start(Tag::CodeBlock(_)) => code_block_depth += 1,
                    Event::End(TagEnd::CodeBlock) => {
                        code_block_depth = code_block_depth.saturating_sub(1)
                    }
                    _ => {}
                }
                out.push(other);
            }
        }
    }
    if !run.is_empty() {
        out.extend(split_bare_urls(run));
    }
    out
}

/// 텍스트에서 URL을 찾아 앞뒤 일반 텍스트와 링크 이벤트로 나눈다.
fn split_bare_urls(text: String) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    let mut plain_start = 0usize;
    let mut search_from = 0usize;

    while let Some(scheme_start) = find_scheme_start(&text[search_from..]).map(|i| i + search_from)
    {
        let url_end = scan_url_end(&text, scheme_start);
        if url_end <= scheme_start {
            // Defensive: a recognized scheme with nothing usable after it (shouldn't happen,
            // the scheme string itself is always non-whitespace). Skip past it and keep scanning.
            search_from = scheme_start + 1;
            continue;
        }

        if scheme_start > plain_start {
            out.push(Event::Text(
                text[plain_start..scheme_start].to_string().into(),
            ));
        }
        let url = text[scheme_start..url_end].to_string();
        out.push(Event::Start(Tag::Link {
            link_type: LinkType::Autolink,
            dest_url: CowStr::from(url.clone()),
            title: CowStr::from(""),
            id: CowStr::from(""),
        }));
        out.push(Event::Text(CowStr::from(url)));
        out.push(Event::End(TagEnd::Link));

        plain_start = url_end;
        search_from = url_end;
    }

    if plain_start < text.len() {
        out.push(Event::Text(text[plain_start..].to_string().into()));
    } else if out.is_empty() {
        // No URL found at all — return the run untouched as a single Text event rather than
        // an empty Vec, so callers never lose content.
        out.push(Event::Text(text.into()));
    }
    out
}

/// 지원하는 scheme 중 가장 먼저 나타나는 위치를 찾는다.
fn find_scheme_start(text: &str) -> Option<usize> {
    AUTOLINK_SCHEMES
        .iter()
        .filter_map(|scheme| text.find(scheme))
        .min()
}

/// 공백 또는 링크를 닫는 문자를 만나면 URL을 끝낸다.
/// 끝의 문장부호도 제외하되 URL 안의 괄호 짝은 유지한다.
fn scan_url_end(text: &str, scheme_start: usize) -> usize {
    let rest = &text[scheme_start..];
    let mut end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || *c == '<' || *c == '>')
        .map(|(i, _)| i)
        .unwrap_or(rest.len());

    loop {
        let candidate = &rest[..end];
        let Some(last) = candidate.chars().last() else {
            break;
        };
        if TRAILING_PUNCTUATION.contains(&last) {
            end -= last.len_utf8();
            continue;
        }
        if last == ')' {
            let opens = candidate.matches('(').count();
            let closes = candidate.matches(')').count();
            if closes > opens {
                end -= 1; // ')' is a single ASCII byte
                continue;
            }
        }
        break;
    }
    scheme_start + end
}

// ── Obsidian-style wikilinks (`[[문서명]]` / `[[문서명|표시텍스트]]`) ────────────

/// 위키링크 대상과 표시할 이름.
struct Wikilink {
    /// The `.md`-less document name as written inside `[[...]]`, already validated to contain
    /// neither `/`, `\`, nor `..` (see [`parse_wikilink_body`]).
    name: String,
    /// Link text: the `|`-delimited display text if present and non-blank, otherwise `name`.
    display: String,
}

/// 위키링크 안의 대상과 선택적 표시 이름을 해석한다.
fn parse_wikilink_body(body: &str) -> Option<Wikilink> {
    if body.contains("[[") {
        return None;
    }
    let (name_part, display_part) = match body.split_once('|') {
        Some((n, d)) => (n, Some(d)),
        None => (body, None),
    };
    let name = name_part.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return None;
    }
    let display = display_part
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(name);
    Some(Wikilink {
        name: name.to_string(),
        display: display.to_string(),
    })
}

/// 같은 디렉터리의 문서를 가리키는 링크를 만든다. 대상이 없거나 기준 폴더를
/// 모르면 링크를 유지하면서 tasty-wikilink-missing 클래스로 감싼다.
fn wikilink_events(link: Wikilink, base_dir: Option<&Path>) -> Vec<Event<'static>> {
    let dest = format!("{}.md", link.name);
    let exists = base_dir.is_some_and(|dir| dir.join(&dest).exists());

    let link_events = [
        Event::Start(Tag::Link {
            link_type: LinkType::Shortcut,
            dest_url: CowStr::from(dest),
            title: CowStr::from(""),
            id: CowStr::from(""),
        }),
        Event::Text(link.display.into()),
        Event::End(TagEnd::Link),
    ];

    if exists {
        link_events.to_vec()
    } else {
        let mut out = vec![Event::Html(CowStr::from(
            r#"<span class="tasty-wikilink-missing">"#,
        ))];
        out.extend(link_events);
        out.push(Event::Html(CowStr::from("</span>")));
        out
    }
}

/// 텍스트에서 위키링크 구문을 찾아 링크 이벤트로 바꾼다.
fn split_wikilinks(text: String, base_dir: Option<&Path>) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    let mut plain_start = 0usize;
    let mut search_from = 0usize;

    while let Some(rel_open) = text[search_from..].find("[[") {
        let open = search_from + rel_open;
        let Some(rel_close) = text[open + 2..].find("]]") else {
            break;
        };
        let close = open + 2 + rel_close;
        let body = &text[open + 2..close];

        let Some(link) = parse_wikilink_body(body) else {
            // Not a valid wikilink body — leave this `[[` untouched and resume right after it,
            // so a `]]` later on the line can still pair with a *subsequent* `[[`.
            search_from = open + 2;
            continue;
        };

        if open > plain_start {
            out.push(Event::Text(text[plain_start..open].to_string().into()));
        }
        out.extend(wikilink_events(link, base_dir));
        plain_start = close + 2;
        search_from = plain_start;
    }

    if plain_start < text.len() {
        out.push(Event::Text(text[plain_start..].to_string().into()));
    } else if out.is_empty() {
        out.push(Event::Text(text.into()));
    }
    out
}

/// 위키링크 구문이 담긴 Text 이벤트를 모아 링크로 바꾼다.
/// 기존 링크와 코드 블록 안의 텍스트는 제외한다.
fn resolve_wikilinks<'a>(events: Vec<Event<'a>>, base_dir: Option<&Path>) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len());
    let mut link_depth: u32 = 0;
    let mut code_block_depth: u32 = 0;
    let mut run = String::new();

    for event in events {
        match event {
            Event::Text(ref text) if link_depth == 0 && code_block_depth == 0 => {
                run.push_str(text);
            }
            other => {
                if !run.is_empty() {
                    out.extend(split_wikilinks(std::mem::take(&mut run), base_dir));
                }
                match &other {
                    Event::Start(Tag::Link { .. }) => link_depth += 1,
                    Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
                    Event::Start(Tag::CodeBlock(_)) => code_block_depth += 1,
                    Event::End(TagEnd::CodeBlock) => {
                        code_block_depth = code_block_depth.saturating_sub(1)
                    }
                    _ => {}
                }
                out.push(other);
            }
        }
    }
    if !run.is_empty() {
        out.extend(split_wikilinks(run, base_dir));
    }
    out
}

fn rewrite_link_event(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: rewrite_link_dest(&dest_url).into(),
            title,
            id,
        }),
        other => other,
    }
}

/// 문서 안 앵커는 유지하고 나머지 Markdown 링크를 내부 이동 마커로 바꾼다.
fn rewrite_link_dest(dest: &str) -> String {
    let trimmed = dest.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return dest.to_string();
    }
    format!(
        "#{NAV_FRAGMENT_MARKER}link:{}",
        percent_encode_fragment(trimmed)
    )
}

/// 코드 펜스의 첫 언어 이름만 남기고 허용 문자로 제한한다.
fn rewrite_code_block_event(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => Event::Start(Tag::CodeBlock(
            CodeBlockKind::Fenced(sanitize_fence_lang(&info).into()),
        )),
        other => other,
    }
}

/// Keep only `[A-Za-z0-9_+-]` from the fence info string's first (language) token, capped at
/// 32 chars. Empty input/output means no class is emitted at all (matches pulldown-cmark's own
/// `lang.is_empty()` branch in `CodeBlockKind::Fenced` handling).
fn sanitize_fence_lang(info: &str) -> String {
    info.split(' ')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '_'))
        .take(32)
        .collect()
}

// ── callouts (GFM `> [!NOTE]` alerts + Obsidian-style `> [!type]+ Title`) ──────

/// 콜아웃 한 종류의 표시 이름, CSS 클래스와 아이콘.
struct CalloutKind {
    /// 파서가 구분한 GFM 콜아웃 종류. 확장 종류는 None이다.
    gfm_kind: Option<BlockQuoteKind>,
    /// Lowercase Obsidian tag text this entry answers to (e.g. `"note"`, `"info"`) — matched
    /// case-insensitively against the `[!type]` token via [`find_callout_kind`]. For the 5 GFM
    /// kinds this is simply their lowercase name, so a bare `[!note]` and an Obsidian-flavored
    /// `[!note]+ Title` resolve to the same entry either way.
    type_key: &'static str,
    /// The literal class this kind renders as (mirrors pulldown-cmark's own `html.rs` naming
    /// for the 5 GFM kinds, kept identical so existing CSS/snapshots don't need to change).
    class: &'static str,
    /// `Translator` key for the default header label (used whenever no custom title follows the
    /// tag) — must exist in `lang/{en,ko,ja}.toml`.
    label_key: &'static str,
    /// Inner markup of a [`tasty_icons`] glyph (`Icon::body` — no wrapping `<svg>`, no color
    /// baked in). Fed to [`alert_icon_data_uri`].
    icon_body: &'static str,
    /// The glyph's own `Icon::filled` — `true` colors it via `fill`, `false` via `stroke`
    /// (mirrors how `tasty_icons`' `stroke_icon!`/`fill_icon!` macros built it).
    icon_filled: bool,
    /// 테마에서 콜아웃 강조색을 가져온다.
    accent: fn(&Theme) -> tasty_type_appearance::color::HexColor,
}

/// 지원하는 콜아웃 종류. 별칭은 CALLOUT_ALIASES에서 정규 이름에 연결한다.
const CALLOUT_KINDS: &[CalloutKind] = &[
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Note),
        type_key: "note",
        class: "markdown-alert-note",
        label_key: "markdown.alert.note",
        icon_body: tasty_icons::ALERT_CIRCLE.body,
        icon_filled: tasty_icons::ALERT_CIRCLE.filled,
        accent: Theme::accent_primary,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Tip),
        type_key: "tip",
        class: "markdown-alert-tip",
        label_key: "markdown.alert.tip",
        icon_body: tasty_icons::STAR_FILL.body,
        icon_filled: tasty_icons::STAR_FILL.filled,
        accent: Theme::accent_success,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Important),
        type_key: "important",
        class: "markdown-alert-important",
        label_key: "markdown.alert.important",
        icon_body: tasty_icons::BELL.body,
        icon_filled: tasty_icons::BELL.filled,
        accent: Theme::accent_agent,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Warning),
        type_key: "warning",
        class: "markdown-alert-warning",
        label_key: "markdown.alert.warning",
        icon_body: tasty_icons::ALERT_TRIANGLE.body,
        icon_filled: tasty_icons::ALERT_TRIANGLE.filled,
        accent: Theme::accent_warning,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Caution),
        type_key: "caution",
        class: "markdown-alert-caution",
        label_key: "markdown.alert.caution",
        icon_body: tasty_icons::CLOSE.body,
        icon_filled: tasty_icons::CLOSE.filled,
        accent: Theme::accent_danger,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "abstract",
        class: "markdown-alert-abstract",
        label_key: "markdown.alert.abstract",
        icon_body: tasty_icons::LIST.body,
        icon_filled: tasty_icons::LIST.filled,
        accent: Theme::accent_primary,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "info",
        class: "markdown-alert-info",
        label_key: "markdown.alert.info",
        icon_body: tasty_icons::ALERT_CIRCLE.body,
        icon_filled: tasty_icons::ALERT_CIRCLE.filled,
        accent: Theme::accent_info,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "todo",
        class: "markdown-alert-todo",
        label_key: "markdown.alert.todo",
        icon_body: tasty_icons::CHECK.body,
        icon_filled: tasty_icons::CHECK.filled,
        accent: Theme::accent_info,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "success",
        class: "markdown-alert-success",
        label_key: "markdown.alert.success",
        icon_body: tasty_icons::CHECK.body,
        icon_filled: tasty_icons::CHECK.filled,
        accent: Theme::accent_success,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "question",
        class: "markdown-alert-question",
        label_key: "markdown.alert.question",
        icon_body: tasty_icons::HELP_CIRCLE.body,
        icon_filled: tasty_icons::HELP_CIRCLE.filled,
        accent: Theme::accent_attention,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "failure",
        class: "markdown-alert-failure",
        label_key: "markdown.alert.failure",
        icon_body: tasty_icons::ALERT_TRIANGLE.body,
        icon_filled: tasty_icons::ALERT_TRIANGLE.filled,
        accent: Theme::accent_danger,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "danger",
        class: "markdown-alert-danger",
        label_key: "markdown.alert.danger",
        icon_body: tasty_icons::CLOSE.body,
        icon_filled: tasty_icons::CLOSE.filled,
        accent: Theme::accent_danger,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "bug",
        class: "markdown-alert-bug",
        label_key: "markdown.alert.bug",
        icon_body: tasty_icons::CLOSE.body,
        icon_filled: tasty_icons::CLOSE.filled,
        accent: Theme::accent_agent,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "example",
        class: "markdown-alert-example",
        label_key: "markdown.alert.example",
        icon_body: tasty_icons::SCRIPT.body,
        icon_filled: tasty_icons::SCRIPT.filled,
        accent: Theme::accent_agent,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "quote",
        class: "markdown-alert-quote",
        label_key: "markdown.alert.quote",
        icon_body: tasty_icons::TEXT_LEFT.body,
        icon_filled: tasty_icons::TEXT_LEFT.filled,
        accent: Theme::accent_primary,
    },
];

/// 콜아웃 별칭과 정규 이름.
const CALLOUT_ALIASES: &[(&str, &str)] = &[
    ("summary", "abstract"),
    ("tldr", "abstract"),
    ("hint", "tip"),
    ("check", "success"),
    ("done", "success"),
    ("help", "question"),
    ("faq", "question"),
    ("fail", "failure"),
    ("missing", "failure"),
    ("error", "danger"),
    ("cite", "quote"),
];

/// 소문자로 정규화된 이름을 정규 이름 및 별칭과 비교한다.
fn find_callout_kind(type_key: &str) -> Option<&'static CalloutKind> {
    if let Some(found) = CALLOUT_KINDS.iter().find(|k| k.type_key == type_key) {
        return Some(found);
    }
    let canonical = CALLOUT_ALIASES
        .iter()
        .find(|(alias, _)| *alias == type_key)?
        .1;
    CALLOUT_KINDS.iter().find(|k| k.type_key == canonical)
}

/// One parsed `[!type]([+-])?( title)?` tag line — the shape [`parse_callout_tag_line`] extracts
/// from a plain blockquote's first line of text.
struct ParsedCalloutTag {
    /// Lowercased `[!type]` token, looked up via [`find_callout_kind`].
    type_key: String,
    /// `Some(true)` = `+` (initially expanded), `Some(false)` = `-` (initially collapsed),
    /// `None` = no fold marker at all (no `<details>` — matches scope: "마커 없음 = 접기 UI
    /// 자체 없음").
    fold: Option<bool>,
    /// Text following the tag (and fold marker, if any) on the same line, trimmed — `None` if
    /// empty (fold and title are independent per Obsidian's own docs: a title can appear with or
    /// without a fold marker).
    title: Option<String>,
}

/// [!type] 태그와 선택적 접기 표시 및 제목을 읽는다.
fn parse_callout_tag_line(text: &str) -> Option<ParsedCalloutTag> {
    let rest = text.strip_prefix("[!")?;
    let close = rest.find(']')?;
    let type_token = &rest[..close];
    if type_token.is_empty()
        || !type_token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let mut after = &rest[close + 1..];
    let fold = match after.chars().next() {
        Some('+') => {
            after = &after[1..];
            Some(true)
        }
        Some('-') => {
            after = &after[1..];
            Some(false)
        }
        _ => None,
    };
    let title = after.trim();
    Some(ParsedCalloutTag {
        type_key: type_token.to_ascii_lowercase(),
        fold,
        title: (!title.is_empty()).then(|| title.to_string()),
    })
}

/// 인용문의 첫 문단에서 콜아웃 태그를 읽어 HTML로 바꾼다.
/// 알 수 없는 종류는 원래 인용문으로 남긴다. 이 처리는 파서의 BlockQuote 이벤트에
/// 적용하며, raw HTML에 같은 class나 data-label이 있는지까지 막지는 않는다.
fn rewrite_callout_events<'a>(events: Vec<Event<'a>>, tr: &Translator) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len());
    let mut iter = events.into_iter();
    while let Some(event) = iter.next() {
        match event {
            Event::Start(Tag::BlockQuote(kind)) => {
                let mut buf = Vec::new();
                let mut nest = 0i32;
                for inner in iter.by_ref() {
                    match &inner {
                        Event::Start(Tag::BlockQuote(_)) => {
                            nest += 1;
                            buf.push(inner);
                        }
                        Event::End(TagEnd::BlockQuote(_)) => {
                            if nest == 0 {
                                break;
                            }
                            nest -= 1;
                            buf.push(inner);
                        }
                        _ => buf.push(inner),
                    }
                }
                out.extend(rewrite_callout_buffer(kind, buf, tr));
            }
            other => out.push(other),
        }
    }
    out
}

/// One buffered blockquote's interior events (stripped of its own `Start`/`End` by
/// [`rewrite_callout_events`]) → either a rendered callout or the original blockquote passed
/// through unchanged. See [`rewrite_callout_events`] doc for the two input shapes this handles.
fn rewrite_callout_buffer<'a>(
    gfm_kind: Option<BlockQuoteKind>,
    buf: Vec<Event<'a>>,
    tr: &Translator,
) -> Vec<Event<'a>> {
    // 중첩 인용문에도 같은 변환을 적용한다.
    let buf = rewrite_callout_events(buf, tr);

    if let Some(kind) = gfm_kind {
        let Some(callout) = CALLOUT_KINDS.iter().find(|k| k.gfm_kind == Some(kind)) else {
            // All 5 GFM `BlockQuoteKind` variants are covered in `CALLOUT_KINDS`; unreachable in
            // practice, but fall back to a plain blockquote rather than panicking if
            // pulldown-cmark ever adds a 6th kind before this table does.
            return wrap_plain_blockquote(Some(kind), buf);
        };
        return wrap_static_callout(callout.class, tr.t(callout.label_key), buf);
    }

    // 첫 문단의 이어지는 텍스트에서 콜아웃 태그를 읽는다.
    let Some(Event::Start(Tag::Paragraph)) = buf.first() else {
        return wrap_plain_blockquote(None, buf);
    };
    let mut first_line = String::new();
    let mut text_run_end = 1;
    while let Some(Event::Text(t)) = buf.get(text_run_end) {
        first_line.push_str(t);
        text_run_end += 1;
    }
    if text_run_end == 1 {
        return wrap_plain_blockquote(None, buf); // no leading text at all — nothing to parse.
    }
    let Some(parsed) = parse_callout_tag_line(&first_line) else {
        return wrap_plain_blockquote(None, buf);
    };
    let Some(callout) = find_callout_kind(&parsed.type_key) else {
        return wrap_plain_blockquote(None, buf);
    };

    let label = parsed
        .title
        .clone()
        .unwrap_or_else(|| tr.t(callout.label_key).to_string());

    // 태그를 제거한 뒤 첫 문단이 비면 문단도 제거한다.
    let mut rest: Vec<Event<'a>> = buf[text_run_end..].to_vec();
    let body: Vec<Event<'a>> = if matches!(rest.first(), Some(Event::End(TagEnd::Paragraph))) {
        rest.remove(0);
        rest
    } else {
        let mut body = vec![Event::Start(Tag::Paragraph)];
        body.append(&mut rest);
        body
    };

    match parsed.fold {
        None => wrap_static_callout(callout.class, &label, body),
        Some(open) => wrap_foldable_callout(callout.class, &label, open, body),
    }
}

/// Re-wraps buffered interior events back into an ordinary, untouched
/// `<blockquote>...</blockquote>` — used by every [`rewrite_callout_buffer`] path that declines
/// to treat the blockquote as a callout.
fn wrap_plain_blockquote(kind: Option<BlockQuoteKind>, buf: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(buf.len() + 2);
    out.push(Event::Start(Tag::BlockQuote(kind)));
    out.extend(buf);
    out.push(Event::End(TagEnd::BlockQuote(kind)));
    out
}

/// 접지 않는 콜아웃에 표시 이름과 CSS 클래스를 붙인다.
fn wrap_static_callout<'a>(class: &str, label: &str, body: Vec<Event<'a>>) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(body.len() + 2);
    out.push(Event::Html(
        format!(
            r#"<blockquote class="{class}" data-label="{}">"#,
            attr_escape(label)
        )
        .into(),
    ));
    out.extend(body);
    out.push(Event::Html("</blockquote>".into()));
    out
}

/// 접을 수 있는 콜아웃을 details와 summary로 감싼다.
fn wrap_foldable_callout<'a>(
    class: &str,
    label: &str,
    open: bool,
    body: Vec<Event<'a>>,
) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(Event::Html(
        format!(
            r#"<details class="{class}"{}>"#,
            if open { " open" } else { "" }
        )
        .into(),
    ));
    out.push(Event::Html("<summary>".into()));
    out.push(Event::Text(label.to_string().into()));
    out.push(Event::Html("</summary>".into()));
    out.extend(body);
    out.push(Event::Html("</details>".into()));
    out
}

// ── footnote backlinks + a11y (`[^name]` / `[^name]: ...`) ─────────────────────

/// 각주 정의와 참조 번호를 처리하는 상태.
#[derive(Default)]
struct FootnoteState {
    /// 각주 이름별 표시 번호. 참조나 정의 중 처음 나타난 순서로 부여한다.
    numbers: std::collections::HashMap<String, usize>,
    /// `name -> how many `FootnoteReference` events for this name have been rewritten so far`
    /// — drives the `fnref-<name>`/`fnref-<name>-2`/... suffix so multiple references to the
    /// same footnote get distinct, individually-targetable ids.
    seen_ref_counts: std::collections::HashMap<String, usize>,
    /// 현재 출력 중인 각주 정의. 끝날 때 되돌아가기 링크를 넣는다.
    open_definition: Option<String>,
}

fn footnote_number(state: &mut FootnoteState, name: &str) -> usize {
    let next = state.numbers.len() + 1;
    *state.numbers.entry(name.to_string()).or_insert(next)
}

/// 각주별 참조 횟수를 먼저 세어 정의 끝에 필요한 되돌아가기 링크 수를 구한다.
fn footnote_reference_totals(source: &str) -> std::collections::HashMap<String, usize> {
    let mut totals = std::collections::HashMap::new();
    for event in Parser::new_ext(source, parser_options()) {
        if let Event::FootnoteReference(name) = event {
            *totals.entry(name.to_string()).or_insert(0) += 1;
        }
    }
    totals
}

/// 각주 참조와 정의를 번호 및 되돌아가기 링크가 있는 HTML로 바꾼다.
fn rewrite_footnote_event<'a>(
    event: Event<'a>,
    tr: &Translator,
    ref_totals: &std::collections::HashMap<String, usize>,
    state: &mut FootnoteState,
) -> Event<'a> {
    match event {
        Event::FootnoteReference(name) => {
            let name = name.to_string();
            let number = footnote_number(state, &name);
            let occurrence = {
                let count = state.seen_ref_counts.entry(name.clone()).or_insert(0);
                *count += 1;
                *count
            };
            let safe = percent_encode_fragment(&name);
            let ref_id = if occurrence == 1 {
                format!("fnref-{safe}")
            } else {
                format!("fnref-{safe}-{occurrence}")
            };
            let aria = attr_escape(&tr.t_fmt("markdown.footnote.ref_aria", &number.to_string()));
            Event::Html(
                format!(
                    r##"<sup class="footnote-reference" id="{ref_id}"><a href="#fndef-{safe}" aria-label="{aria}">{number}</a></sup>"##
                )
                .into(),
            )
        }
        Event::Start(Tag::FootnoteDefinition(name)) => {
            let name = name.to_string();
            let number = footnote_number(state, &name);
            let safe = percent_encode_fragment(&name);
            state.open_definition = Some(name);
            Event::Html(
                format!(
                    r#"<div class="footnote-definition" id="fndef-{safe}"><sup class="footnote-definition-label">{number}</sup>"#
                )
                .into(),
            )
        }
        Event::End(TagEnd::FootnoteDefinition) => {
            let name = state
                .open_definition
                .take()
                .unwrap_or_else(|| String::from("unknown"));
            let number = footnote_number(state, &name);
            let safe = percent_encode_fragment(&name);
            let total = *ref_totals.get(&name).unwrap_or(&0);
            let mut backlinks = String::new();
            for occurrence in 1..=total {
                let ref_id = if occurrence == 1 {
                    format!("fnref-{safe}")
                } else {
                    format!("fnref-{safe}-{occurrence}")
                };
                let aria = if total > 1 {
                    tr.t("markdown.footnote.backlink_aria_nth")
                        .replace("{0}", &number.to_string())
                        .replace("{1}", &occurrence.to_string())
                } else {
                    tr.t_fmt("markdown.footnote.backlink_aria", &number.to_string())
                };
                backlinks.push_str(&format!(
                    r##"<a href="#{ref_id}" aria-label="{}">↩</a>"##,
                    attr_escape(&aria)
                ));
            }
            Event::Html(format!("{backlinks}</div>\n").into())
        }
        other => other,
    }
}

/// 본문 HTML에서 허용한 태그와 속성만 남긴다. script, 인라인 이벤트 처리기와
/// javascript URL은 제거한다. class 값 자체는 검사하지 않으며 raw HTML에도
/// 허용 목록이 그대로 적용된다. 따라서 문서 작성자가 콜아웃 class나 data-label을
/// 직접 넣을 수 있다. 이 속성은 콜아웃 변환에서만 생성된다는 보장은 없다.
fn sanitize_html(unsafe_html: &str) -> String {
    use ammonia::Builder;
    use std::collections::{HashMap, HashSet};

    let tags: HashSet<&str> = [
        "p",
        "br",
        "hr",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "strong",
        "em",
        "del",
        "s",
        "code",
        "pre",
        "blockquote",
        "ul",
        "ol",
        "li",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
        "a",
        "img",
        "input",
        "sup",
        "sub",
        "dl",
        "dt",
        "dd",
        "div",
        // figurize_solo_image_paragraphs()의 <figure>/<figcaption> 래핑 — 둘 다 별도
        // attribute 없이 bare tag 로만 쓴다(tag_attributes 등록 불필요).
        "figure",
        "figcaption",
        // wrap_foldable_callout() 의 접기 콜아웃(+/- 마커) — details 는 class+open, summary 는
        // 별도 attribute 없이 label 텍스트만 담는다(tag_attributes 등록 불필요).
        "details",
        "summary",
        // pulldown-cmark 의 ENABLE_MATH 가 InlineMath/DisplayMath 이벤트를 기본으로
        // `<span class="math math-inline|math-display">{escaped latex}</span>` 로 쓴다(자체
        // Rust 쪽 이벤트 rewrite 불필요 — 라이브러리 기본 동작 그대로 통과시킨다). 아래
        // `span`+`class` 화이트리스트 근거는 `tag_attributes` 주석 참조.
        "span",
    ]
    .into_iter()
    .collect();

    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    // `aria-label` scoped to `a` only (not `generic_attributes`, which every tag would then
    // get) — the two consumers are [`rewrite_footnote_event`]'s reference/backlink anchors,
    // both `attr_escape`d before injection.
    tag_attributes.insert(
        "a",
        ["href", "title", "id", "aria-label"].into_iter().collect(),
    );
    tag_attributes.insert("img", ["src", "alt", "title"].into_iter().collect());
    tag_attributes.insert(
        "input",
        ["type", "checked", "disabled"].into_iter().collect(),
    );
    tag_attributes.insert("th", ["align"].into_iter().collect());
    tag_attributes.insert("td", ["align"].into_iter().collect());
    tag_attributes.insert("code", ["class"].into_iter().collect());
    tag_attributes.insert("sup", ["class"].into_iter().collect());
    tag_attributes.insert("div", ["class"].into_iter().collect());
    tag_attributes.insert("blockquote", ["class", "data-label"].into_iter().collect());
    tag_attributes.insert("details", ["class", "open"].into_iter().collect());
    // 수식 span의 class를 남겨 KaTeX 스크립트가 찾을 수 있게 한다.
    tag_attributes.insert("span", ["class"].into_iter().collect());

    Builder::default()
        .tags(tags)
        .tag_attributes(tag_attributes)
        .generic_attributes(["id"].into_iter().collect())
        // 내부 nav fragment(`#tasty-nav:...`)는 scheme 이 없어 이 allowlist 와 무관하게
        // 항상 통과한다 — 여기서 막는 건 `javascript:`/기타 스킴을 가진 `href`/`src` 뿐.
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .clean(unsafe_html)
        .to_string()
}

// ── local image inlining (sanitize 뒤, 문서 디렉토리 트리로 범위를 좁혀서) ──────

/// 로컬 이미지를 읽기 전에 검사하는 파일별 크기 기준.
/// metadata 조회 뒤 파일이 커지는 경우까지 제한하지는 않는다.
const MAX_INLINE_IMAGE_BYTES: u64 = 4 * 1024 * 1024;

/// 한 문서에서 인라인하는 이미지 바이트 총합 상한.
const MAX_INLINE_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

/// 확장자로 고르는 이미지 MIME 목록. 파일 내부 형식은 검사하지 않는다.
const INLINE_IMAGE_MIME: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("apng", "image/apng"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
    ("svg", "image/svg+xml"),
];

/// 정리한 HTML의 로컬 이미지를 data URI로 바꾼다. HTTP(S) 이미지는 그대로 둔다.
/// 기준 폴더 안의 정규화된 경로와 허용 확장자만 읽으며, 읽기 전 metadata 길이로
/// 파일별·문서별 기준을 검사한다. 경로 확인과 읽기가 하나의 원자적 연산은 아니다.
/// 읽지 못하는 이미지의 src는 지워 WebView가 로컬 경로를 직접 열지 않게 한다.
fn inline_local_images(html: &str, base_dir: Option<&Path>) -> String {
    let base_canon = base_dir.and_then(|d| std::fs::canonicalize(d).ok());
    let mut budget = MAX_INLINE_TOTAL_BYTES;
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find("<img") {
        let (head, tail) = rest.split_at(pos);
        out.push_str(head);
        // ammonia 는 속성값의 `<`/`>` 를 escape 하므로 태그 끝은 첫 `>` 다.
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let (tag, after) = tail.split_at(end + 1);
        out.push_str(&rewrite_img_tag(tag, base_canon.as_deref(), &mut budget));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// 이미지 태그 하나의 로컬 src를 인라인 처리한다.
fn rewrite_img_tag(tag: &str, base_canon: Option<&Path>, budget: &mut u64) -> String {
    const SRC: &str = " src=\"";
    let Some(rel) = tag.find(SRC) else {
        return tag.to_string();
    };
    let value_start = rel + SRC.len();
    let Some(len) = tag[value_start..].find('"') else {
        return tag.to_string();
    };
    let value = &tag[value_start..value_start + len];
    let raw = html_unescape(value);
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return tag.to_string();
    }
    let replacement = read_inline_image(&raw, base_canon, budget)
        .map(|uri| format!("{SRC}{}\"", attr_escape(&uri)))
        // 읽지 못한 로컬 경로는 src에서 제거한다. 이후 오류 표시 스크립트가 처리한다.
        .unwrap_or_default();
    let mut out = String::with_capacity(tag.len());
    out.push_str(&tag[..rel]);
    out.push_str(&replacement);
    out.push_str(&tag[value_start + len + 1..]);
    out
}

/// `src` 값 하나를 파일로 풀어 `data:` URI 를 만든다. 아래 중 하나라도 걸리면 `None`:
/// base_dir 이 없음 · 경로가 트리 밖 · 파일이 없음 · 확장자가 허용목록 밖 · 상한 초과.
fn read_inline_image(raw: &str, base_canon: Option<&Path>, budget: &mut u64) -> Option<String> {
    let base = base_canon?;
    // 조각(`#`)과 질의(`?`)는 파일 경로의 일부가 아니다.
    let path_part = raw.split(['#', '?']).next().unwrap_or(raw);
    if path_part.is_empty() {
        return None;
    }
    // 마크다운 저자는 공백을 `%20` 으로 적는다 — 파일 이름으로 되돌린다.
    let decoded = percent_decode(path_part);
    let p = Path::new(&decoded);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    };
    let canon = std::fs::canonicalize(&joined).ok()?;
    if !canon.starts_with(base) {
        return None;
    }
    let ext = canon.extension()?.to_str()?.to_ascii_lowercase();
    let mime = INLINE_IMAGE_MIME
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, m)| *m)?;
    let len = std::fs::metadata(&canon).ok()?.len();
    if len > MAX_INLINE_IMAGE_BYTES || len > *budget {
        return None;
    }
    let bytes = std::fs::read(&canon).ok()?;
    *budget = budget.saturating_sub(bytes.len() as u64);
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

/// [`attr_escape`] 의 역 — ammonia 가 속성값에 넣은 엔티티를 되돌린다.
fn html_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

// ── CSS (theme → custom properties) ───────────────────────────────────────────

/// 테마 값을 CSS 변수와 스타일로 변환한다. 제목과 각주의 scroll-margin-top은
/// 고정 주소창 높이를 고려한다. 주소창, 검색창 위치와 앵커 여백은 --md-addr-bar-h를
/// 공유한다. 검색 개수 표시의 min-width:40px는 별도의 너비 값이다.
///
/// html의 height:100%와 body의 min-height:100%는 서로 역할이 다르다.
/// body가 문서만큼 늘어나야 sticky 주소창이 끝까지 남고, html 높이가 정해져야
/// 짧은 문서에서도 body의 백분율 최소 높이가 viewport를 채운다.
fn theme_css(theme: &Theme) -> String {
    let [h1, h2, h3, h4, h5, h6] = heading_sizes_px(theme);
    let body = theme.font_size_body.value();
    format!(
        r#":root{{
--md-fg:{fg};
--md-strong:{strong};
--md-link:{link};
--md-code-bg:{code_bg};
--md-code-border:{code_border};
--md-border:{border};
--md-quote-bar:{quote_bar};
--md-rule:{rule};
--md-zebra:{zebra};
--md-bg:{bg};
--md-radius:{radius}px;
--md-border-w:{border_w}px;
--md-space-xs:{space_xs}px;
--md-space-sm:{space_sm}px;
--md-space-md:{space_md}px;
--md-addr-bar-h:40px;
--md-font-body:{body}px;
--md-h1:{h1}px;--md-h2:{h2}px;--md-h3:{h3}px;--md-h4:{h4}px;--md-h5:{h5}px;--md-h6:{h6}px;
}}
html{{height:100%;margin:0;padding:0;}}
body{{min-height:100%;margin:0;padding:0;background:var(--md-bg);color:var(--md-fg);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;font-size:var(--md-font-body);line-height:1.6;}}
#tasty-addr-bar{{position:sticky;top:0;display:flex;align-items:center;gap:var(--md-space-sm);height:var(--md-addr-bar-h);padding:0 var(--md-space-sm);box-sizing:border-box;background:{bg_sidebar};border-bottom:var(--md-border-w) solid {separator};}}
#tasty-addr-input{{flex:1;height:24px;border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);padding:0 var(--md-space-xs);background:var(--md-bg);color:var(--md-fg);font-size:var(--md-font-body);}}
#tasty-addr-go{{height:24px;padding:0 var(--md-space-sm);border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-code-bg);color:var(--md-fg);cursor:pointer;}}
#tasty-addr-input[readonly]{{color:{muted};}}
#tasty-refresh{{height:24px;padding:0 var(--md-space-sm);border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-code-bg);color:var(--md-fg);cursor:pointer;}}
#tasty-refresh[data-stale="true"]{{background:{accent};border-color:{accent};color:{on_accent};}}
#tasty-find-bar{{position:fixed;top:calc(var(--md-addr-bar-h) + var(--md-space-xs));right:var(--md-space-sm);z-index:20;display:flex;align-items:center;gap:var(--md-space-xs);height:28px;padding:0 var(--md-space-xs);background:{bg_sidebar};border:var(--md-border-w) solid {separator};border-radius:var(--md-radius);box-shadow:0 2px 8px rgba(0,0,0,0.25);}}
#tasty-find-bar[hidden]{{display:none;}}
#tasty-find-input{{width:140px;height:22px;border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);padding:0 var(--md-space-xs);background:var(--md-bg);color:var(--md-fg);font-size:var(--md-font-body);}}
#tasty-find-count{{min-width:40px;text-align:center;font-size:calc(var(--md-font-body) * 0.85);color:{muted};}}
#tasty-find-count.tasty-find-nomatch{{color:{danger};}}
.tasty-find-btn{{height:22px;width:22px;flex-shrink:0;display:inline-flex;align-items:center;justify-content:center;border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-code-bg);color:var(--md-fg);font-size:10px;line-height:1;padding:0;cursor:pointer;}}
.tasty-find-btn:disabled{{opacity:0.45;cursor:default;}}
mark.tasty-find-hit{{background:{find_match_bg};color:inherit;border-radius:2px;}}
mark.tasty-find-hit.tasty-find-current{{background:{find_current_bg};color:{find_current_fg};}}
#tasty-md-body{{padding:var(--md-space-sm) var(--md-space-md);}}
h1,h2,h3,h4,h5,h6{{color:var(--md-strong);font-weight:600;margin:1em 0 0.5em;scroll-margin-top:calc(var(--md-addr-bar-h) + var(--md-space-sm));}}
.footnote-reference,.footnote-definition{{scroll-margin-top:calc(var(--md-addr-bar-h) + var(--md-space-sm));}}
h1{{font-size:var(--md-h1);}}h2{{font-size:var(--md-h2);}}h3{{font-size:var(--md-h3);}}
h4{{font-size:var(--md-h4);}}h5{{font-size:var(--md-h5);}}h6{{font-size:var(--md-h6);}}
#tasty-toc{{margin:var(--md-space-sm) var(--md-space-md) 0;padding:var(--md-space-sm) var(--md-space-md);border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-code-bg);}}
#tasty-toc-toggle{{all:unset;cursor:pointer;display:inline-flex;align-items:center;gap:var(--md-space-xs);font-weight:600;font-size:var(--md-font-body);color:var(--md-strong);}}
#tasty-toc-toggle::before{{content:"\25be";display:inline-block;}}
#tasty-toc.tasty-toc-collapsed #tasty-toc-toggle::before{{content:"\25b8";}}
#tasty-toc-list{{list-style:none;margin:var(--md-space-xs) 0 0;padding:0;max-height:280px;overflow-y:auto;}}
#tasty-toc.tasty-toc-collapsed #tasty-toc-list{{display:none;}}
#tasty-toc-list a{{display:block;padding:var(--md-space-xs) 0;color:var(--md-link);text-decoration:none;font-size:var(--md-font-body);}}
#tasty-toc-list a:hover{{text-decoration:underline;}}
.tasty-toc-l1 a{{padding-left:0;}}
.tasty-toc-l2 a{{padding-left:var(--md-space-sm);}}
.tasty-toc-l3 a{{padding-left:calc(var(--md-space-sm) * 2);}}
.tasty-toc-l4 a{{padding-left:calc(var(--md-space-sm) * 3);}}
.tasty-toc-l5 a{{padding-left:calc(var(--md-space-sm) * 4);}}
.tasty-toc-l6 a{{padding-left:calc(var(--md-space-sm) * 5);}}
a{{color:var(--md-link);}}
.tasty-wikilink-missing a{{color:{danger};text-decoration:underline dotted;}}
strong{{color:var(--md-strong);font-weight:600;}}
code{{background:var(--md-code-bg);border-radius:var(--md-radius);padding:0.1em 0.35em;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;}}
pre{{position:relative;background:var(--md-code-bg);border:var(--md-border-w) solid var(--md-code-border);border-radius:var(--md-radius);padding:var(--md-space-sm);overflow:auto;}}
pre code{{background:none;padding:0;}}
.tasty-copy-btn{{position:absolute;top:var(--md-space-xs);right:var(--md-space-xs);height:22px;padding:0 var(--md-space-xs);border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-bg);color:var(--md-fg);font-size:calc(var(--md-font-body) * 0.8);line-height:1;cursor:pointer;opacity:0;transition:opacity 0.15s ease;}}
pre:hover .tasty-copy-btn,.tasty-copy-btn:focus-visible{{opacity:1;}}
@media (hover:none){{.tasty-copy-btn{{opacity:1;}}}}
.tasty-copy-btn[data-state="copied"]{{border-color:{success};color:{success};}}
.tasty-copy-btn[data-state="failed"]{{border-color:{danger};color:{danger};}}
table{{border-collapse:collapse;}}
th,td{{border:var(--md-border-w) solid var(--md-border);padding:var(--md-space-xs) var(--md-space-sm);text-align:left;}}
tr:nth-child(even){{background:var(--md-zebra);}}
blockquote{{border-left:calc(var(--md-border-w) * 3) solid var(--md-quote-bar);margin:0.5em 0;padding:0.1em var(--md-space-md);opacity:0.9;}}
blockquote[class^="markdown-alert-"]{{opacity:1;border-radius:var(--md-radius);padding:var(--md-space-sm) var(--md-space-md);}}
blockquote[class^="markdown-alert-"]::before{{content:attr(data-label);display:block;font-weight:600;margin-bottom:var(--md-space-xs);padding-left:22px;background-repeat:no-repeat;background-position:left center;background-size:16px 16px;}}
details[class^="markdown-alert-"]{{border-radius:var(--md-radius);padding:var(--md-space-sm) var(--md-space-md);border-left:calc(var(--md-border-w) * 3) solid;}}
details[class^="markdown-alert-"]>summary{{cursor:pointer;font-weight:600;}}
details[class^="markdown-alert-"]>summary::before{{content:"";display:inline-block;width:16px;height:16px;margin-right:6px;vertical-align:middle;background-repeat:no-repeat;background-position:center;background-size:16px 16px;}}
details[class^="markdown-alert-"][open]>summary{{margin-bottom:var(--md-space-xs);}}
{alert_rules}
{hljs_rules}
hr{{border:none;border-top:var(--md-border-w) solid var(--md-rule);margin:var(--md-space-md) 0;}}
img{{max-width:100%;}}
.tasty-img-error{{display:inline-flex;align-items:center;gap:var(--md-space-xs);flex-wrap:wrap;max-width:100%;box-sizing:border-box;padding:var(--md-space-xs) var(--md-space-sm);border:var(--md-border-w) dashed {danger};border-radius:var(--md-radius);background:var(--md-code-bg);}}
.tasty-img-error-icon{{flex:0 0 auto;width:16px;height:16px;background-image:url("{img_error_icon}");background-repeat:no-repeat;background-size:16px 16px;}}
.tasty-img-error-label{{color:{danger};font-weight:600;}}
.tasty-img-error-path{{color:{muted};word-break:break-all;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;}}
figure{{margin:var(--md-space-sm) 0;text-align:center;}}
figcaption{{margin-top:var(--md-space-xs);font-size:var(--md-font-body);color:{muted};}}
ul,ol{{padding-left:1.5em;}}
li input[type=checkbox]{{margin-right:0.4em;}}
.tasty-state{{padding:var(--md-space-md);color:{muted};}}
.tasty-state-title{{font-size:var(--md-font-body);font-weight:600;color:{danger};}}
.tasty-state-detail{{color:{muted};white-space:pre-wrap;}}
"#,
        fg = theme.text_secondary().to_hex(),
        strong = theme.text_primary().to_hex(),
        link = theme.accent_primary().to_hex(),
        code_bg = theme.surface_raised().to_hex(),
        code_border = theme.separator.to_hex(),
        border = theme.md_table_border().to_hex(),
        quote_bar = theme.border_strong().to_hex(),
        rule = theme.separator.to_hex(),
        zebra = theme.md_table_row_bg_zebra().to_hex(),
        // webview 렌더 경로엔 focus 신호가 없다 — surfaces.markdown.focused_bg 대신
        // bg_app(=crust)을 문서의 유일한 배경으로 쓴다.
        bg = theme.bg_app().to_hex(),
        radius = theme.corner_radius.value(),
        border_w = theme.border_width.value(),
        space_xs = theme.spacing_xs.value(),
        space_sm = theme.spacing_sm.value(),
        space_md = theme.spacing_md.value(),
        body = body,
        h1 = h1,
        h2 = h2,
        h3 = h3,
        h4 = h4,
        h5 = h5,
        h6 = h6,
        bg_sidebar = theme.bg_sidebar().to_hex(),
        separator = theme.separator.to_hex(),
        muted = theme.text_muted().to_hex(),
        danger = theme.accent_danger().to_hex(),
        success = theme.accent_success().to_hex(),
        img_error_icon = alert_icon_data_uri(
            tasty_icons::IMAGE.body,
            false,
            &theme.accent_danger().to_hex()
        ),
        alert_rules = alert_css(theme),
        hljs_rules = hljs_css(theme),
        find_match_bg = theme
            .accent_warning()
            .with_alpha(FIND_HIT_BG_ALPHA)
            .to_hex(),
        find_current_bg = theme.accent_primary().to_hex(),
        find_current_fg = theme.text_on_accent().to_hex(),
        accent = theme.accent_primary().to_hex(),
        on_accent = theme.text_on_accent().to_hex(),
    )
}

/// 콜아웃 종류별 테마 색과 아이콘 CSS를 만든다.
fn alert_css(theme: &Theme) -> String {
    /// ~12% opacity — same ratio `drop_overlay.rs` uses for `accent_primary().with_alpha(31)`.
    const BG_ALPHA: u8 = 31;
    let mut rules = String::new();
    for kind in CALLOUT_KINDS {
        let color = (kind.accent)(theme);
        let icon_uri = alert_icon_data_uri(kind.icon_body, kind.icon_filled, &color.to_hex());
        rules.push_str(&format!(
            ".{class}{{border-left-color:{hex};background:{bg};}}.{class}::before,.{class}>summary::before{{color:{hex};background-image:url(\"{icon_uri}\");}}\n",
            class = kind.class,
            hex = color.to_hex(),
            bg = color.with_alpha(BG_ALPHA).to_hex(),
        ));
    }
    rules
}

/// 지정한 색의 SVG를 data URI로 만든다.
fn alert_icon_data_uri(icon_body: &str, filled: bool, color_hex: &str) -> String {
    let (fill, stroke) = if filled {
        (color_hex, color_hex)
    } else {
        ("none", color_hex)
    };
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="{fill}" stroke="{stroke}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{icon_body}</svg>"#,
    );
    format!("data:image/svg+xml,{}", percent_encode_fragment(&svg))
}

/// 구문 강조 클래스에 테마 색을 연결한다.
fn hljs_css(theme: &Theme) -> String {
    format!(
        r#"code.hljs{{background:none;}}
.hljs-comment,.hljs-quote{{color:{comment};font-style:italic;}}
.hljs-meta{{color:{comment};}}
.hljs-keyword,.hljs-selector-tag,.hljs-subst,.hljs-operator{{color:{keyword};}}
.hljs-number,.hljs-literal{{color:{number};}}
.hljs-string,.hljs-doctag,.hljs-regexp,.hljs-link{{color:{string};}}
.hljs-title,.hljs-section,.hljs-selector-id,.hljs-title.function_{{color:{title};}}
.hljs-type,.hljs-class .hljs-title{{color:{type_};}}
.hljs-tag,.hljs-name,.hljs-attribute,.hljs-attr{{color:{tag};}}
.hljs-variable,.hljs-template-variable,.hljs-symbol,.hljs-bullet{{color:{variable};}}
.hljs-built_in,.hljs-builtin-name{{color:{builtin};}}
.hljs-deletion{{background:{deletion_bg};}}
.hljs-addition{{background:{addition_bg};}}
.hljs-emphasis{{font-style:italic;}}
.hljs-strong{{font-weight:700;}}
"#,
        comment = theme.text_muted().to_hex(),
        keyword = theme.mauve.to_hex(),
        number = theme.peach.to_hex(),
        string = theme.green.to_hex(),
        title = theme.blue.to_hex(),
        type_ = theme.yellow.to_hex(),
        tag = theme.teal.to_hex(),
        variable = theme.lavender.to_hex(),
        builtin = theme.red.to_hex(),
        deletion_bg = theme
            .accent_danger()
            .with_alpha(DIFF_LINE_BG_ALPHA)
            .to_hex(),
        addition_bg = theme
            .accent_success()
            .with_alpha(DIFF_LINE_BG_ALPHA)
            .to_hex(),
    )
}

/// Per-level heading pixel sizes, linearly interpolated between `font_size_prose_h1` (h1) and
/// `font_size_body` (h6) — real CSS, unlike the retired `egui_commonmark` renderer, can set
/// each level independently (module doc "library exceptions" this rewrite resolves).
fn heading_sizes_px(theme: &Theme) -> [f32; 6] {
    let h1 = theme.font_size_prose_h1.value();
    let body = theme.font_size_body.value();
    let step = (h1 - body) / 5.0;
    [
        h1,
        h1 - step,
        h1 - 2.0 * step,
        h1 - 3.0 * step,
        h1 - 4.0 * step,
        body,
    ]
}

// ── address bar (HTML/CSS/minimal trusted JS — design decision: Option 1) ────

/// 경로 입력, 최근 경로와 원격 문서 새로고침 버튼을 포함한 주소창.
fn addr_bar_html(
    tr: &Translator,
    file_path: &str,
    recent: &[String],
    remote: Option<RemoteView>,
) -> String {
    if let Some(view) = remote {
        let tooltip = if view.stale {
            tr.t("markdown.remote.refresh_stale")
        } else {
            tr.t("markdown.remote.refresh")
        };
        return format!(
            r#"<div id="tasty-addr-bar"><input id="tasty-addr-input" value="{value}" readonly><button id="tasty-refresh" type="button" data-stale="{stale}" title="{tooltip}" aria-label="{tooltip}">&#8635;</button></div>"#,
            value = attr_escape(file_path),
            stale = view.stale,
            tooltip = attr_escape(tooltip),
        );
    }
    let options: String = recent
        .iter()
        .map(|p| format!(r#"<option value="{}"></option>"#, attr_escape(p)))
        .collect();
    format!(
        r#"<div id="tasty-addr-bar"><input id="tasty-addr-input" list="tasty-addr-recent" value="{value}" placeholder="{placeholder}"><datalist id="tasty-addr-recent">{options}</datalist><button id="tasty-addr-go" title="{go_tooltip}">&#8594;</button></div>"#,
        value = attr_escape(file_path),
        placeholder = attr_escape(tr.t("markdown.addr.placeholder")),
        options = options,
        go_tooltip = attr_escape(tr.t("markdown.addr.go")),
    )
}

/// 플러그인이 본문 정리 후 추가하는 이동 스크립트.
/// 주소 입력과 새로고침은 #tasty-nav 마커로 호스트에 알린다.
/// 문서 안 링크는 기본 이동을 취소하고 해당 ID로 스크롤한다. 같은 링크를 다시
/// 클릭해도 동작하며, ID는 원문과 percent-decoding한 값을 차례로 찾는다.
///
/// 파일 경로별 스크롤 위치를 sessionStorage에 저장하고 복원을 시도한다.
/// load_html 이후 저장소가 유지되는지는 세 WebView 백엔드 모두에서 검증하지 않았다.
/// 저장소를 사용할 수 없으면 복원하지 않는다.
fn nav_script(file_path: &str) -> String {
    format!(
        r#"(function(){{
var i=document.getElementById('tasty-addr-input');
var g=document.getElementById('tasty-addr-go');
function go(){{
var v=i.value.trim();
if(!v)return;
location.hash='tasty-nav:addr:'+encodeURIComponent(v);
}}
if(g)g.addEventListener('click',go);
var r=document.getElementById('tasty-refresh');
if(r)r.addEventListener('click',function(){{location.hash='tasty-nav:refresh:'+Date.now();}});
if(i)i.addEventListener('keydown',function(e){{if(e.key==='Enter')go();}});
var toc=document.getElementById('tasty-toc');
var tocToggle=document.getElementById('tasty-toc-toggle');
if(toc&&tocToggle){{
tocToggle.addEventListener('click',function(){{
var collapsed=toc.classList.toggle('tasty-toc-collapsed');
tocToggle.setAttribute('aria-expanded',collapsed?'false':'true');
}});
}}
document.addEventListener('click',function(e){{
var t=e.target;
if(!t||!t.closest)return;
var a=t.closest('a[href]');
if(!a)return;
var href=a.getAttribute('href')||'';
if(href.charAt(0)!=='#')return;
if(href.indexOf('#'+{marker_json})===0)return;
e.preventDefault();
var id=href.slice(1);
if(!id){{window.scrollTo(0,0);return;}}
var el=document.getElementById(id);
if(!el){{try{{el=document.getElementById(decodeURIComponent(id));}}catch(err){{}}}}
if(!el)return;
el.scrollIntoView({{block:'start'}});
}});
var scrollKey='tasty-md-scroll:'+{file_path_json};
try{{
var saved=sessionStorage.getItem(scrollKey);
if(saved)window.scrollTo(0,parseInt(saved,10)||0);
}}catch(e){{}}
var saveTimer=null;
window.addEventListener('scroll',function(){{
if(saveTimer)clearTimeout(saveTimer);
saveTimer=setTimeout(function(){{
try{{sessionStorage.setItem(scrollKey,String(window.scrollY));}}catch(e){{}}
}},150);
}});
}})();"#,
        marker_json = serde_json::to_string(NAV_FRAGMENT_MARKER)
            .unwrap_or_else(|_| format!("\"{NAV_FRAGMENT_MARKER}\"")),
        file_path_json = serde_json::to_string(file_path).unwrap_or_else(|_| "\"\"".to_string()),
    )
}

// ── find-in-page (trusted JS, TreeWalker-based text-node highlight) ─────────────

/// 본문 검색 UI. 정규식 입력이 아닌 일반 텍스트 검색을 제공한다.
fn find_bar_html(tr: &Translator) -> String {
    format!(
        r#"<div id="tasty-find-bar" role="search" hidden><input id="tasty-find-input" type="text" placeholder="{placeholder}" autocomplete="off" spellcheck="false"><span id="tasty-find-count">0/0</span><button type="button" id="tasty-find-prev" class="tasty-find-btn" title="{prev}" aria-label="{prev}">&#9650;</button><button type="button" id="tasty-find-next" class="tasty-find-btn" title="{next}" aria-label="{next}">&#9660;</button><button type="button" id="tasty-find-close" class="tasty-find-btn" title="{close}" aria-label="{close}">&times;</button></div>"#,
        placeholder = attr_escape(tr.t("markdown.find.placeholder")),
        prev = attr_escape(tr.t("markdown.find.prev_tooltip")),
        next = attr_escape(tr.t("markdown.find.next_tooltip")),
        close = attr_escape(tr.t("markdown.find.close_tooltip")),
    )
}

/// 플러그인이 제공하는 본문 검색 스크립트. 이전 강조를 지운 뒤 #tasty-md-body의
/// 텍스트 노드를 탐색하며 pre/code 안의 텍스트는 제외한다. 검색어를 정규식
/// 리터럴로 escape해 대소문자 구분 없이 찾는다. 검색 UI 자체는 본문 밖에 있다.
/// IME 조합 중에는 새 검색 예약을 생략하고 compositionend에서 다시 예약한다.
fn find_in_page_script(tr: &Translator) -> String {
    let json_or_empty = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(function(){{
var bar=document.getElementById('tasty-find-bar');
var input=document.getElementById('tasty-find-input');
var countEl=document.getElementById('tasty-find-count');
var prevBtn=document.getElementById('tasty-find-prev');
var nextBtn=document.getElementById('tasty-find-next');
var closeBtn=document.getElementById('tasty-find-close');
var body=document.getElementById('tasty-md-body');
if(!bar||!input||!countEl||!prevBtn||!nextBtn||!closeBtn||!body)return;
var MATCH_COUNT={match_count};
var matches=[];
var current=-1;
var composing=false;
var debounceTimer=null;
function clearHighlights(){{
matches.forEach(function(m){{
var parent=m.parentNode;
if(!parent)return;
parent.replaceChild(document.createTextNode(m.textContent),m);
parent.normalize();
}});
matches=[];
current=-1;
}}
function escapeRegExp(s){{return s.replace(/[.*+?^${{}}()|[\]\\]/g,'\\$&');}}
function collectTextNodes(){{
var nodes=[];
var walker=document.createTreeWalker(body,NodeFilter.SHOW_TEXT,{{
acceptNode:function(node){{
if(!node.nodeValue||!node.nodeValue.trim())return NodeFilter.FILTER_REJECT;
var el=node.parentElement;
while(el&&el!==body){{
if(el.tagName==='PRE'||el.tagName==='CODE')return NodeFilter.FILTER_REJECT;
el=el.parentElement;
}}
return NodeFilter.FILTER_ACCEPT;
}}
}});
var n;
while((n=walker.nextNode()))nodes.push(n);
return nodes;
}}
function updateCount(){{
var total=matches.length;
if(total===0){{
countEl.textContent='0/0';
}}else{{
countEl.textContent=MATCH_COUNT.replace('{{current}}',String(current+1)).replace('{{total}}',String(total));
}}
countEl.classList.toggle('tasty-find-nomatch',input.value.length>0&&total===0);
prevBtn.disabled=total===0;
nextBtn.disabled=total===0;
}}
function applyCurrent(){{
matches.forEach(function(m,i){{
if(i===current)m.classList.add('tasty-find-current');
else m.classList.remove('tasty-find-current');
}});
if(current>=0)matches[current].scrollIntoView({{block:'center'}});
}}
function runSearch(){{
clearHighlights();
var query=input.value;
if(!query){{updateCount();return;}}
var re=new RegExp(escapeRegExp(query),'gi');
collectTextNodes().forEach(function(node){{
var text=node.nodeValue;
re.lastIndex=0;
var m;
var frag=null;
var lastIndex=0;
while((m=re.exec(text))){{
if(!frag)frag=document.createDocumentFragment();
if(m.index>lastIndex)frag.appendChild(document.createTextNode(text.slice(lastIndex,m.index)));
var mark=document.createElement('mark');
mark.className='tasty-find-hit';
mark.textContent=m[0];
frag.appendChild(mark);
matches.push(mark);
lastIndex=m.index+m[0].length;
}}
if(frag){{
if(lastIndex<text.length)frag.appendChild(document.createTextNode(text.slice(lastIndex)));
node.parentNode.replaceChild(frag,node);
}}
}});
current=matches.length?0:-1;
applyCurrent();
updateCount();
}}
function scheduleSearch(){{
if(composing)return;
if(debounceTimer)clearTimeout(debounceTimer);
debounceTimer=setTimeout(runSearch,150);
}}
function next(){{if(!matches.length)return;current=(current+1)%matches.length;applyCurrent();updateCount();}}
function prev(){{if(!matches.length)return;current=(current-1+matches.length)%matches.length;applyCurrent();updateCount();}}
function openBar(){{
bar.hidden=false;
input.focus();
input.select();
}}
function closeBar(){{
if(bar.hidden)return;
bar.hidden=true;
clearHighlights();
input.value='';
updateCount();
}}
input.addEventListener('compositionstart',function(){{composing=true;}});
input.addEventListener('compositionend',function(){{composing=false;scheduleSearch();}});
input.addEventListener('input',scheduleSearch);
input.addEventListener('keydown',function(e){{
if(e.key==='Escape'){{e.preventDefault();closeBar();}}
else if(e.key==='Enter'){{e.preventDefault();if(e.shiftKey)prev();else next();}}
}});
prevBtn.addEventListener('click',prev);
nextBtn.addEventListener('click',next);
closeBtn.addEventListener('click',closeBar);
document.addEventListener('keydown',function(e){{
if((e.ctrlKey||e.metaKey)&&!e.altKey&&(e.key==='f'||e.key==='F')){{
e.preventDefault();
openBar();
}}else if(e.key==='Escape'&&!bar.hidden){{
closeBar();
}}
}});
}})();"#,
        match_count = json_or_empty(tr.t("markdown.find.match_count")),
    )
}

// ── mermaid ────────────────────────────────────────────────────────────────────

/// 번들에 포함한 Mermaid 코드. 버전과 라이선스는 assets/NOTICE.md에 있다.
const MERMAID_JS_RAW: &str = include_str!("../assets/mermaid.min.js");

/// HTML script 요소를 중간에 닫지 않도록 번들의 </script를 대소문자 구분 없이
/// <\/script로 바꾼다. JS 문자열과 정규식 리터럴의 슬래시 escape에 사용한다.
/// ASCII 부분만 검사·치환하므로 다른 UTF-8 바이트는 그대로 복사한다.
fn escape_script_close(s: &str) -> String {
    let bytes = s.as_bytes();
    let lower = s.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if lower_bytes[i..].starts_with(b"</script") {
            out.extend_from_slice(b"<\\/");
            out.extend_from_slice(&bytes[i + 2..i + 8]);
            i += 8;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// 종료 태그를 escape한 Mermaid 번들을 프로세스당 한 번 만들어 보관한다.
fn mermaid_js_source() -> &'static str {
    static ESCAPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ESCAPED.get_or_init(|| escape_script_close(MERMAID_JS_RAW))
}

/// Mermaid 번들과 실행 스크립트. Theme.is_light에
/// 따라 default 또는 dark 테마를 사용한다. suppressErrors를 지정하고
/// 초기화 예외와 run의 실패를 콘솔에 기록한다.
fn mermaid_script(is_light: bool) -> String {
    let mermaid_theme = if is_light { "default" } else { "dark" };
    format!(
        r#"<script>{js}</script><script>(function(){{try{{mermaid.initialize({{startOnLoad:false,theme:'{mermaid_theme}'}});mermaid.run({{querySelector:'code.language-mermaid',suppressErrors:true}}).catch(function(e){{console.error('mermaid render failed',e);}});}}catch(e){{console.error('mermaid init failed',e);}}}})();</script>"#,
        js = mermaid_js_source(),
        mermaid_theme = mermaid_theme,
    )
}

// ── syntax highlighting (highlight.js) ──────────────────────────────────────────

/// 번들에 포함한 highlight.js 코드. 버전과 언어 목록은 assets/NOTICE.md에 있다.
const HIGHLIGHT_JS_RAW: &str = include_str!("../assets/highlight.min.js");

/// 종료 태그를 escape한 구문 강조 번들을 한 번 만들어 보관한다.
fn highlight_js_source() -> &'static str {
    static ESCAPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ESCAPED.get_or_init(|| escape_script_close(HIGHLIGHT_JS_RAW))
}

/// 언어가 지정된 코드 블록에 구문 강조를 적용한다. 지원하지 않는 언어는 건너뛴다.
/// highlight.js는 코드의 textContent로 토큰 span을 만들고 색은 hljs_css에서 정한다.
/// 블록별 예외를 기록한 뒤 다음 블록을 처리한다.
fn highlight_script() -> String {
    format!(
        r#"<script>{js}</script><script>(function(){{try{{document.querySelectorAll('pre code[class*="language-"]').forEach(function(el){{try{{var m=/language-([A-Za-z0-9_+-]+)/.exec(el.className);if(!m)return;if(!hljs.getLanguage(m[1]))return;hljs.highlightElement(el);}}catch(e){{console.error('highlight.js block failed',e);}}}});}}catch(e){{console.error('highlight.js init failed',e);}}}})();</script>"#,
        js = highlight_js_source(),
    )
}

// ── copy-to-clipboard button ────────────────────────────────────────────────

/// 본문의 pre > code에 복사 버튼을 넣는다. 오류 상세처럼 code 자식이 없는
/// pre에는 넣지 않는다. 비동기 렌더링 중일 수 있는 Mermaid 블록도 제외한다.
/// 클릭 시 textContent를 읽어 복사하므로 구문 강조용 태그는 복사되지 않는다.
///
/// navigator.clipboard.writeText를 사용할 수 없거나 실패하면 execCommand(copy)를
/// 시도한다. Linux/WebKitGTK에서 확인한 것은 writeText 경로이며, 그 실행에서
/// fallback이나 macOS·Windows의 동작까지 확인한 것은 아니다.
/// 기록: docs/plugins/markdown/screens/markdown.md.
fn copy_button_script(tr: &Translator) -> String {
    let json_or_empty = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"<script>(function(){{
var COPY={copy},COPIED={copied},FAILED={failed};
function fallbackCopy(text){{
try{{
var ta=document.createElement('textarea');
ta.value=text;
ta.setAttribute('readonly','');
ta.style.position='fixed';
ta.style.top='-1000px';
ta.style.opacity='0';
document.body.appendChild(ta);
ta.select();
ta.setSelectionRange(0,text.length);
var ok=document.execCommand('copy');
document.body.removeChild(ta);
return ok;
}}catch(e){{return false;}}
}}
function feedback(btn,ok){{
if(btn._tastyResetTimer)clearTimeout(btn._tastyResetTimer);
btn.textContent=ok?COPIED:FAILED;
btn.setAttribute('data-state',ok?'copied':'failed');
btn._tastyResetTimer=setTimeout(function(){{
btn.textContent=COPY;
btn.removeAttribute('data-state');
}},1500);
}}
try{{
document.querySelectorAll('#tasty-md-body pre > code').forEach(function(code){{
try{{
if(code.classList.contains('language-mermaid'))return;
var pre=code.parentElement;
if(!pre||pre.querySelector('.tasty-copy-btn'))return;
var btn=document.createElement('button');
btn.type='button';
btn.className='tasty-copy-btn';
btn.setAttribute('tabindex','0');
btn.setAttribute('aria-label',COPY);
btn.textContent=COPY;
btn.addEventListener('click',function(){{
var text=code.textContent;
if(navigator.clipboard&&navigator.clipboard.writeText){{
navigator.clipboard.writeText(text).then(function(){{feedback(btn,true);}},function(){{feedback(btn,fallbackCopy(text));}});
}}else{{
feedback(btn,fallbackCopy(text));
}}
}});
pre.appendChild(btn);
}}catch(e){{console.error('copy button attach failed',e);}}
}});
}}catch(e){{console.error('copy button init failed',e);}}
}})();</script>"#,
        copy = json_or_empty(tr.t("markdown.copy.label")),
        copied = json_or_empty(tr.t("markdown.copy.copied")),
        failed = json_or_empty(tr.t("markdown.copy.failed")),
    )
}

/// 이미지 오류 이벤트를 구독하고, 이미 로드가 끝났지만 naturalWidth가 0인 이미지도
/// 실패 표시로 바꾼다. 요소의 data-tasty-img-failed로 중복 처리를 막는다.
/// 표시할 주소는 getAttribute(src)에서 읽는다. 이는 이미지 인라인 처리 후의 값이며
/// 원본 Markdown 경로와 다를 수 있다.
fn image_error_script(tr: &Translator) -> String {
    let json_or_empty = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"<script>(function(){{
var LABEL={label};
function replaceWithPlaceholder(img){{
if(img.dataset.tastyImgFailed)return;
img.dataset.tastyImgFailed='1';
var src=img.getAttribute('src')||'';
var alt=img.getAttribute('alt')||'';
var ph=document.createElement('span');
ph.className='tasty-img-error';
ph.setAttribute('role','img');
ph.setAttribute('aria-label',alt||LABEL);
var icon=document.createElement('span');
icon.className='tasty-img-error-icon';
ph.appendChild(icon);
var label=document.createElement('span');
label.className='tasty-img-error-label';
label.textContent=LABEL;
ph.appendChild(label);
if(src){{
var path=document.createElement('span');
path.className='tasty-img-error-path';
path.textContent=src;
ph.appendChild(path);
}}
if(img.parentNode)img.parentNode.replaceChild(ph,img);
}}
try{{
document.querySelectorAll('#tasty-md-body img').forEach(function(img){{
try{{
if(img.dataset.tastyImgChecked)return;
img.dataset.tastyImgChecked='1';
img.addEventListener('error',function(){{replaceWithPlaceholder(img);}});
if(img.complete&&img.naturalWidth===0){{replaceWithPlaceholder(img);}}
}}catch(e){{console.error('image error-check attach failed',e);}}
}});
}}catch(e){{console.error('image error-check init failed',e);}}
}})();</script>"#,
        label = json_or_empty(tr.t("markdown.image.failed")),
    )
}

// ── math (KaTeX) ─────────────────────────────────────────────────────────────

/// Vendored `katex.min.js` bundle (see `assets/NOTICE.md` for version/license/source). Fetched
/// once at packaging time — never over the network at runtime.
const KATEX_JS_RAW: &str = include_str!("../assets/katex.min.js");

/// Vendored `katex.min.css` — still has its original `@font-face { src: url(fonts/...) }`
/// relative paths at this point; [`katex_css_with_embedded_fonts`] rewrites those before use.
const KATEX_CSS_RAW: &str = include_str!("../assets/katex.min.css");

/// Every font KaTeX 0.18.4 ships (`woff2` only — see `assets/NOTICE.md`), paired with the exact
/// basename [`KATEX_CSS_RAW`]'s `@font-face` rules reference so [`katex_css_with_embedded_fonts`]
/// can find-and-replace each one directly by string match (no regex dependency needed — the set
/// of basenames is fixed and known at compile time).
const KATEX_FONTS: &[(&str, &[u8])] = &[
    (
        "KaTeX_AMS-Regular",
        include_bytes!("../assets/fonts/KaTeX_AMS-Regular.woff2"),
    ),
    (
        "KaTeX_Caligraphic-Bold",
        include_bytes!("../assets/fonts/KaTeX_Caligraphic-Bold.woff2"),
    ),
    (
        "KaTeX_Caligraphic-Regular",
        include_bytes!("../assets/fonts/KaTeX_Caligraphic-Regular.woff2"),
    ),
    (
        "KaTeX_Fraktur-Bold",
        include_bytes!("../assets/fonts/KaTeX_Fraktur-Bold.woff2"),
    ),
    (
        "KaTeX_Fraktur-Regular",
        include_bytes!("../assets/fonts/KaTeX_Fraktur-Regular.woff2"),
    ),
    (
        "KaTeX_Main-Bold",
        include_bytes!("../assets/fonts/KaTeX_Main-Bold.woff2"),
    ),
    (
        "KaTeX_Main-BoldItalic",
        include_bytes!("../assets/fonts/KaTeX_Main-BoldItalic.woff2"),
    ),
    (
        "KaTeX_Main-Italic",
        include_bytes!("../assets/fonts/KaTeX_Main-Italic.woff2"),
    ),
    (
        "KaTeX_Main-Regular",
        include_bytes!("../assets/fonts/KaTeX_Main-Regular.woff2"),
    ),
    (
        "KaTeX_Math-BoldItalic",
        include_bytes!("../assets/fonts/KaTeX_Math-BoldItalic.woff2"),
    ),
    (
        "KaTeX_Math-Italic",
        include_bytes!("../assets/fonts/KaTeX_Math-Italic.woff2"),
    ),
    (
        "KaTeX_SansSerif-Bold",
        include_bytes!("../assets/fonts/KaTeX_SansSerif-Bold.woff2"),
    ),
    (
        "KaTeX_SansSerif-Italic",
        include_bytes!("../assets/fonts/KaTeX_SansSerif-Italic.woff2"),
    ),
    (
        "KaTeX_SansSerif-Regular",
        include_bytes!("../assets/fonts/KaTeX_SansSerif-Regular.woff2"),
    ),
    (
        "KaTeX_Script-Regular",
        include_bytes!("../assets/fonts/KaTeX_Script-Regular.woff2"),
    ),
    (
        "KaTeX_Size1-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size1-Regular.woff2"),
    ),
    (
        "KaTeX_Size2-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size2-Regular.woff2"),
    ),
    (
        "KaTeX_Size3-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size3-Regular.woff2"),
    ),
    (
        "KaTeX_Size4-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size4-Regular.woff2"),
    ),
    (
        "KaTeX_Typewriter-Regular",
        include_bytes!("../assets/fonts/KaTeX_Typewriter-Regular.woff2"),
    ),
];

/// 종료 태그를 escape한 KaTeX 번들을 한 번 만들어 보관한다.
fn katex_js_source() -> &'static str {
    static ESCAPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ESCAPED.get_or_init(|| escape_script_close(KATEX_JS_RAW))
}

/// KaTeX CSS의 번들 폰트 URL을 data URI로 바꾼다.
fn katex_css_with_embedded_fonts() -> &'static str {
    static EMBEDDED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    EMBEDDED.get_or_init(|| {
        use base64::Engine;
        let mut css = KATEX_CSS_RAW.to_string();
        for (name, bytes) in KATEX_FONTS {
            let original = format!(
                r#"url(fonts/{name}.woff2) format("woff2"),url(fonts/{name}.woff) format("woff"),url(fonts/{name}.ttf) format("truetype")"#
            );
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            let replacement = format!(r#"url(data:font/woff2;base64,{b64}) format("woff2")"#);
            css = css.replace(&original, &replacement);
        }
        css
    })
}

/// 본문의 수식 span에서 textContent를 읽어 KaTeX에 전달한다.
/// trust:false로 URL·HTML을 허용하는 명령을 제한하고 throwOnError:false로
/// 파싱 오류를 표시한다. 그 밖의 예외는 요소별로 콘솔에 기록한다.
/// 재실행 시 이미 처리한 요소를 다시 해석하지 않도록 표시를 남긴다.
/// 수식 색은 CSS의 currentColor로 본문 색을 따른다.
///
/// Linux/WebKitGTK에서 잘못된 \frac{1}은 원문을 담은 .katex-error로, 함께 둔
/// 유효한 수식 두 개는 KaTeX MathML로 표시됐다.
/// 기록: docs/plugins/markdown/screens/markdown.md.
fn katex_script() -> String {
    format!(
        r#"<style>{css}</style><script>{js}</script><script>(function(){{
try{{
document.querySelectorAll('#tasty-md-body .math-inline, #tasty-md-body .math-display').forEach(function(el){{
try{{
if(el.dataset.tastyMathRendered)return;
el.dataset.tastyMathRendered='1';
var tex=el.textContent;
var display=el.classList.contains('math-display');
katex.render(tex,el,{{throwOnError:false,trust:false,displayMode:display}});
}}catch(e){{console.error('katex render failed',e);}}
}});
}}catch(e){{console.error('katex init failed',e);}}
}})();</script>"#,
        css = katex_css_with_embedded_fonts(),
        js = katex_js_source(),
    )
}

// ── escaping helpers ──────────────────────────────────────────────────────────

/// Escape text for placement in an HTML text node.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape text for placement inside a double-quoted HTML attribute value.
fn attr_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-encode every byte outside `A-Za-z0-9-_.~` — used for the nav-fragment payload
/// (embedded in an `href` attribute, so this alone also makes HTML-attribute-escaping moot:
/// no `&`/`"`/`<` survive encoding).
fn percent_encode_fragment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode `%XX` percent-escapes (inverse of [`percent_encode_fragment`] / JS `encodeURIComponent`).
/// Malformed sequences pass through literally rather than erroring — this decodes a same-process
/// value we generated ourselves moments earlier, so a defensive fallback is enough.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\docs\md")
        } else {
            PathBuf::from("/docs/md")
        }
    }

    fn file_str(c: Option<LinkClick>) -> String {
        match c {
            Some(LinkClick::File(p)) => p.to_string_lossy().replace('\\', "/"),
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn relative_resolves_against_base_dir() {
        let b = base();
        let got = file_str(classify_link("docs/index.md", Some(&b)));
        let want = base()
            .join("docs/index.md")
            .to_string_lossy()
            .replace('\\', "/");
        assert_eq!(got, want);
    }

    #[test]
    fn parent_relative_is_normalized() {
        let b = base();
        let got = file_str(classify_link("../sibling.md", Some(&b)));
        let want = if cfg!(windows) {
            "C:/docs/sibling.md"
        } else {
            "/docs/sibling.md"
        };
        assert_eq!(got, want);
    }

    #[test]
    fn absolute_dest_passes_through() {
        let b = base();
        let abs = if cfg!(windows) {
            r"C:\other\readme.md"
        } else {
            "/other/readme.md"
        };
        let got = file_str(classify_link(abs, Some(&b)));
        let want = if cfg!(windows) {
            "C:/other/readme.md"
        } else {
            "/other/readme.md"
        };
        assert_eq!(got, want);
    }

    #[test]
    fn external_schemes_are_external() {
        let b = base();
        for url in [
            "http://example.com",
            "https://example.com/page",
            "mailto:foo@bar.com",
            "data:text/plain;base64,AAAA",
        ] {
            assert_eq!(
                classify_link(url, Some(&b)),
                Some(LinkClick::External(url.to_string())),
                "{url} should be External",
            );
        }
    }

    #[test]
    fn javascript_scheme_is_ignored_not_treated_as_file() {
        let b = base();
        assert_eq!(classify_link("javascript:alert(1)", Some(&b)), None);
        assert_eq!(
            classify_link("JavaScript:alert(document.cookie)", Some(&b)),
            None
        );
    }

    #[test]
    fn anchor_is_ignored() {
        assert_eq!(classify_link("#section", base().as_path().into()), None);
    }

    #[test]
    fn relative_without_base_dir_is_unresolvable() {
        assert_eq!(classify_link("docs/index.md", None), None);
    }

    #[test]
    fn empty_dest_is_ignored() {
        assert_eq!(classify_link("", Some(&base())), None);
    }

    #[test]
    fn rewrite_link_dest_leaves_anchors_untouched() {
        assert_eq!(rewrite_link_dest("#heading"), "#heading");
        assert_eq!(rewrite_link_dest(""), "");
    }

    #[test]
    fn rewrite_link_dest_percent_encodes_into_nav_fragment() {
        let got = rewrite_link_dest("../sibling.md");
        assert_eq!(got, "#tasty-nav:link:..%2Fsibling.md");
    }

    #[test]
    fn parse_nav_fragment_roundtrips_link_and_addr() {
        let link_url = format!(
            "about:blank#{}",
            rewrite_link_dest("../sibling.md").trim_start_matches('#')
        );
        assert_eq!(
            parse_nav_fragment(&link_url),
            Some(NavIntent::Link("../sibling.md".to_string()))
        );

        let addr_url = "file:///docs/md/#tasty-nav:addr:%2Fhome%2Fu%2Fnotes.md";
        assert_eq!(
            parse_nav_fragment(addr_url),
            Some(NavIntent::Addr("/home/u/notes.md".to_string()))
        );
    }

    #[test]
    fn parse_nav_fragment_none_for_unrelated_url() {
        assert_eq!(parse_nav_fragment("about:blank"), None);
        assert_eq!(parse_nav_fragment("https://example.com/#section"), None);
    }

    #[test]
    fn sanitize_html_strips_script_tags() {
        let out = sanitize_html("<p>hi</p><script>alert(1)</script>");
        assert!(!out.contains("script"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn sanitize_html_strips_event_handler_attributes() {
        let out = sanitize_html(r#"<img src="x.png" onerror="alert(1)">"#);
        // ★ 먼저 img 가 **살아남았다**는 것을 못박는다. 이 줄이 없으면 아래 부정 둘은
        // `out` 이 빈 문자열일 때도 통과하고, 그 초록의 뜻은 "핸들러를 벗겼다" 가 아니라
        // "아무것도 안 남았다" 다. 빈 출력은 가상이 아니다 — 태그 허용목록에서 `img` 가
        // 빠지면 ammonia 가 요소를 통째로 지우고, 그때도 이 시험은 계속 초록이다.
        assert!(out.contains("x.png"));
        assert!(!out.contains("onerror"));
        assert!(!out.contains("alert"));
    }

    #[test]
    fn sanitize_html_strips_javascript_scheme_href() {
        let out = sanitize_html(r#"<a href="javascript:alert(1)">click</a>"#);
        // 위와 같은 이유의 양성 짝. 다만 이 줄이 증명하는 것은 **출력이 비지 않았다**
        // 까지다 — ammonia 는 `<a>` 를 지워도 안쪽 텍스트를 남기므로, "앵커가 살아남았다"
        // 는 뜻이 아니다. 그 이상을 주장하면 그게 새 거짓이다.
        assert!(out.contains("click"));
        assert!(!out.contains("javascript:"));
    }

    #[test]
    fn sanitize_html_keeps_tables_checkboxes_and_code() {
        let source = "| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n`inline` and:\n\n```\nfenced\n```\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(out.contains("<table"));
        assert!(out.contains("checkbox"));
        assert!(out.contains("<code"));
        assert!(out.contains("fenced"));
    }

    #[test]
    fn fenced_code_block_language_class_survives_sanitize() {
        let source = "```rust\nfn main() {}\n```\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(
            out.contains(r#"class="language-rust""#),
            "expected language class to survive sanitize, got: {out}"
        );
    }

    #[test]
    fn fenced_code_block_language_is_normalized_before_sanitize() {
        // `"` 이후 aren't valid identifier chars; pulldown-cmark itself HTML-escapes the info
        // string too (attribute breakout was never possible), but the point here is that the
        // *value* collapses to a plain identifier — the shape a future mermaid consumer relies on.
        assert_eq!(
            sanitize_fence_lang(r#"rust"><script>alert(1)</script>"#),
            "rustscriptalert1script"
        );
        assert_eq!(sanitize_fence_lang(""), "");
        assert_eq!(
            sanitize_fence_lang("mermaid extra-ignored-token"),
            "mermaid"
        );
    }

    // ── image captions (solo-image paragraphs → <figure>/<figcaption>) ─────────

    #[test]
    fn figurize_promotes_solo_image_with_alt_to_figure_caption() {
        let source = "![A caption](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(out.contains("<figure>"), "got: {out}");
        assert!(out.contains("</figure>"), "got: {out}");
        assert!(
            out.contains("<figcaption>A caption</figcaption>"),
            "got: {out}"
        );
        assert!(out.contains(r#"src="img.png""#), "got: {out}");
        assert!(out.contains(r#"alt="A caption""#), "got: {out}");
        // No leftover empty <p></p> wrapper around the promoted image.
        assert!(!out.contains("<p>"), "got: {out}");
    }

    #[test]
    fn figurize_leaves_alt_less_image_unpromoted() {
        // alt-less image — nothing to caption, must render exactly as before this feature.
        let source = "![](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains("<p>"), "got: {out}");
        assert!(out.contains(r#"src="img.png""#), "got: {out}");
    }

    #[test]
    fn figurize_does_not_promote_image_mixed_with_text() {
        let source = "before ![alt](img.png) after\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains("before"), "got: {out}");
        assert!(out.contains("after"), "got: {out}");
        assert!(out.contains(r#"alt="alt""#), "got: {out}");
    }

    #[test]
    fn figurize_does_not_promote_link_wrapped_image() {
        // `[![alt](img.png)](url)` — the image is alone, but wrapped in a link; conservative
        // policy declines promotion here too (see figurize_paragraph_buffer doc).
        let source = "[![alt](img.png)](https://example.com)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains("<a "), "got: {out}");
        assert!(out.contains(r#"alt="alt""#), "got: {out}");
    }

    #[test]
    fn figurize_does_not_promote_paragraph_with_two_images() {
        let source = "![a](1.png)![b](2.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains(r#"src="1.png""#), "got: {out}");
        assert!(out.contains(r#"src="2.png""#), "got: {out}");
    }

    #[test]
    fn figurize_caption_strips_markup_from_alt_with_emphasis() {
        let source = "![**bold** caption](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(
            out.contains("<figcaption>bold caption</figcaption>"),
            "got: {out}"
        );
        assert!(!out.contains("<strong>"), "got: {out}");
    }

    #[test]
    fn figurize_caption_escapes_special_characters_in_alt() {
        // Spaces around `<`/`>` keep pulldown-cmark from mis-parsing them as inline HTML tags —
        // they stay literal `Event::Text`, matching how a reader would actually type "1 < 2".
        let source = "![1 < 2 & 3 > 1](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(
            out.contains("<figcaption>1 &lt; 2 &amp; 3 &gt; 1</figcaption>"),
            "got: {out}"
        );
    }

    #[test]
    fn footnote_markup_survives_sanitize() {
        let source = "See[^1].\n\n[^1]: A note.\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(out.contains("footnote-reference"), "got: {out}");
        assert!(out.contains("footnote-definition"), "got: {out}");
        assert!(out.contains("A note."), "got: {out}");
    }

    fn real_translator() -> Translator {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("en.toml"), include_str!("../lang/en.toml")).unwrap();
        Translator::load(dir.path(), "en")
    }

    #[test]
    fn footnote_reference_and_backlink_are_wired_correctly() {
        let source = "See[^1].\n\n[^1]: A note.\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            out.contains(r#"id="fnref-1""#),
            "reference should get id=fnref-1, got: {out}"
        );
        assert!(
            out.contains(r##"href="#fndef-1""##),
            "reference should link to the definition, got: {out}"
        );
        assert!(
            out.contains(r#"id="fndef-1""#),
            "definition should get id=fndef-1, got: {out}"
        );
        assert!(
            out.contains(r##"href="#fnref-1""##),
            "definition should carry exactly one backlink to the reference, got: {out}"
        );
        assert_eq!(
            out.matches(r##"href="#fnref-1""##).count(),
            1,
            "a single reference should produce exactly one backlink, got: {out}"
        );
    }

    #[test]
    fn footnote_multiple_references_get_distinct_backlinks() {
        // Same footnote referenced twice — the definition must carry two backlinks, each
        // targeting its own reference occurrence, with no cross-contamination (e.g. both
        // pointing at the same id, or the second reference silently reusing the first's id).
        let source = "First[^a] and second[^a].\n\n[^a]: Shared note.\n";
        let out = unsafe_content_html(source, &Translator::default());

        assert!(out.contains(r#"id="fnref-a""#), "got: {out}");
        assert!(out.contains(r#"id="fnref-a-2""#), "got: {out}");
        assert_ne!(
            out.matches(r#"id="fnref-a""#).count() + out.matches(r#"id="fnref-a-2""#).count(),
            0
        );

        // Both references must point at the same definition.
        assert_eq!(
            out.matches(r##"href="#fndef-a""##).count(),
            2,
            "both references should link to the shared definition, got: {out}"
        );

        // The definition must carry exactly one backlink per reference occurrence, each
        // pointing at its own distinct id.
        assert_eq!(out.matches(r##"href="#fnref-a""##).count(), 1, "got: {out}");
        assert_eq!(
            out.matches(r##"href="#fnref-a-2""##).count(),
            1,
            "got: {out}"
        );
    }

    #[test]
    fn footnote_name_with_special_chars_gets_safe_id() {
        // Footnote names can contain whitespace/unicode (not valid verbatim in an HTML id) —
        // percent-encoding (reusing `percent_encode_fragment`, the same helper nav-fragments and
        // baked icon data URIs already use) keeps it a single-token, ASCII, collision-safe id.
        let source = "See[^노트 1].\n\n[^노트 1]: Definition.\n";
        let out = unsafe_content_html(source, &Translator::default());

        // No raw space or raw unicode byte sequence should leak into an id/href attribute value.
        assert!(
            !out.contains(r#"id="fnref-노트 1""#),
            "raw unicode/space must not appear verbatim in an id, got: {out}"
        );
        // The percent-encoded form must be internally consistent: whatever id the reference
        // carries is exactly what the definition's href (and vice versa) target.
        let safe = percent_encode_fragment("노트 1");
        assert!(
            out.contains(&format!(r#"id="fnref-{safe}""#)),
            "got: {out} (expected safe id fnref-{safe})"
        );
        assert!(
            out.contains(&format!(r##"href="#fndef-{safe}""##)),
            "got: {out}"
        );
        assert!(out.contains(&format!(r#"id="fndef-{safe}""#)), "got: {out}");
        assert!(
            out.contains(&format!(r##"href="#fnref-{safe}""##)),
            "got: {out}"
        );
    }

    #[test]
    fn undefined_footnote_reference_does_not_panic() {
        // No matching `[^missing]: ...` definition anywhere — pulldown-cmark's parser itself
        // never emits a `FootnoteReference` event for this (confirmed by dumping the raw event
        // stream: it falls back to plain `[`/`^missing`/`]` text), so rewrite_footnote_event
        // never even sees it. This just proves the whole pipeline degrades gracefully rather
        // than panicking or losing content.
        let source = "This has an[^missing] undefined reference.\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(!out.contains("footnote-reference"), "got: {out}");
        assert!(out.contains("^missing"), "got: {out}");
    }

    #[test]
    fn footnote_aria_labels_present_in_rendered_html() {
        let tr = real_translator();
        let source = "See[^1].\n\n[^1]: A note.\n";
        let out = unsafe_content_html(source, &tr);
        assert!(
            out.contains(r#"aria-label="Jump to footnote 1""#),
            "got: {out}"
        );
        assert!(
            out.contains(r#"aria-label="Back to footnote 1""#),
            "got: {out}"
        );

        // Survives sanitize_html's allowlist (`a` -> aria-label).
        let sanitized = sanitize_html(&out);
        assert!(
            sanitized.contains(r#"aria-label="Jump to footnote 1""#),
            "got: {sanitized}"
        );
        assert!(
            sanitized.contains(r#"aria-label="Back to footnote 1""#),
            "got: {sanitized}"
        );
    }

    #[test]
    fn footnote_aria_label_distinguishes_multiple_backlinks() {
        let tr = real_translator();
        let source = "First[^a] and second[^a].\n\n[^a]: Shared note.\n";
        let out = unsafe_content_html(source, &tr);
        assert!(
            out.contains(r#"aria-label="Back to footnote 1, reference 1""#),
            "got: {out}"
        );
        assert!(
            out.contains(r#"aria-label="Back to footnote 1, reference 2""#),
            "got: {out}"
        );
    }

    #[test]
    fn document_without_footnotes_renders_unaffected() {
        let source = "# Title\n\nJust a normal paragraph, nothing special.\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(!out.contains("footnote"), "got: {out}");
        assert!(!out.contains("fnref-"), "got: {out}");
        assert!(!out.contains("fndef-"), "got: {out}");
        assert!(out.contains("Just a normal paragraph"), "got: {out}");
    }

    #[test]
    fn unsafe_content_html_rewrites_internal_link_hrefs() {
        let out = unsafe_content_html("[go](./other.md)", &Translator::default());
        assert!(out.contains("#tasty-nav:link:"));
        assert!(!out.contains(r#"href="./other.md""#));
    }

    #[test]
    fn unsafe_content_html_leaves_anchor_links_untouched() {
        let out = unsafe_content_html("[jump](#section)", &Translator::default());
        assert!(out.contains(r##"href="#section""##));
    }

    #[test]
    fn wikilink_to_existing_file_becomes_plain_nav_link() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let out = sanitize_html(&unsafe_content_html_in_dir(
            "See [[Other]] for details.",
            &Translator::default(),
            Some(dir.path()),
        ));
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
        assert!(out.contains(">Other</a>"), "got: {out}");
        assert!(
            !out.contains("tasty-wikilink-missing"),
            "existing target must not get the missing marker, got: {out}"
        );
    }

    #[test]
    fn wikilink_to_missing_file_gets_missing_marker() {
        let dir = tempfile::tempdir().unwrap();
        let out = sanitize_html(&unsafe_content_html_in_dir(
            "See [[Nonexistent]] for details.",
            &Translator::default(),
            Some(dir.path()),
        ));
        assert!(
            out.contains("tasty-wikilink-missing"),
            "missing target should get the visual marker, got: {out}"
        );
        // Still a real link (destination present, not stripped) — clicking falls through to
        // the existing nonexistent-file handling in main.rs::dispatch_file_link.
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
        assert!(out.contains(">Nonexistent</a>"), "got: {out}");
    }

    #[test]
    fn wikilink_with_no_base_dir_is_treated_as_missing() {
        // Same "unresolvable" treatment as classify_link gives an ordinary relative destination
        // with no base directory — still a link, just visually marked.
        let out = sanitize_html(&unsafe_content_html_in_dir(
            "See [[Other]] for details.",
            &Translator::default(),
            None,
        ));
        assert!(out.contains("tasty-wikilink-missing"), "got: {out}");
    }

    #[test]
    fn wikilink_custom_display_text() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let out = unsafe_content_html_in_dir(
            "See [[Other|a different page]] for details.",
            &Translator::default(),
            Some(dir.path()),
        );
        assert!(out.contains(">a different page</a>"), "got: {out}");
        assert!(!out.contains(">Other</a>"), "got: {out}");
    }

    #[test]
    fn wikilink_inside_code_block_is_not_linked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let out = unsafe_content_html_in_dir(
            "```\n[[Other]]\n```\n",
            &Translator::default(),
            Some(dir.path()),
        );
        assert!(!out.contains("<a "), "got: {out}");
        assert!(out.contains("[[Other]]"), "got: {out}");
    }

    #[test]
    fn wikilink_with_path_traversal_is_left_as_literal_text() {
        let dir = tempfile::tempdir().unwrap();
        for body in ["[[../secret]]", "[[sub/dir]]", r"[[sub\dir]]"] {
            let out = unsafe_content_html_in_dir(body, &Translator::default(), Some(dir.path()));
            assert!(
                !out.contains("<a "),
                "{body} must not become a link, got: {out}"
            );
            assert!(
                out.contains(body),
                "{body} should pass through unchanged, got: {out}"
            );
        }
    }

    #[test]
    fn wikilink_does_not_collide_with_footnote_reference() {
        // Empirical confirmation (not just the structural `Event::FootnoteReference` argument in
        // resolve_wikilinks's doc comment): both syntaxes in the same document, each must resolve
        // to its own kind of markup with no cross-contamination.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let source = "See[^1] and [[Other]].\n\n[^1]: A note.\n";
        let out = unsafe_content_html_in_dir(source, &Translator::default(), Some(dir.path()));
        assert!(out.contains("footnote-reference"), "got: {out}");
        assert!(out.contains(r#"id="fnref-1""#), "got: {out}");
        assert!(out.contains(">Other</a>"), "got: {out}");
        assert!(
            !out.contains("[[Other]]"),
            "wikilink should have been rewritten, got: {out}"
        );
        assert!(
            !out.contains("[^1]"),
            "footnote ref should have been rewritten, got: {out}"
        );
    }

    #[test]
    fn bare_url_becomes_nav_fragment_link() {
        let out = sanitize_html(&unsafe_content_html(
            "Visit https://example.com today.",
            &Translator::default(),
        ));
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
        assert!(!out.contains(r#"href="https://example.com""#), "got: {out}");
        assert!(out.contains(">https://example.com</a>"), "got: {out}");
    }

    #[test]
    fn bare_url_inside_inline_code_is_not_linked() {
        let out = unsafe_content_html("`https://example.com` is code.", &Translator::default());
        assert!(
            out.contains("<code>https://example.com</code>"),
            "got: {out}"
        );
        assert!(!out.contains("<a "), "got: {out}");
    }

    #[test]
    fn bare_url_inside_code_block_is_not_linked() {
        let out = unsafe_content_html("```\nhttps://example.com\n```\n", &Translator::default());
        assert!(out.contains("<pre><code>https://example.com"), "got: {out}");
        assert!(!out.contains("<a "), "got: {out}");
    }

    #[test]
    fn already_explicit_link_is_not_double_linked() {
        let out = unsafe_content_html(
            "[https://example.com](https://example.com)",
            &Translator::default(),
        );
        assert_eq!(out.matches("<a ").count(), 1, "got: {out}");
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
    }

    #[test]
    fn bare_url_trailing_sentence_period_stays_outside_link() {
        let out = unsafe_content_html("Visit https://example.com. Thanks.", &Translator::default());
        assert!(out.contains(">https://example.com</a>."), "got: {out}");
    }

    #[test]
    fn bare_url_with_balanced_parens_keeps_trailing_paren() {
        let out = unsafe_content_html(
            "See https://en.wikipedia.org/wiki/Rust_(programming_language) here.",
            &Translator::default(),
        );
        assert!(
            out.contains(">https://en.wikipedia.org/wiki/Rust_(programming_language)</a>"),
            "got: {out}"
        );
    }

    #[test]
    fn bare_url_wrapped_in_sentence_parens_excludes_outer_paren() {
        let out = unsafe_content_html("See (https://example.com) here.", &Translator::default());
        assert!(out.contains(">https://example.com</a>)"), "got: {out}");
    }

    #[test]
    fn yaml_frontmatter_at_document_start_is_hidden() {
        let out = unsafe_content_html("---\nkey: value\n---\n\n# Body\n", &Translator::default());
        assert!(!out.contains("<hr"));
        assert!(!out.contains("key: value"));
        assert!(!out.contains("<h2"));
        // 이제 heading 은 자동 슬러그 `id` 를 받는다 — "heading ids + TOC" 절.
        assert!(out.contains(r#"<h1 id="body">Body</h1>"#), "got: {out}");
    }

    #[test]
    fn toml_frontmatter_at_document_start_is_hidden() {
        let out = unsafe_content_html(
            "+++\nkey = \"value\"\n+++\n\n# Body\n",
            &Translator::default(),
        );
        assert!(!out.contains("<hr"));
        assert!(!out.contains("key = "));
        assert!(out.contains(r#"<h1 id="body">Body</h1>"#), "got: {out}");
    }

    #[test]
    fn thematic_break_mid_document_still_renders_as_hr() {
        let out = unsafe_content_html("# Title\n\n---\n\nMore text\n", &Translator::default());
        assert!(out.contains("<hr"));
        assert!(out.contains(r#"<h1 id="title">Title</h1>"#), "got: {out}");
        assert!(out.contains("More text"));
    }

    #[test]
    fn smart_punctuation_converts_quotes_dashes_and_ellipsis() {
        let out = unsafe_content_html("\"hello\" and 'test'\n", &Translator::default());
        assert!(out.contains('\u{201c}'));
        assert!(out.contains('\u{201d}'));
        assert!(out.contains('\u{2018}'));
        assert!(out.contains('\u{2019}'));

        let out = unsafe_content_html("a -- b and a --- b\n", &Translator::default());
        assert!(out.contains('\u{2013}'));
        assert!(out.contains('\u{2014}'));

        let out = unsafe_content_html("wait...\n", &Translator::default());
        assert!(out.contains('\u{2026}'));
    }

    #[test]
    fn smart_punctuation_does_not_convert_inside_code() {
        let out = unsafe_content_html("`--` stays literal\n", &Translator::default());
        assert!(out.contains("<code>--</code>"));

        let out = unsafe_content_html("```\n--\n```\n", &Translator::default());
        assert!(out.contains("--\n"));
        assert!(!out.contains('\u{2013}'));
        assert!(!out.contains('\u{2014}'));
    }

    #[test]
    fn escaped_punctuation_stays_literal() {
        let out = unsafe_content_html("\\\"hello\\\"\n", &Translator::default());
        assert!(out.contains("\"hello\""));
        assert!(!out.contains('\u{201c}'));

        let out = unsafe_content_html("a \\-\\- b\n", &Translator::default());
        assert!(out.contains("a -- b"));
        assert!(!out.contains('\u{2013}'));
    }

    #[test]
    fn percent_encode_and_decode_roundtrip() {
        let raw = "a b/c#d?e&f=g";
        let enc = percent_encode_fragment(raw);
        assert_eq!(percent_decode(&enc), raw);
    }

    // CSS 문자열의 선언을 검사한다. 실제 WebView 레이아웃 검증을 대신하지 않는다.
    fn stylesheet_of_a_rendered_document() -> String {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let recent: Vec<String> = Vec::new();
        render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/current.md",
            source: "# Hello\n\nSome text.",
            load_error: None,
            base_dir: Some(Path::new("/a")),
            recent: &recent,
            remote: None,
        })
    }

    #[test]
    fn stylesheet_lets_body_grow_while_html_stays_definite() {
        let css = stylesheet_of_a_rendered_document();
        assert!(
            css.contains("html{height:100%"),
            "html 높이를 지정해 body의 백분율 min-height 기준을 제공해야 한다"
        );
        assert!(
            css.contains("body{min-height:100%"),
            "긴 문서에서도 sticky 주소창이 보이도록 body가 늘어날 수 있어야 한다"
        );
        assert!(
            !css.contains("html,body{height:100%"),
            "html과 body 모두에 height:100%를 지정하면 안 된다"
        );
    }

    #[test]
    fn bar_height_is_declared_once_and_read_by_four_rules() {
        let css = stylesheet_of_a_rendered_document();
        assert_eq!(
            css.matches("--md-addr-bar-h:").count(),
            1,
            "주소창 높이 선언은 한 자리여야 한다"
        );
        assert_eq!(
            css.matches("var(--md-addr-bar-h)").count(),
            4,
            "주소창 높이, 검색창 위치, 제목과 각주 여백이 같은 CSS 변수를 사용해야 한다"
        );
    }

    #[test]
    fn render_document_embeds_theme_css_and_addr_bar() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let recent = vec!["/a/one.md".to_string(), "/b/two.md".to_string()];
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/current.md",
            source: "# Hello\n\nSome *text*.",
            load_error: None,
            base_dir: Some(Path::new("/a")),
            recent: &recent,
            remote: None,
        });
        assert!(html.contains("<style>"));
        assert!(html.contains("tasty-addr-bar"));
        assert!(html.contains("tasty-addr-input"));
        assert!(html.contains("/a/one.md"));
        assert!(html.contains("Hello"));
        assert!(!html.contains("<script>alert"));
    }

    fn remote_document(source: &str, load_error: Option<&str>, view: RemoteView) -> String {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/remote/notes.md",
            source,
            load_error,
            base_dir: None,
            recent: &["/local/recent.md".to_string()],
            remote: Some(view),
        })
    }

    #[test]
    fn remote_document_swaps_go_for_a_refresh_button_that_carries_the_stale_flag() {
        let fresh = remote_document(
            "# Remote",
            None,
            RemoteView {
                loading: false,
                stale: false,
                ..RemoteView::default()
            },
        );
        assert!(fresh.contains(r#"id="tasty-refresh""#));
        assert!(fresh.contains(r#"data-stale="false""#));
        assert!(fresh.contains("readonly"));
        // 경로는 원격 호스트의 것이라 여기서 열 수 없다 — Go 와 이 머신의 최근목록을 내지 않는다.
        assert!(!fresh.contains(r#"id="tasty-addr-go""#));
        assert!(!fresh.contains("/local/recent.md"));
        assert!(!fresh.contains("<base href="));
        assert!(fresh.contains("markdown.remote.refresh"));

        let stale = remote_document(
            "# Remote",
            None,
            RemoteView {
                loading: false,
                stale: true,
                ..RemoteView::default()
            },
        );
        assert!(stale.contains(r#"data-stale="true""#));
        assert!(stale.contains("markdown.remote.refresh_stale"));
    }

    #[test]
    fn stale_refresh_button_color_comes_from_theme_accent_tokens() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let css = theme_css(&theme);
        let rule = css
            .lines()
            .find(|l| l.starts_with(r#"#tasty-refresh[data-stale="true"]"#))
            .expect("stale rule");
        assert!(rule.contains(&theme.accent_primary().to_hex()), "{rule}");
        assert!(rule.contains(&theme.text_on_accent().to_hex()), "{rule}");
    }

    #[test]
    fn remote_document_shows_loading_until_content_arrives_and_error_wins() {
        let loading = remote_document(
            "",
            None,
            RemoteView {
                loading: true,
                stale: false,
                ..RemoteView::default()
            },
        );
        assert!(loading.contains("markdown.remote.loading"));
        assert!(!loading.contains("markdown.state.empty"));

        let failed = remote_document(
            "",
            Some("mirror workspace disconnected"),
            RemoteView {
                loading: true,
                stale: false,
                ..RemoteView::default()
            },
        );
        assert!(failed.contains("markdown.state.failed"));
        assert!(failed.contains("mirror workspace disconnected"));
        assert!(!failed.contains("markdown.remote.loading"));
    }

    /// 연결이 끊긴 문서는 받은 원문이 있어도 원문 대신 끊김을 그린다 — 옛 원문을 그대로
    /// 두면 연결이 살아 있는 화면과 구분되지 않는다. 로딩·실패보다도 앞선다.
    #[test]
    fn disconnected_remote_document_shows_the_disconnect_instead_of_the_old_source() {
        let disconnected = RemoteView {
            disconnected: true,
            ..RemoteView::default()
        };
        let html = remote_document("# Old heading", None, disconnected);
        assert!(html.contains("markdown.remote.disconnected"));
        assert!(!html.contains("Old heading"));
        assert!(
            html.contains(r#"id="tasty-refresh""#),
            "새로고침은 그대로 누를 수 있다"
        );

        let while_loading = remote_document(
            "",
            Some("boom"),
            RemoteView {
                loading: true,
                ..disconnected
            },
        );
        assert!(while_loading.contains("markdown.remote.disconnected"));
        assert!(!while_loading.contains("markdown.remote.loading"));
        assert!(!while_loading.contains("markdown.state.failed"));
    }

    #[test]
    fn remote_document_does_not_inline_images_from_the_local_disk() {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("tasty-md-remote-img-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("pic.png");
        std::fs::write(&img, b"\x89PNG\r\n\x1a\n").unwrap();
        let source = format!("![pic]({})", img.display());
        let html = remote_document(&source, None, RemoteView::default());
        assert!(
            !html.contains("data:image/png;base64"),
            "로컬 파일을 끌어오면 안 된다"
        );
        let _ = std::fs::remove_dir_all(&dir); // best-effort 정리 — 실패 무시(테스트 결과 무관).
    }

    #[test]
    fn refresh_script_uses_a_nonce_so_repeat_clicks_still_navigate() {
        let script = nav_script("/remote/notes.md");
        assert!(script.contains("tasty-nav:refresh:'+Date.now()"));
    }

    #[test]
    fn parse_nav_fragment_reads_refresh_with_any_nonce() {
        assert_eq!(
            parse_nav_fragment("about:blank#tasty-nav:refresh:1757750000000"),
            Some(NavIntent::Refresh)
        );
        assert_eq!(parse_nav_fragment("about:blank#tasty-nav:refreshx"), None);
    }

    #[test]
    fn nav_script_keys_scroll_restore_by_file_path_and_json_escapes_it() {
        let script = nav_script("/a/weird \"path\".md");
        assert!(script.contains("sessionStorage"));
        assert!(script.contains("tasty-md-scroll:"));
        assert!(script.contains("scrollTo"));
        // file_path is embedded via serde_json::to_string, so an embedded quote must come
        // through backslash-escaped (valid JS string literal) rather than breaking out of it.
        assert!(script.contains(r#"weird \"path\".md"#));
        assert!(!script.contains(r#"'weird "path".md'"#));
    }

    #[test]
    fn render_document_shows_error_state() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/missing.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(html.contains("No such file"));
    }

    #[test]
    fn render_document_inlines_mermaid_only_when_block_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_mermaid = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/diagram.md",
            source: "```mermaid\ngraph TD; A-->B;\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(with_mermaid.contains(r#"class="language-mermaid""#));
        assert!(with_mermaid.contains("mermaid.initialize"));
        assert!(with_mermaid.contains("mermaid.run"));
        assert!(with_mermaid.contains("querySelector:'code.language-mermaid'"));
        assert!(with_mermaid.contains("suppressErrors:true"));

        let without_mermaid = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "```rust\nfn main() {}\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(without_mermaid.contains(r#"class="language-rust""#));
        assert!(!without_mermaid.contains("mermaid.initialize"));
        assert!(!without_mermaid.contains("mermaid.run"));
    }

    #[test]
    fn mermaid_script_picks_theme_from_is_light() {
        assert!(mermaid_script(true).contains("theme:'default'"));
        assert!(mermaid_script(false).contains("theme:'dark'"));
    }

    #[test]
    fn mermaid_script_js_source_has_no_premature_script_close() {
        let js = mermaid_js_source();
        // 번들이 비어 있으면 아래 부정은 무조건 통과한다 — 그 갈래를 먼저 닫는다.
        assert!(js.contains("mermaid"));
        assert!(!js.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn escape_script_close_neutralizes_mixed_case_occurrences() {
        let input = "a</script>b</SCRIPT>c</Script>d</ScRiPt>e";
        let escaped = escape_script_close(input);
        // No case variant of `</script` survives.
        assert!(!escaped.to_ascii_lowercase().contains("</script"));
        // Original case of the tag name is preserved, only `</` becomes `<\/`.
        assert_eq!(escaped, r#"a<\/script>b<\/SCRIPT>c<\/Script>d<\/ScRiPt>e"#);
    }

    #[test]
    fn escape_script_close_leaves_unrelated_text_untouched() {
        assert_eq!(escape_script_close(""), "");
        assert_eq!(
            escape_script_close("no closing tags here"),
            "no closing tags here"
        );
        assert_eq!(escape_script_close("</scrip"), "</scrip"); // too short to match
        // Non-ASCII text around/inside the match must survive intact (UTF-8 char-boundary safety).
        assert_eq!(
            escape_script_close("한글</script>한글"),
            "한글<\\/script>한글"
        );
    }

    #[test]
    fn mermaid_script_wraps_run_in_try_catch_with_console_error_fallback() {
        let script = mermaid_script(false);
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
        assert!(script.contains(".catch(function(e){console.error"));
    }

    // ── syntax highlighting (highlight.js) ──────────────────────────────────

    #[test]
    fn render_document_inlines_highlight_js_only_when_code_block_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/snippet.md",
            source: "```rust\nfn main() {}\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(with_code.contains(r#"class="language-rust""#));
        assert!(with_code.contains("hljs.getLanguage"));
        assert!(with_code.contains("hljs.highlightElement"));

        let without_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo fenced blocks here.",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(!without_code.contains("hljs.getLanguage"));
        assert!(!without_code.contains("hljs.highlightElement"));
    }

    #[test]
    fn highlight_js_source_has_no_premature_script_close() {
        // Same invariant as `mermaid_script_js_source_has_no_premature_script_close` — the
        // bundle is inlined verbatim inside an HTML <script> element.
        let js = highlight_js_source();
        assert!(!js.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn highlight_script_wraps_each_block_in_try_catch_with_console_error_fallback() {
        let script = highlight_script();
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error('highlight.js block failed'"));
        assert!(script.contains("catch(e){console.error('highlight.js init failed'"));
    }

    #[test]
    fn highlight_script_skips_unsupported_languages_without_erroring() {
        let script = highlight_script();
        assert!(script.contains("if(!hljs.getLanguage(m[1]))return;"));
    }

    #[test]
    fn highlight_js_recognizes_every_minimum_required_language() {
        // 언어 식별자와 별칭을 번들에서 찾을 수 있는지 확인한다.
        let js = highlight_js_source();
        for lang in [
            "rust",
            "javascript",
            "typescript",
            "python",
            "json",
            "bash",
            "yaml",
            "markdown",
        ] {
            assert!(
                js.contains(&format!("grmr_{lang}:")),
                "expected the vendored bundle to register grammar grmr_{lang}"
            );
        }
        assert!(
            js.contains("grmr_ini:") && js.contains(r#"aliases:["toml"]"#),
            "expected toml to be covered as an alias of the ini grammar"
        );
    }

    #[test]
    fn code_block_with_literal_script_tag_stays_escaped_text_after_highlight_insertion() {
        // 코드 텍스트를 HTML로 실행하지 않도록 escape하는지 확인한다.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/xss.md",
            source: "```rust\n<script>alert(1)</script>\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        // The only `<script` occurrences in the whole document must be tasty's own trusted
        // script tags (nav_script/highlight_script) — never a literal, still-live `<script>`
        // reconstructed from the user's code block content.
        assert!(!html.contains("<script>alert(1)</script>"));
    }

    #[test]
    fn hljs_css_derives_colors_from_theme_not_a_hardcoded_palette() {
        let base_colors = tasty_themes::mocha_fallback_colors();
        let mut alt_colors = base_colors.clone();
        // Only need one hue field to actually differ to prove `hljs_css` re-derives from
        // `Theme` on every call rather than baking in a fixed palette (e.g. a vendored
        // highlight.js theme like github.css would render identically regardless of this).
        alt_colors.mauve =
            tasty_type_appearance::color::HexColor::from_hex("#ff00ff").expect("valid hex literal");
        let base = Theme::with_colors_and_zoom(base_colors, false, 1.0);
        let alt = Theme::with_colors_and_zoom(alt_colors, false, 1.0);
        let base_css = hljs_css(&base);
        let alt_css = hljs_css(&alt);
        assert!(base_css.contains(".hljs-keyword"));
        assert_ne!(
            base_css, alt_css,
            "hljs token colors should follow the active theme, not a fixed palette"
        );
        assert!(alt_css.contains("#ff00ff"));
    }

    // ── copy button ──────────────────────────────────────────────────────────

    #[test]
    fn render_document_inlines_copy_button_only_when_code_block_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/snippet.md",
            source: "```rust\nfn main() {}\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        // `.tasty-copy-btn` itself is always in the document (it's a CSS rule in `theme_css`,
        // emitted unconditionally like every other selector) — the actual conditional signal is
        // whether the attach *script* (and its selector) got inlined at all.
        assert!(with_code.contains("#tasty-md-body pre > code"));

        let without_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo fenced blocks here.",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(!without_code.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn render_document_inlines_copy_button_for_unlabeled_code_block() {
        // No language token → pulldown-cmark emits `<pre><code>` with no `class` at all. The
        // gate condition (`<pre><code` substring, not `class="language-`) must still catch this
        // — `highlight`'s gate deliberately doesn't, but the copy button applies regardless of
        // whether the block has a recognized (or any) language.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/unlabeled.md",
            source: "```\nplain text, no language\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(html.contains("<pre><code>"));
        assert!(html.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn render_document_error_state_has_no_copy_button() {
        // 오류 상세 pre에는 code 자식이 없어 복사 버튼 대상이 아니다.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/broken.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(html.contains("tasty-state-detail"));
        assert!(!html.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn copy_button_script_is_scoped_to_body_code_blocks_only() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn copy_button_script_skips_mermaid_blocks_regardless_of_script_order() {
        // Mermaid는 비동기로 렌더하므로 클래스 검사로 복사 버튼 대상에서 제외한다.
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("code.classList.contains('language-mermaid')"));
    }

    #[test]
    fn copy_button_script_reads_text_content_not_inner_html() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("code.textContent"));
        assert!(!script.contains("code.innerHTML"));
    }

    #[test]
    fn copy_button_script_dedups_per_pre_against_repeat_attachment() {
        // Defense-in-depth against duplicate listeners: even though `reload_webview` always
        // replaces the whole document (so listeners can't literally accumulate across reloads —
        // see `render_document`'s module docs), an existing `.tasty-copy-btn` inside the same
        // `<pre>` still short-circuits the attach loop for that block.
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("pre.querySelector('.tasty-copy-btn')"));
    }

    #[test]
    fn copy_button_script_has_clipboard_api_with_exec_command_fallback() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("navigator.clipboard"));
        assert!(script.contains("navigator.clipboard.writeText"));
        assert!(script.contains("document.execCommand('copy')"));
    }

    #[test]
    fn copy_button_script_is_keyboard_focusable_with_aria_label() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("tabindex"));
        assert!(script.contains("aria-label"));
    }

    #[test]
    fn copy_button_script_wraps_attachment_in_try_catch_with_console_error_fallback() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
    }

    #[test]
    fn copy_button_css_uses_theme_tokens_not_hardcoded_colors() {
        let base_colors = tasty_themes::mocha_fallback_colors();
        let mut alt_colors = base_colors.clone();
        alt_colors.green =
            tasty_type_appearance::color::HexColor::from_hex("#00ff00").expect("valid hex literal");
        let base = Theme::with_colors_and_zoom(base_colors, false, 1.0);
        let alt = Theme::with_colors_and_zoom(alt_colors, false, 1.0);
        let base_css = theme_css(&base);
        let alt_css = theme_css(&alt);
        assert!(base_css.contains(".tasty-copy-btn"));
        assert!(base_css.contains(r#".tasty-copy-btn[data-state="copied"]"#));
        assert_ne!(
            base_css, alt_css,
            "copy button color should follow accent_success, not a fixed palette"
        );
    }

    // ── image load failure ───────────────────────────────────────────────────

    #[test]
    fn render_document_inlines_image_error_script_only_when_image_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_image = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/pic.md",
            source: "![alt text](missing.png)\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(with_image.contains("<img"));
        assert!(with_image.contains("#tasty-md-body img"));

        let without_image = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo images here.",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(!without_image.contains("#tasty-md-body img"));
    }

    #[test]
    fn render_document_error_state_has_no_image_error_script() {
        // Error/empty states never render an `<img>` at all, so the gate (`<img` substring on
        // `body_html`) must not fire for them.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/broken.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(!html.contains("#tasty-md-body img"));
    }

    #[test]
    fn image_error_script_is_scoped_to_body_images_only() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("#tasty-md-body img"));
    }

    #[test]
    fn image_error_script_checks_both_future_and_already_failed_images() {
        // Listener attachment alone misses images whose `error` event already fired before this
        // script ran (it executes near the end of the document) — `complete && naturalWidth===0`
        // catches those retroactively.
        let script = image_error_script(&Translator::default());
        assert!(script.contains("addEventListener('error'"));
        assert!(script.contains("img.complete&&img.naturalWidth===0"));
    }

    #[test]
    fn image_error_script_reads_src_attribute_not_property() {
        // DOM에서 URL로 해석한 src 프로퍼티 대신 src 속성값을 읽는다.
        let script = image_error_script(&Translator::default());
        assert!(script.contains("img.getAttribute('src')"));
        assert!(!script.contains("img.src"));
    }

    #[test]
    fn image_error_script_preserves_alt_as_aria_label() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("img.getAttribute('alt')"));
        assert!(script.contains("role','img'"));
        assert!(script.contains("aria-label"));
    }

    #[test]
    fn image_error_script_guards_against_duplicate_replacement() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("img.dataset.tastyImgChecked"));
        assert!(script.contains("img.dataset.tastyImgFailed"));
    }

    #[test]
    fn image_error_script_wraps_attachment_in_try_catch_with_console_error_fallback() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
    }

    #[test]
    fn image_error_css_uses_theme_tokens_not_hardcoded_colors() {
        let base_colors = tasty_themes::mocha_fallback_colors();
        let mut alt_colors = base_colors.clone();
        alt_colors.red =
            tasty_type_appearance::color::HexColor::from_hex("#00ff00").expect("valid hex literal");
        let base = Theme::with_colors_and_zoom(base_colors, false, 1.0);
        let alt = Theme::with_colors_and_zoom(alt_colors, false, 1.0);
        let base_css = theme_css(&base);
        let alt_css = theme_css(&alt);
        assert!(base_css.contains(".tasty-img-error"));
        assert!(base_css.contains(".tasty-img-error-icon"));
        assert_ne!(
            base_css, alt_css,
            "image error placeholder color should follow accent_danger, not a fixed palette"
        );
    }

    // ── math (KaTeX) ─────────────────────────────────────────────────────────

    #[test]
    fn render_document_inlines_katex_only_when_math_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_math = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/math.md",
            source: "Einstein: $E=mc^2$\n\n$$\\sum_{i=1}^n i$$\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(with_math.contains(r#"class="math math-inline""#));
        assert!(with_math.contains(r#"class="math math-display""#));
        assert!(with_math.contains("katex.render"));

        let without_math = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo math here, and no literal dollar signs either.",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(!without_math.contains("katex.render"));
    }

    #[test]
    fn render_document_preserves_original_latex_source_in_math_span() {
        // The HTML writer HTML-escapes the LaTeX source into the span's text content — this
        // locks in that pulldown-cmark's own default `ENABLE_MATH` output survives
        // `sanitize_html` unmangled (no custom event rewrite needed for this shape).
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/math.md",
            source: "$a < b$\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(html.contains("a &lt; b"), "got: {html}");
    }

    #[test]
    fn render_document_error_state_has_no_katex_script() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/broken.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(!html.contains("katex.render"));
    }

    #[test]
    fn katex_script_is_scoped_to_body_math_spans_only() {
        let script = katex_script();
        assert!(script.contains("#tasty-md-body .math-inline, #tasty-md-body .math-display"));
    }

    #[test]
    fn katex_script_sets_required_security_options_explicitly() {
        // Task requirement: throwOnError/trust must be explicitly false, not left as defaults.
        let script = katex_script();
        assert!(script.contains("throwOnError:false"));
        assert!(script.contains("trust:false"));
    }

    #[test]
    fn katex_script_reads_text_content_not_inner_html() {
        let script = katex_script();
        assert!(script.contains("el.textContent"));
        assert!(!script.contains("el.innerHTML"));
    }

    #[test]
    fn katex_script_distinguishes_inline_from_display_mode() {
        let script = katex_script();
        assert!(script.contains("el.classList.contains('math-display')"));
        assert!(script.contains("displayMode:display"));
    }

    #[test]
    fn katex_script_guards_against_duplicate_rendering() {
        let script = katex_script();
        assert!(script.contains("el.dataset.tastyMathRendered"));
    }

    #[test]
    fn katex_script_wraps_render_in_try_catch_with_console_error_fallback() {
        let script = katex_script();
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
    }

    #[test]
    fn katex_js_source_has_no_premature_script_close() {
        let js = katex_js_source();
        assert!(!js.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn katex_css_embeds_every_vendored_font_as_a_data_uri_with_no_leftover_relative_urls() {
        let css = katex_css_with_embedded_fonts();
        assert!(!css.contains("url(fonts/"), "leftover relative font url");
        assert_eq!(
            css.matches("data:font/woff2;base64,").count(),
            KATEX_FONTS.len()
        );
    }

    #[test]
    fn katex_css_sets_no_hardcoded_color_so_math_inherits_body_text_color() {
        // Locks in the "no extra theme wiring needed" claim in `katex_script`'s doc comment —
        // if a future re-vendor ever introduces a hardcoded `.katex{color:...}` rule, this fails
        // loudly instead of silently breaking dark/light theme following.
        let css = KATEX_CSS_RAW;
        assert!(
            !css.contains(".katex{color:") && !css.contains(".katex {color:"),
            "katex.min.css should rely on inherited `currentColor`, not set its own color"
        );
    }

    // ── callouts (GFM alerts + Obsidian extensions) ─────────────────────────

    #[test]
    fn gfm_alert_tags_render_their_respective_classes() {
        for (tag, class) in [
            ("NOTE", "markdown-alert-note"),
            ("TIP", "markdown-alert-tip"),
            ("IMPORTANT", "markdown-alert-important"),
            ("WARNING", "markdown-alert-warning"),
            ("CAUTION", "markdown-alert-caution"),
        ] {
            let source = format!("> [!{tag}]\n> body text\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(&format!(r#"class="{class}""#)),
                "[!{tag}] should render class={class}, got: {out}"
            );
        }
    }

    #[test]
    fn gfm_alert_tag_is_case_insensitive() {
        for tag in ["[!Note]", "[!NOTE]", "[!note]", "[!nOtE]"] {
            let source = format!("> {tag}\n> body\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(r#"class="markdown-alert-note""#),
                "{tag} should be recognized case-insensitively, got: {out}"
            );
        }
    }

    #[test]
    fn gfm_tag_with_trailing_text_becomes_an_obsidian_custom_title_callout() {
        let out = unsafe_content_html(
            "> [!NOTE] with trailing text\n> more\n",
            &Translator::default(),
        );
        assert!(out.contains(r#"class="markdown-alert-note""#), "got: {out}");
        assert!(
            out.contains(r#"data-label="with trailing text""#),
            "trailing text after the tag should become the custom title, got: {out}"
        );
        assert!(
            !out.contains("[!NOTE] with trailing text"),
            "the tag line itself must not leak into the body, got: {out}"
        );
    }

    #[test]
    fn plain_blockquote_without_alert_tag_is_unaffected() {
        // Regression: an ordinary `>` quote must render exactly as before — no class attribute
        // at all (pulldown-cmark's `Tag::BlockQuote(None)` emits an empty class_str) and no
        // `data-label`.
        let out = unsafe_content_html("> just a quote\n", &Translator::default());
        assert!(out.contains("<blockquote>"), "got: {out}");
        assert!(!out.contains("class="), "got: {out}");
        assert!(!out.contains("data-label"), "got: {out}");
    }

    #[test]
    fn raw_html_blockquote_spoofing_an_alert_class_gets_no_data_label() {
        // Raw HTML의 class만으로 자동 data-label을 추가하지 않는지 확인한다.
        // 입력에 직접 넣은 data-label을 차단하는 시험은 아니다.
        let source = "Intro\n\n\
<blockquote class=\"markdown-alert-note\">Spoofed trustworthy-looking note</blockquote>\n\n\
Outro\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            !out.contains("data-label"),
            "raw HTML without a data-label must not gain one automatically, got: {out}"
        );
        // Raw HTML을 버려서 검사에 통과한 것이 아닌지 함께 확인한다.
        assert!(
            out.contains(r#"<blockquote class="markdown-alert-note">Spoofed trustworthy-looking note</blockquote>"#),
            "expected the raw HTML to survive unmodified pre-sanitize, got: {out}"
        );

        // 같은 문서의 Markdown 콜아웃에는 제목을 붙인다.
        let mixed = format!("{source}\n> [!NOTE]\n> a real one\n");
        let mixed_out = unsafe_content_html(&mixed, &Translator::default());
        assert_eq!(
            mixed_out.matches("data-label").count(),
            1,
            "exactly the genuine alert should get a data-label, got: {mixed_out}"
        );
    }

    #[test]
    fn gfm_alert_data_label_uses_translator_and_survives_sanitize() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("en.toml"),
            "[markdown.alert]\nnote = \"Custom Note Label\"\n",
        )
        .unwrap();
        let tr = Translator::load(dir.path(), "en");

        let html = unsafe_content_html("> [!NOTE]\n> body\n", &tr);
        assert!(
            html.contains(r#"data-label="Custom Note Label""#),
            "got: {html}"
        );

        // Must also survive `sanitize_html`'s allowlist (blockquote now allows data-label).
        let sanitized = sanitize_html(&html);
        assert!(
            sanitized.contains(r#"data-label="Custom Note Label""#),
            "got: {sanitized}"
        );
        assert!(sanitized.contains(r#"class="markdown-alert-note""#));
    }

    #[test]
    fn obsidian_extended_types_render_their_classes() {
        for (tag, class) in [
            ("info", "markdown-alert-info"),
            ("abstract", "markdown-alert-abstract"),
            ("todo", "markdown-alert-todo"),
            ("success", "markdown-alert-success"),
            ("question", "markdown-alert-question"),
            ("failure", "markdown-alert-failure"),
            ("danger", "markdown-alert-danger"),
            ("bug", "markdown-alert-bug"),
            ("example", "markdown-alert-example"),
            ("quote", "markdown-alert-quote"),
        ] {
            // Bare (no fold marker, no title) — pulldown-cmark's own GFM scanner never
            // recognizes these 10 (not one of its 5 fixed names), so this exercises the
            // plain-blockquote/first-line-parsing half of `rewrite_callout_buffer`.
            let source = format!("> [!{tag}]\n> body text\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(&format!(r#"class="{class}""#)),
                "[!{tag}] should render class={class}, got: {out}"
            );
        }
    }

    #[test]
    fn obsidian_documented_aliases_resolve_to_their_canonical_kind() {
        for (alias, class) in [
            ("summary", "markdown-alert-abstract"),
            ("tldr", "markdown-alert-abstract"),
            ("hint", "markdown-alert-tip"),
            ("check", "markdown-alert-success"),
            ("done", "markdown-alert-success"),
            ("help", "markdown-alert-question"),
            ("faq", "markdown-alert-question"),
            ("fail", "markdown-alert-failure"),
            ("missing", "markdown-alert-failure"),
            ("error", "markdown-alert-danger"),
            ("cite", "markdown-alert-quote"),
        ] {
            let source = format!("> [!{alias}]\n> body\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(&format!(r#"class="{class}""#)),
                "[!{alias}] should alias to class={class}, got: {out}"
            );
        }
    }

    #[test]
    fn obsidian_aliases_never_shadow_a_gfm_kind_of_the_same_name() {
        // important와 caution을 지원하며 attention은 알 수 없는 태그로 남긴다.
        let important = unsafe_content_html("> [!important]\n> body\n", &Translator::default());
        assert!(
            important.contains(r#"class="markdown-alert-important""#),
            "got: {important}"
        );
        let caution = unsafe_content_html("> [!caution]\n> body\n", &Translator::default());
        assert!(
            caution.contains(r#"class="markdown-alert-caution""#),
            "got: {caution}"
        );
        // `attention` has no GFM entry of its own and isn't in `CALLOUT_ALIASES` either (kept
        // out deliberately, see `CALLOUT_KINDS` doc) — an unrecognized type must leave the
        // blockquote alone, literal tag text and all.
        let attention = unsafe_content_html("> [!attention]\n> body\n", &Translator::default());
        assert!(!attention.contains("markdown-alert-"), "got: {attention}");
        assert!(attention.contains("[!attention]"), "got: {attention}");
    }

    #[test]
    fn unrecognized_bracket_tag_is_left_as_a_plain_blockquote() {
        let out = unsafe_content_html("> [!not-a-real-type]\n> body\n", &Translator::default());
        assert!(!out.contains("markdown-alert-"), "got: {out}");
        assert!(!out.contains("<details"), "got: {out}");
        assert!(out.contains("[!not-a-real-type]"), "got: {out}");
    }

    #[test]
    fn fold_marker_plus_renders_initially_open_details() {
        let out = unsafe_content_html("> [!note]+\n> body\n", &Translator::default());
        assert!(
            out.contains("<details class=\"markdown-alert-note\" open>"),
            "got: {out}"
        );
        assert!(out.contains("<summary>"), "got: {out}");
        // No custom title given — falls back to the default label key (`Translator::default()`
        // has no loaded strings, so `t()`'s "key itself" miss-fallback is what surfaces here —
        // `gfm_alert_data_label_uses_translator_and_survives_sanitize` below covers the real,
        // translated label text via a loaded `Translator`).
        assert!(out.contains(">markdown.alert.note</summary>"), "got: {out}");
        assert!(out.contains("body"), "got: {out}");
    }

    #[test]
    fn fold_marker_minus_renders_initially_closed_details() {
        let out = unsafe_content_html("> [!warning]-\n> body\n", &Translator::default());
        assert!(
            out.contains("<details class=\"markdown-alert-warning\">"),
            "collapsed details must omit `open`, got: {out}"
        );
        assert!(!out.contains("markdown-alert-warning\" open"), "got: {out}");
    }

    #[test]
    fn no_fold_marker_never_uses_details_even_with_a_custom_title() {
        // Scope: "마커 없음 = 접기 UI 자체 없음" — fold and title are independent (matches
        // Obsidian's own docs), so a title with no `+`/`-` must still render as a plain
        // (non-foldable) blockquote, not `<details>`.
        let out = unsafe_content_html("> [!tip] Custom Title\n> body\n", &Translator::default());
        assert!(!out.contains("<details"), "got: {out}");
        assert!(
            out.contains(r#"class="markdown-alert-tip" data-label="Custom Title""#),
            "got: {out}"
        );
    }

    #[test]
    fn fold_marker_with_custom_title() {
        let out = unsafe_content_html(
            "> [!danger]+ Look out below\n> body\n",
            &Translator::default(),
        );
        assert!(
            out.contains("<details class=\"markdown-alert-danger\" open>"),
            "got: {out}"
        );
        assert!(out.contains(">Look out below</summary>"), "got: {out}");
    }

    #[test]
    fn nested_callouts_do_not_crash_and_keep_all_text() {
        // Scope: nested callouts don't need perfect styling, only crash/data-loss safety.
        let source = "> [!note] Outer\n\
> outer body\n\
> > [!warning]- Inner\n\
> > inner body\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains("markdown-alert-note"), "got: {out}");
        assert!(out.contains("markdown-alert-warning"), "got: {out}");
        assert!(out.contains("outer body"), "got: {out}");
        assert!(out.contains("inner body"), "got: {out}");
        // Must also survive sanitize + a full render_document() pass without panicking.
        let _ = sanitize_html(&out);
    }

    #[test]
    fn nested_plain_blockquote_inside_a_callout_is_unaffected() {
        let source = "> [!note]\n> intro\n> > just a nested quote\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains("markdown-alert-note"), "got: {out}");
        assert!(out.contains("just a nested quote"), "got: {out}");
    }

    #[test]
    fn alert_css_produces_distinct_rules_per_kind() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let css = alert_css(&theme);
        let mut blocks = Vec::new();
        for kind in CALLOUT_KINDS {
            let needle = format!(".{}{{", kind.class);
            assert!(
                css.contains(&needle),
                "missing rule block for {}, got: {css}",
                kind.class
            );
            // Icon/color rule covers both the `<blockquote>` shape (`::before`) and the
            // `<details>`/`<summary>` foldable shape (`>summary::before`) in one selector.
            assert!(
                css.contains(&format!(
                    ".{cls}::before,.{cls}>summary::before{{color:",
                    cls = kind.class
                )),
                "missing ::before color rule for {}, got: {css}",
                kind.class
            );
            assert!(
                css.contains("background-image:url(\"data:image/svg+xml,"),
                "missing baked icon data URI, got: {css}"
            );
            blocks.push(needle);
        }
        // Every kind (GFM 5 + Obsidian extensions) gets its own distinct selector (no accidental
        // collision/reuse) — icon/color duplication across kinds is expected and fine (module
        // doc), only the selector itself must stay unique per class.
        let unique: std::collections::HashSet<_> = blocks.iter().collect();
        assert_eq!(unique.len(), CALLOUT_KINDS.len(), "got: {css}");
    }

    #[test]
    fn alert_icon_data_uri_bakes_requested_color() {
        let uri = alert_icon_data_uri(tasty_icons::ALERT_TRIANGLE.body, false, "#ff0000");
        assert!(uri.starts_with("data:image/svg+xml,"));
        // percent-encoded `stroke="#ff0000"` — `#` and `"` are both escaped by
        // percent_encode_fragment, so check the decoded round-trip instead of raw substrings.
        let decoded = percent_decode(&uri["data:image/svg+xml,".len()..]);
        assert!(decoded.contains("stroke=\"#ff0000\""), "got: {decoded}");
    }

    // ── heading ids + TOC ────────────────────────────────────────────────────

    #[test]
    fn headings_get_unique_ids_across_all_levels() {
        let source = "# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n";
        let out = unsafe_content_html(source, &Translator::default());
        for (tag, text) in [
            ("h1", "One"),
            ("h2", "Two"),
            ("h3", "Three"),
            ("h4", "Four"),
            ("h5", "Five"),
            ("h6", "Six"),
        ] {
            let expected = format!(r#"<{tag} id="{}">{text}</{tag}>"#, text.to_lowercase());
            assert!(out.contains(&expected), "expected {expected:?}, got: {out}");
        }
    }

    #[test]
    fn duplicate_heading_text_gets_deduped_ids() {
        let source = "# Foo\n\n## Foo\n\n### Foo\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains(r#"<h1 id="foo">Foo</h1>"#), "got: {out}");
        assert!(out.contains(r#"<h2 id="foo-1">Foo</h2>"#), "got: {out}");
        assert!(out.contains(r#"<h3 id="foo-2">Foo</h3>"#), "got: {out}");
    }

    #[test]
    fn heading_slug_strips_markup_and_uses_plain_text_only() {
        let source = "# Hello `code` and [a link](x.md) and **bold**\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            out.contains(r#"id="hello-code-and-a-link-and-bold""#),
            "got: {out}"
        );
    }

    #[test]
    fn non_ascii_heading_gets_a_reasonable_slug() {
        let source = "# 한글 제목입니다\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            out.contains(r#"<h1 id="한글-제목입니다">한글 제목입니다</h1>"#),
            "got: {out}"
        );
    }

    #[test]
    fn all_punctuation_heading_falls_back_to_default_slug() {
        let source = "# !!! ??? ...\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains(r#"id="heading""#), "got: {out}");
    }

    #[test]
    fn heading_id_survives_sanitize() {
        let html = unsafe_content_html("# Title\n", &Translator::default());
        let sanitized = sanitize_html(&html);
        assert!(sanitized.contains(r#"id="title""#), "got: {sanitized}");
    }

    #[test]
    fn render_document_omits_toc_when_no_headings() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "Just a paragraph, no headings.\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        // `#tasty-toc{...}` still appears in the static `<style>` block regardless of
        // headings (theme_css isn't conditional) — assert on the actual `<nav>` element, not
        // the bare "tasty-toc" substring, or this would spuriously pass/fail on CSS presence.
        assert!(!html.contains(r#"<nav id="tasty-toc""#), "got: {html}");
    }

    #[test]
    fn render_document_includes_toc_with_matching_anchors_when_headings_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "# Intro\n\nSome text.\n\n## Details\n\nMore text.\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(html.contains(r#"id="tasty-toc""#), "got: {html}");
        assert!(html.contains(r##"href="#intro""##), "got: {html}");
        assert!(html.contains(r##"href="#details""##), "got: {html}");
        assert!(html.contains(r#"<h1 id="intro">Intro</h1>"#), "got: {html}");
        assert!(
            html.contains(r#"<h2 id="details">Details</h2>"#),
            "got: {html}"
        );
        // toggle button + collapsed-state hook present (collapsibility, task requirement).
        assert!(html.contains(r#"id="tasty-toc-toggle""#), "got: {html}");
        assert!(html.contains("tasty-toc-collapsed"), "got: {html}");
    }

    // base href가 있으면 #앵커도 다른 문서의 주소로 해석되므로 넣지 않는다.
    #[test]
    fn document_with_a_base_dir_still_carries_no_base_tag() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "# Intro\n\nSome text.\n",
            load_error: None,
            base_dir: Some(Path::new("/a")),
            recent: &[],
            remote: None,
        });
        assert!(!html.contains("<base"), "got: {html}");
        // 문서가 실제로 그려졌다는 것을 먼저 못박는다 — 빈 출력이면 위 부정은 공허하다.
        assert!(html.contains(r#"<h1 id="intro">Intro</h1>"#), "got: {html}");
    }

    /// 중복 제목·한글 제목·본문 내부 링크가 전부 **실재하는 id** 를 가리킨다. 목차 항목의
    /// raw href 와 heading id 를 값으로 대조한다(문자열 포함이 아니라 짝 맞춤).
    #[test]
    fn toc_hrefs_and_internal_links_match_real_ids_for_duplicate_and_korean_headings() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "# Start\n\n[내부 이동](#target)\n\n## Target\n\na\n\n## Target\n\nb\n\n## 한글 제목\n\nc\n",
            load_error: None,
            base_dir: Some(Path::new("/a")),
            recent: &[],
            remote: None,
        });
        for (href, id_tag) in [
            (r##"href="#start""##, r#"<h1 id="start">"#),
            (r##"href="#target""##, r#"<h2 id="target">"#),
            (r##"href="#target-1""##, r#"<h2 id="target-1">"#),
            (r##"href="#한글-제목""##, r#"<h2 id="한글-제목">"#),
        ] {
            assert!(html.contains(href), "missing {href} in: {html}");
            assert!(html.contains(id_tag), "missing {id_tag} in: {html}");
        }
        // 본문의 사용자 작성 내부 링크는 nav fragment 로 감싸이지 않고 그대로 남는다.
        assert!(
            !html.contains(&format!("{NAV_FRAGMENT_MARKER}link:%23target")),
            "internal anchor must not be routed through the host signal channel: {html}"
        );
    }

    /// 각주 참조/복귀 링크의 href 와 그 목적지 id 가 같은 철자로 짝을 이룬다 —
    /// [`nav_script`] 의 `getElementById` 가 그 철자를 그대로 쓴다.
    #[test]
    fn footnote_anchor_hrefs_pair_with_real_ids() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "# Start\n\nbody[^n]\n\n[^n]: note\n",
            load_error: None,
            base_dir: Some(Path::new("/a")),
            recent: &[],
            remote: None,
        });
        assert!(html.contains(r##"href="#fndef-n""##), "got: {html}");
        assert!(html.contains(r#"id="fndef-n""#), "got: {html}");
        assert!(html.contains(r##"href="#fnref-n""##), "got: {html}");
        assert!(html.contains(r#"id="fnref-n""#), "got: {html}");
    }

    /// 신뢰 스크립트가 fragment-only 앵커를 직접 스크롤하되 host 신호 채널
    /// (`#tasty-nav:`)은 건드리지 않는다.
    #[test]
    fn nav_script_scrolls_anchors_and_leaves_the_host_signal_channel_alone() {
        let js = nav_script("/a/doc.md");
        assert!(js.contains("scrollIntoView"), "got: {js}");
        assert!(js.contains("preventDefault"), "got: {js}");
        assert!(
            js.contains("getElementById(decodeURIComponent(id))"),
            "got: {js}"
        );
        // 마커는 Rust 상수에서 와야 한다 — 손으로 두 번 적으면 한쪽만 바뀐다.
        assert!(
            js.contains(&format!("'#'+\"{NAV_FRAGMENT_MARKER}\"")),
            "guard must be built from NAV_FRAGMENT_MARKER: {js}"
        );
    }

    #[test]
    fn toc_is_placed_between_addr_bar_and_body() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "# Intro\n\nSome text.\n",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        // Match the actual elements, not the bare id substrings — those also appear earlier,
        // in the static `<style>` block's `#tasty-addr-bar{...}`/`#tasty-md-body{...}`/
        // `#tasty-toc{...}` selectors, which would corrupt the ordering check.
        let addr_idx = html
            .find(r#"<div id="tasty-addr-bar""#)
            .expect("addr bar present");
        let toc_idx = html.find(r#"<nav id="tasty-toc""#).expect("toc present");
        let body_idx = html
            .find(r#"<div id="tasty-md-body""#)
            .expect("body present");
        assert!(
            addr_idx < toc_idx && toc_idx < body_idx,
            "expected addr bar < toc < body, got: {html}"
        );
    }

    // ── find-in-page (find bar + TreeWalker highlight script) ──────────────────

    #[test]
    fn render_document_always_embeds_find_bar_hidden_by_default() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "Just a plain paragraph, no headings.",
            load_error: None,
            base_dir: None,
            recent: &[],
            remote: None,
        });
        assert!(html.contains(r#"id="tasty-find-bar""#), "got: {html}");
        // hidden by default — the bar only appears on Ctrl+F, never on load.
        assert!(
            html.contains(r#"<div id="tasty-find-bar" role="search" hidden>"#),
            "got: {html}"
        );
        assert!(html.contains(r#"id="tasty-find-input""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-count""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-prev""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-next""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-close""#), "got: {html}");
    }

    #[test]
    fn find_bar_html_uses_translator_for_placeholder_and_tooltips() {
        let tr = Translator::default();
        let html = find_bar_html(&tr);
        assert!(
            html.contains(tr.t("markdown.find.placeholder")),
            "got: {html}"
        );
        assert!(
            html.contains(tr.t("markdown.find.prev_tooltip")),
            "got: {html}"
        );
        assert!(
            html.contains(tr.t("markdown.find.next_tooltip")),
            "got: {html}"
        );
        assert!(
            html.contains(tr.t("markdown.find.close_tooltip")),
            "got: {html}"
        );
    }

    #[test]
    fn find_script_excludes_code_blocks_from_the_tree_walker_scan() {
        let script = find_in_page_script(&Translator::default());
        assert!(
            script.contains("tagName==='PRE'||el.tagName==='CODE'"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_restores_dom_before_every_search_not_only_on_close() {
        let script = find_in_page_script(&Translator::default());
        assert!(
            script.contains("function clearHighlights()"),
            "got: {script}"
        );
        assert!(script.contains(".normalize()"), "got: {script}");
        assert!(
            script.contains("function runSearch(){\nclearHighlights();"),
            "got: {script}"
        );
        assert!(script.contains("function closeBar(){"), "got: {script}");
    }

    #[test]
    fn find_script_skips_search_while_ime_composing() {
        let script = find_in_page_script(&Translator::default());
        assert!(script.contains("compositionstart"), "got: {script}");
        assert!(script.contains("compositionend"), "got: {script}");
        assert!(script.contains("if(composing)return;"), "got: {script}");
    }

    #[test]
    fn find_script_handles_escape_enter_and_shift_enter_on_the_input() {
        let script = find_in_page_script(&Translator::default());
        assert!(script.contains("e.key==='Escape'"), "got: {script}");
        assert!(script.contains("e.key==='Enter'"), "got: {script}");
        assert!(
            script.contains("if(e.shiftKey)prev();else next();"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_debounces_input_before_searching() {
        let script = find_in_page_script(&Translator::default());
        assert!(
            script.contains("setTimeout(runSearch,150)"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_opens_on_ctrl_or_meta_f_and_prevents_default() {
        let script = find_in_page_script(&Translator::default());
        assert!(
            script.contains("(e.ctrlKey||e.metaKey)&&!e.altKey&&(e.key==='f'||e.key==='F')"),
            "got: {script}"
        );
        assert!(
            script.contains("e.preventDefault();\nopenBar();"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_escapes_query_as_a_regex_literal() {
        // Query is only ever used as a case-insensitive literal substring match — RegExp is
        // reused as the multi-match engine, not exposed as a user-facing regex feature.
        let script = find_in_page_script(&Translator::default());
        assert!(script.contains("function escapeRegExp(s)"), "got: {script}");
        assert!(
            script.contains("new RegExp(escapeRegExp(query),'gi')"),
            "got: {script}"
        );
    }

    #[test]
    fn theme_css_derives_find_highlight_colors_from_theme_not_hardcoded() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let css = theme_css(&theme);
        assert!(css.contains("mark.tasty-find-hit{"), "got: {css}");
        assert!(
            css.contains(&format!(
                "background:{};",
                theme
                    .accent_warning()
                    .with_alpha(FIND_HIT_BG_ALPHA)
                    .to_hex()
            )),
            "got: {css}"
        );
        assert!(
            css.contains(&format!("background:{};", theme.accent_primary().to_hex())),
            "got: {css}"
        );
    }

    // ── 로컬 이미지 인라인 (sanitize 뒤, 문서 트리로 범위를 좁힌 자리) ──────────

    /// `<img src="...">` 하나짜리 본문 — sanitize 를 거친 형태를 그대로 흉내낸다.
    fn img(src: &str) -> String {
        format!(r#"<p><img src="{src}" alt="x"></p>"#)
    }

    /// `src="..."` 값을 뽑는다. 속성이 아예 없으면 `None`.
    fn src_of(html: &str) -> Option<String> {
        let i = html.find(" src=\"")? + 6;
        let j = html[i..].find('"')?;
        Some(html[i..i + j].to_string())
    }

    fn tree_with_image() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("pic.png");
        // 확장자 판정을 확인하기 위한 파일이며 유효한 PNG 데이터는 아니다.
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n-not-a-real-png-").unwrap();
        (dir, png)
    }

    #[test]
    fn inlines_a_relative_image_inside_the_document_tree() {
        let (dir, _) = tree_with_image();
        let out = inline_local_images(&img("pic.png"), Some(dir.path()));
        let src = src_of(&out).expect("src 가 남아야 한다");
        assert!(src.starts_with("data:image/png;base64,"), "got: {src}");
        // 원본 경로는 문서에 안 남는다 — 남으면 백엔드가 그것을 따로 로드할 수 있다.
        assert!(!out.contains("pic.png"), "got: {out}");
    }

    #[test]
    fn inlines_a_scheme_less_absolute_path_inside_the_tree() {
        let (dir, png) = tree_with_image();
        let out = inline_local_images(&img(&png.to_string_lossy()), Some(dir.path()));
        assert!(
            src_of(&out).unwrap().starts_with("data:image/png;base64,"),
            "got: {out}"
        );
    }

    #[test]
    fn inlines_a_raw_html_img_the_same_way() {
        let (dir, _) = tree_with_image();
        // raw HTML `<img>` 는 sanitize 를 그대로 통과한다(상대 URL 정책) — 그래서 이
        // 함수가 보는 형태도 위와 같다. 두 경로가 갈리지 않는다는 것을 못박는다.
        let body = r#"<p>before</p><img src="pic.png"><p>after</p>"#;
        let out = inline_local_images(body, Some(dir.path()));
        assert!(
            src_of(&out).unwrap().starts_with("data:image/png;base64,"),
            "got: {out}"
        );
        assert!(out.contains("<p>before</p>") && out.contains("<p>after</p>"));
    }

    #[test]
    fn refuses_an_image_outside_the_document_tree() {
        let (dir, _) = tree_with_image();
        let outside = tempfile::tempdir().unwrap();
        let out_png = outside.path().join("out.png");
        std::fs::write(&out_png, b"\x89PNG\r\n\x1a\n").unwrap();
        // 절대 경로로도, `..` 로도 못 나간다.
        for src in [
            out_png.to_string_lossy().to_string(),
            format!(
                "../{}/out.png",
                outside.path().file_name().unwrap().to_string_lossy()
            ),
        ] {
            let out = inline_local_images(&img(&src), Some(dir.path()));
            assert_eq!(src_of(&out), None, "src={src} 에서 src 가 남았다: {out}");
            assert!(
                !out.contains("out.png"),
                "src={src} 에서 경로가 샜다: {out}"
            );
            // ★ img 요소 자체는 남아야 한다. 이 줄이 없으면 위 부정 둘은 출력이
            // 통째로 비었을 때도 통과하고, 그 초록의 뜻은 "거절했다" 가 아니라
            // "아무것도 안 남았다" 다.
            assert!(out.contains("<img"), "img 가 통째로 사라졌다: {out}");
        }
    }

    #[test]
    fn refuses_a_symlink_that_escapes_the_tree() {
        let (dir, _) = tree_with_image();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret.png");
        std::fs::write(&target, b"\x89PNG\r\n\x1a\n").unwrap();
        let link = dir.path().join("link.png");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(not(unix))]
        return;
        // 판정을 `canonicalize` 뒤에 하는 이유가 이것이다 — 이름만 트리 안이다.
        let out = inline_local_images(&img("link.png"), Some(dir.path()));
        assert_eq!(src_of(&out), None, "got: {out}");
    }

    #[test]
    fn leaves_remote_srcs_untouched() {
        let (dir, _) = tree_with_image();
        for src in ["http://example.com/a.png", "https://example.com/a.png"] {
            let out = inline_local_images(&img(src), Some(dir.path()));
            assert_eq!(src_of(&out).as_deref(), Some(src), "got: {out}");
        }
    }

    #[test]
    fn refuses_an_extension_outside_the_image_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"secret").unwrap();
        let out = inline_local_images(&img("notes.txt"), Some(dir.path()));
        assert_eq!(src_of(&out), None, "got: {out}");
        assert!(!out.contains("secret"), "파일 내용이 실렸다: {out}");
    }

    #[test]
    fn refuses_an_image_over_the_per_image_cap() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.png");
        std::fs::write(&big, vec![0u8; MAX_INLINE_IMAGE_BYTES as usize + 1]).unwrap();
        let out = inline_local_images(&img("big.png"), Some(dir.path()));
        assert_eq!(src_of(&out), None, "got len {}", out.len());
    }

    #[test]
    fn stops_inlining_when_the_document_budget_is_spent() {
        let dir = tempfile::tempdir().unwrap();
        // 상한 딱 아래짜리 넷 = 총합 상한을 넘긴다. 앞쪽은 실리고 뒤쪽은 안 실린다.
        let each = MAX_INLINE_IMAGE_BYTES;
        let n = (MAX_INLINE_TOTAL_BYTES / each) as usize + 1;
        let mut body = String::new();
        for i in 0..n {
            let name = format!("i{i}.png");
            std::fs::write(dir.path().join(&name), vec![0u8; each as usize]).unwrap();
            body.push_str(&img(&name));
        }
        let out = inline_local_images(&body, Some(dir.path()));
        let inlined = out.matches("data:image/png;base64,").count();
        assert!(inlined < n, "예산을 안 지켰다: {inlined}/{n}");
        assert!(inlined > 0, "하나도 안 실렸다 — 예산 계산이 뒤집혔다");
    }

    #[test]
    fn no_base_dir_means_nothing_local_is_inlined() {
        let out = inline_local_images(&img("pic.png"), None);
        assert_eq!(src_of(&out), None, "got: {out}");
    }

    // 기준 폴더 밖의 파일 경로를 인라인하지 않는지 확인한다.
    #[test]
    fn no_src_survives_that_is_neither_remote_nor_inlined() {
        let (dir, png) = tree_with_image();
        let outside = tempfile::tempdir().unwrap();
        // ★ 트리 밖 파일은 **실재해야** 한다. 없는 파일로 적으면 `canonicalize` 가
        // 먼저 실패해서, 범위 판정을 통째로 지워도 이 시험이 초록으로 남는다.
        let outside_png = outside.path().join("x.png");
        std::fs::write(&outside_png, b"\x89PNG\r\n\x1a\n").unwrap();
        let escape_rel = format!(
            "../{}/x.png",
            outside.path().file_name().unwrap().to_string_lossy()
        );
        let mut body = String::new();
        for src in [
            "pic.png".to_string(),
            png.to_string_lossy().to_string(),
            outside_png.to_string_lossy().to_string(),
            escape_rel,
            "missing.png".to_string(),
            "https://example.com/a.png".to_string(),
        ] {
            body.push_str(&img(&src));
        }
        let out = inline_local_images(&body, Some(dir.path()));
        let mut rest = out.as_str();
        let mut seen = 0;
        while let Some(i) = rest.find(" src=\"") {
            let v = &rest[i + 6..];
            let j = v.find('"').unwrap();
            let value = &v[..j];
            assert!(
                value.starts_with("http://")
                    || value.starts_with("https://")
                    || value.starts_with("data:image/"),
                "허용하지 않은 src가 남았다: {value}"
            );
            seen += 1;
            rest = &v[j..];
        }
        // 남은 src 는 원격 하나 + 인라인 둘이어야 한다 — 0 이면 위 단정이 한 번도
        // 안 돌고 초록이 된다.
        assert_eq!(seen, 3, "got: {out}");
    }

    // ── sanitize 단계의 스킴 판정 (허용목록을 값으로 못박는다) ──────────────────

    #[test]
    fn sanitize_keeps_only_http_https_mailto_schemes() {
        for (src, kept) in [
            ("https://example.com/a.png", true),
            ("http://example.com/a.png", true),
            ("data:image/png;base64,AAAA", false),
            ("file:///etc/passwd", false),
            ("javascript:alert(1)", false),
        ] {
            let out = sanitize_html(&format!(r#"<img src="{src}" alt="a">"#));
            // ★ img 가 살아남았다는 것을 먼저 못박는다(빈 출력이면 아래가 무의미하다).
            assert!(
                out.contains("<img"),
                "src={src} 에서 img 가 사라졌다: {out}"
            );
            assert_eq!(
                out.contains(src),
                kept,
                "src={src} 의 판정이 바뀌었다: {out}"
            );
        }
    }

    /// 스킴 **없는** 경로는 위 허용목록과 무관하게 통과한다 — 그것이 로컬 이미지가
    /// 인라이너까지 도달하는 경로다. 이 줄이 깨지면 인라이너는 볼 것이 없어진다.
    #[test]
    fn sanitize_keeps_scheme_less_paths_for_the_inliner() {
        for src in ["local.svg", "/tmp/x/local.svg", "./sub/a.png"] {
            let out = sanitize_html(&format!(r#"<img src="{src}" alt="a">"#));
            assert!(out.contains(src), "src={src} 가 사라졌다: {out}");
        }
    }
}
