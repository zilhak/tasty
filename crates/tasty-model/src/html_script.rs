//! html surface 문서의 스크립트 감지 결과와 문서 단위 허용 상태.
//!
//! 규칙은 ADR-0053을 따른다. 허용은 main frame 문서의 URL(fragment 제외)과
//! 로드 때 구한 파일 전체 지문에 묶이고, 다른 main frame 문서가 commit되면 풀린다.
//! backend의 navigation 콜백이 아래 `on_*` 메서드를 순서대로 부르고 돌려받은 값으로 JS를 켜거나 끈다.

/// 문서 원본 전체의 SHA-256 지문.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

/// 원본 스캔으로 판정한 실행 가능한 스크립트의 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptDetection {
    /// 실행 가능한 스크립트가 없다. 상한 뒤에만 있는 경우도 여기에 속한다.
    #[default]
    None,
    /// 로컬 또는 inline 스크립트, 이벤트 핸들러 속성, `javascript:` URL이 있다.
    Scripts,
    /// `http(s)` `src` 스크립트만 있다. 원격 콘텐츠가 차단돼 있으면 허용해도 실행되지 않는다.
    ScriptsRemoteOnly,
}

impl ScriptDetection {
    pub fn has_scripts(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// 한 번의 원본 읽기로 구한 감지 결과와 지문.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptScan {
    pub fingerprint: Fingerprint,
    pub detection: ScriptDetection,
}

/// 마지막 main frame commit에서 기록한 화면 문서.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRecord {
    /// fragment를 뺀 URL.
    pub url: String,
    /// 응답 단계 없이 commit된 문서나 `file://`이 아닌 문서는 `None`이다. 이 문서는 허용할 수 없다.
    pub scan: Option<ScriptScan>,
}

/// 허용 기록. 허용 클릭 때의 현재 문서를 그대로 옮긴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowance {
    pub url: String,
    pub fingerprint: Fingerprint,
}

/// 응답 단계의 결과. 로드 시작 때 비우고 commit에서 소비한다.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingResponse {
    url: String,
    scan: Option<ScriptScan>,
    matched: bool,
}

/// 배너 표시에 필요한 문서 단위 표지. 표시 판단은 호스트가 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BannerFlags {
    /// 감지 결과는 있지만 사용자가 아직 이 문서를 보지 않았다.
    pub pending_view: bool,
    /// 이 문서에서 배너를 띄웠다.
    pub shown: bool,
    /// 사용자가 이 문서의 배너를 닫았다.
    pub dismissed: bool,
}

/// 허용할 수 없는 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowError {
    /// 아직 commit된 문서가 없다.
    NoDocument,
    /// 현재 문서의 지문이 없다(`file://`이 아니거나 응답 단계 없이 commit됐다).
    NoFingerprint,
}

impl std::fmt::Display for AllowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDocument => f.write_str("no document has been committed yet"),
            Self::NoFingerprint => f.write_str(
                "the current document has no content fingerprint (not a file:// document, \
                 or it was committed without a response decision)",
            ),
        }
    }
}

/// surface 하나의 스크립트 허용 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlScriptState {
    /// 전역 "Sandbox scripts" 설정. 꺼져 있으면 모든 문서에서 JS를 켠다.
    sandbox: bool,
    current: Option<DocumentRecord>,
    allowance: Option<Allowance>,
    pending: Option<PendingResponse>,
    /// 마지막 로드 시작 뒤 끝나지 않았다.
    load_in_flight: bool,
    /// 마지막 로드 시작 뒤 main frame commit이 있었다.
    committed_since_start: bool,
    /// 허용 직후 호스트가 같은 문서를 다시 로드해야 한다.
    reload_requested: bool,
    banner: BannerFlags,
}

impl Default for HtmlScriptState {
    fn default() -> Self {
        Self::new(true)
    }
}

/// fragment를 뺀 URL. 같은 문서인지 비교할 때 쓴다.
pub fn strip_fragment(url: &str) -> &str {
    url.split_once('#').map_or(url, |(base, _)| base)
}

impl HtmlScriptState {
    pub fn new(sandbox: bool) -> Self {
        Self {
            sandbox,
            current: None,
            allowance: None,
            pending: None,
            load_in_flight: false,
            committed_since_start: false,
            reload_requested: false,
            banner: BannerFlags::default(),
        }
    }

    pub fn sandbox(&self) -> bool {
        self.sandbox
    }

    pub fn current(&self) -> Option<&DocumentRecord> {
        self.current.as_ref()
    }

    pub fn allowance(&self) -> Option<&Allowance> {
        self.allowance.as_ref()
    }

