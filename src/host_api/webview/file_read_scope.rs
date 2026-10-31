//! 로컬 `file://` 문서가 읽을 수 있는 범위. macOS WKWebView 는 `loadFileURL:allowingReadAccessToURL:`
//! 로 범위를 받고 그 밖의 하위 리소스를 알림 없이 거부한다. 계약은 docs/surfaces/html/index.md "OS별 적용".

/// 문서 경로와 관계없이 파일 시스템 루트를 돌려준다. 문서 폴더 밖의 상대 CSS·이미지·링크도 읽어야 하고,
/// Linux·Windows WebView 에도 폴더 범위 제한이 없다. 스크립트 실행은 스크립트 게이트, 외부 전송은
/// 원격 차단이 따로 막는다.
pub(crate) fn read_access_root(_document_path: &str) -> &'static str {
    "/"
}

#[cfg(test)]
mod tests {
    use super::read_access_root;

    #[test]
    fn any_document_reads_from_filesystem_root() {
        for path in [
            "/Users/me/plan/layout/components/index.html",
            "/Volumes/ext/doc.html",
            "/tmp/a/index.html",
            "/index.html",
            "/Users/me/한글 폴더/문서.html",
            "",
        ] {
            assert_eq!(read_access_root(path), "/", "{path}");
        }
    }
}
