//! 파일 복사·이동·휴지통·되돌리기 엔진. 화면과 무관하게 경로를 받아 결과를 돌려준다.
//! 로컬 explorer worker 와 원격 explorer 요청을 받는 서버가 함께 쓴다.
pub(crate) mod job;
pub(crate) mod named;
pub(crate) mod ops;
// Properties 권한 편집(GUI popup)만 쓴다. 원격 explorer 요청에는 권한 변경이 없어 헤드리스 빌드에서 뺀다.
#[cfg(feature = "gui")]
pub(crate) mod permissions;
pub(crate) mod remote;