    pub fn banner(&self) -> BannerFlags {
        self.banner
    }

    pub fn banner_mut(&mut self) -> &mut BannerFlags {
        &mut self.banner
    }

    /// 현재 문서의 감지 결과. 문서나 지문이 없으면 `None`.
    pub fn current_detection(&self) -> Option<ScriptDetection> {
        self.current.as_ref()?.scan.map(|s| s.detection)
    }

    /// 현재 문서가 허용 기록과 같은 문서인지.
    pub fn current_is_allowed(&self) -> bool {
        match (&self.current, &self.allowance) {
            (Some(doc), Some(allow)) => doc
                .scan
                .is_some_and(|s| doc.url == allow.url && s.fingerprint == allow.fingerprint),
            _ => false,
        }
    }

    /// 지금 webview에 적용할 JS 값.
    ///
    /// 로드가 진행 중이고 아직 commit 전이면 응답 단계의 판단(없으면 끔)을 따른다.
    /// 그렇지 않으면 화면 문서의 허용 여부를 따른다.
    pub fn effective_js(&self) -> bool {
        if !self.sandbox {
            return true;
        }
        if self.load_in_flight && !self.committed_since_start {
            return self.pending.as_ref().is_some_and(|p| p.matched);
        }
        self.current_is_allowed()
    }

    /// 전역 sandbox 설정을 바꾸고 적용할 JS 값을 돌려준다.
    pub fn set_sandbox(&mut self, sandbox: bool) -> bool {
        self.sandbox = sandbox;
        self.effective_js()
    }

    /// main frame 로드가 시작됐다. 응답 단계의 이전 결과를 비운다.
    pub fn on_load_started(&mut self) -> bool {
        self.pending = None;
        self.load_in_flight = true;
        self.committed_since_start = false;
        self.effective_js()
    }

    /// main frame main resource의 응답 결정. `scan`은 `file://` 문서에서만 있다.
    pub fn on_main_response(&mut self, url: &str, scan: Option<ScriptScan>) -> bool {
        let url = strip_fragment(url).to_string();
        let matched = match (&self.allowance, scan) {
            (Some(allow), Some(scan)) => allow.url == url && allow.fingerprint == scan.fingerprint,
            _ => false,
        };
        self.pending = Some(PendingResponse { url, scan, matched });
        // 응답 단계가 로드 시작 신호보다 먼저 올 수 없지만, 신호를 놓쳐도 이 로드의 판단을 따른다.
        self.load_in_flight = true;
        self.effective_js()
    }

    /// main frame 문서가 commit됐다. 응답 단계 결과를 소비해 현재 문서로 기록한다.
    ///
    /// 응답 단계가 없었으면(bfcache 복원 등) `committed_url`로 지문 없는 문서를 기록한다.
    pub fn on_committed(&mut self, committed_url: Option<&str>) -> bool {
        let pending = self.pending.take();
        let matched = pending.as_ref().is_some_and(|p| p.matched);
        if !matched {
            self.allowance = None;
        }
        let record = match pending {
            Some(p) => Some(DocumentRecord {
                url: p.url,
                scan: p.scan,
            }),
            None => committed_url.map(|u| DocumentRecord {
                url: strip_fragment(u).to_string(),
                scan: None,
            }),
        };
        let same_document = matches!(
            (&self.current, &record),
            (Some(a), Some(b)) if a == b
        );
        self.current = record;
        if !same_document {
            self.banner = BannerFlags::default();
        }
        self.committed_since_start = true;
        self.effective_js()
    }

    /// main frame 로드가 끝났다. commit 없이 끝났으면 화면 문서의 허용 상태로 되돌릴 값을 준다.
    pub fn on_load_finished(&mut self) -> Option<bool> {
        let restore = self.load_in_flight && !self.committed_since_start;
        self.load_in_flight = false;
        self.pending = None;
        restore.then(|| self.effective_js())
    }

    /// 현재 문서를 허용으로 기록하고 재로드를 요청한다. 파일을 다시 읽지 않는다.
    pub fn allow_current(&mut self) -> Result<(), AllowError> {
        let doc = self.current.as_ref().ok_or(AllowError::NoDocument)?;
        let scan = doc.scan.ok_or(AllowError::NoFingerprint)?;
        self.allowance = Some(Allowance {
            url: doc.url.clone(),
            fingerprint: scan.fingerprint,
        });
        self.reload_requested = true;
        Ok(())
    }

    /// 허용 직후의 재로드 요청을 한 번 꺼낸다.
    pub fn take_reload_request(&mut self) -> bool {
        std::mem::take(&mut self.reload_requested)
    }
}

#[cfg(test)]
mod tests;
