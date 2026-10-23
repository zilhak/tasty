# HTML entry 전송 계약

attach 서버의 HTML 전송과 client entry viewer를 설명한다. Linux GUI client는 파일 entry 본문을 받아 별도 native context에서 표시한다. 하위 리소스와 HTTP(S) 실행은 지원하지 않으며 client 파일·네트워크로 대체하지 않는다. [HTML surface](../surfaces/html/index.md), [attach 동작](attach-behavior.md)을 함께 본다.

## source와 협상

`RemoteSurface`는 생성 인자와 복원 snapshot의 URL을 초기 source로 사용한다. `webview.set_url`은 GUI/headless 공통으로 최신 요청을 기록하고 source가 달라질 때 generation을 증가시킨다. 초기 생성의 경로와 plugin이 확인해 준 같은 file URL은 같은 source이며 generation을 유지한다. HTML plugin의 `html.open`도 이 경로를 지난다. snapshot의 생성·복원 URL은 덮어쓰지 않는다. native commit URL은 GUI 관측 사본이며 최신 요청 source를 덮지 않는다. 페이지 안 탐색·GUI 주소 띠의 native 경로를 요청 source와 혼합하지 않는다.

`HtmlSource`는 File/Http/Inline/Unsupported다. file URL은 percent decoding 뒤 해당 서버 OS의 절대 경로로 해석한다. 문자열 DTO로 손실 없이 표현할 수 없는 경로는 Unsupported로 거절한다. File source는 읽기 경로와 정규화한 file URL을 따로 보관하여 query·fragment를 잃지 않는다. 둘 다 서버의 주소이며 client가 자기 파일 URI로 직접 탐색하지 않는다. HTTP(S)·raw HTML은 파일로 읽지 않는다. 생성·복원 인자는 파일 경로를 받을 수 있지만, `webview.set_url`의 주소가 아닌 문자열은 native 로드 규칙에 따라 일반 텍스트도 Inline으로 기록한다. 파일 source의 부모를 root·cwd로 사용하고, 그 외 source는 상속 cwd를 보관한다.

`STREAM_PROTO=1`, 기존 tag와 `MAX_FRAME_LEN=1MiB`는 유지한다. workspace descriptor는 `ipc.stream.html-transfer-v1`을 광고한다. 새 client는 이 광고가 있는 연결에만 `html_capabilities`로 같은 capability를 선언하고 서버는 해당 registration에 지원 여부를 기록한다. 생략하면 구 peer다. 서버 광고만으로 새 메시지를 보내지 않는다. `ipc.stream.html-entry-v1`은 entry viewer capability다. 서버는 두 capability를 광고하고, Linux GUI client는 둘 다 선언한다. 서버는 두 선언을 모두 받은 registration에만 자신이 소유한 `com.tasty.html` leaf의 `html` role과 source metadata를 보낸다. 구 peer·미지원 client·다른 plugin의 동명 kind에는 placeholder를 유지한다. terminal·markdown 경로는 기존 방식이다.

## metadata, allowance와 credit

wire DTO는 `crates/tasty-ipc/src/html_transfer.rs`에 있다. 모든 요청/응답은 Control JSON이며 각 응답을 직렬화한 뒤 전체 payload 상한을 검사한다.

1. `html_entry_request`는 서버 surface ID와 transfer identity를 받는다. client가 파일 경로를 지정하는 필드는 없다. 서버가 현재 HTML source를 결정한다.
2. worker가 정규 파일 여부·크기·root를 열린 handle로 확인하고 `html_metadata`를 보낸다. metadata에는 typed source, source generation, root, cwd, size, regular가 있다. HTTP source는 regular=false metadata 뒤 proxy 미지원 실패로 끝나며 HTTP 실행이나 client network fallback이 없다. 파일 종류·경로 확인 실패는 oversized로 분류하지 않는다.
3. `html_read`는 정확한 다음 raw offset과 allowance를 보낸다. 100000 bytes 이하는 별도 크기 허용 없이 credit을 발급한다. 초과 파일은 metadata 뒤 Allow를 기다린다. Decline은 cancel이다. 처음 보낼 때도 metadata와 body를 분리한다.
4. credit 하나는 최대 48KiB raw bytes의 base64 `html_chunk` 하나를 허용한다. 마지막 chunk 뒤 `html_complete`가 온다. 빈 파일은 credit 뒤 complete만 온다. 큰 본문 전체를 JSON 하나로 만들지 않는다.
5. `html_cancel`, source 교체·surface 수명 종료·connection retirement·30초 credit 무응답은 worker의 handle과 buffer를 해제한다. 취소는 credit queue를 기다리지 않고 atomic 표지를 설정한다. 명시적 취소는 `html_cancelled`, 파일 판 변경이나 timeout은 `html_failure`다. 끊긴 registration에 대한 응답을 새 registration으로 넘기지 않는다.

entry는 attach workspace holder만 요청한다. markdown 조회와 같은 SSH 신뢰 경계와 engine 전체 조회 범위를 사용하며 새 permission token은 없다. 원 grant와 StreamHub registration을 요청 소유권에 기록하고 후속 read에서도 검사한다. transfer 번호는 한 connection에서 단조 증가해야 하며 종료된 identity를 재사용하지 않는다. 동시에 살아 있는 worker는 process 전체에서 최대 8개이며 engine별 map도 8개로 제한한다. worker command queue는 1개다. engine 소멸 때는 취소를 요청하고 미완 thread의 join handle을 bounded retirement 목록으로 옮긴다. 해당 슬롯은 실제 join 뒤에만 재사용한다. 파일 시스템 읽기가 반환하지 않으면 취소 완료로 보고하지 않으며 새 요청의 입장 예산도 반환하지 않는다. 중복 credit·offset 오류는 전송을 중단한다. 출력 queue 손실 때는 원 registration을 끊어 불완전 본문을 정상 완료로 처리하지 않는다.

## 파일 경계

`src/remote/html_files.rs`와 `src/remote/html_transfer.rs`가 파일 경로 확인과 읽기를 worker에서 수행한다. entry root 밖 symlink·directory·FIFO·device는 거절한다. client는 절대 하위경로나 percent-encoded traversal을 요청 경로로 보낼 수 없다. 서버의 `html_resource_request`는 최초 entry identity·source generation과 decoded root 상대 경로를 받는다. 이 채널은 client native 리소스 표시 기능과 별개다. 현재 viewer는 entry만 표시하며 하위 리소스 capability를 광고하지 않는다. 서버는 원 entry가 연 directory handle을 문서 수명 동안 보관하고 모든 resource를 그 root에서 연다. 문서 root는 engine당 최대256개이며 새 entry·원 grant/registration/source 소멸·entry cancel로 이전 root와 하위 worker를 취소한다. 상대 경로의 빈 component·`.`·`..`·절대 경로·backslash·NUL은 거절하고, root 안 symlink도 canonical target이 원 root 안에 남고 no-follow 상대 open이 성공할 때만 읽는다. 각 credit 전후 원 entry와 resource 모두 변경되지 않았는지 검사한다.

Unix는 `/`부터 각 root component를 directory handle의 `openat`·`O_NOFOLLOW`로 열어 최초 directory handle과 device/inode를 보관한다. canonical target이 그 root 안에 있는지 확인한 뒤 파일도 최초 directory handle 기준으로 연다. 이후 source 검사에서 새 root를 인가하지 않는다. 최종 파일도 no-follow·nonblocking으로 열고 handle metadata로 정규 파일 여부를 다시 검사한다. canonicalize 이후 경로가 symlink로 교체되면 열지 않는다. root 안을 가리키는 symlink는 canonical target을 통해 읽을 수 있다. Windows는 canonical root를 `NtCreateFile`의 `OBJ_DONT_REPARSE`로 열어 root와 ancestor의 junction 교체를 거절하고, 최초 root handle과 volume/file identity를 전송 수명 동안 유지한다. entry는 그 handle을 `RootDirectory`로 지정한 상대 경로로 열며 모든 component의 reparse를 거절한다. 각 source 검사도 같은 root identity를 확인하고 최초 handle에서 파일을 다시 연다. 교체된 root/file의 최종 pathname끼리 비교하여 새 root를 인가하지 않는다. 기존 handle의 소유권·identity 조회 방식은 유지한다. [OBJECT_ATTRIBUTES](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes)와 [NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile)의 root-relative·no-reparse 계약을 사용한다.

각 credit 전후에 handle의 크기·시각·파일 identity와 현재 source 경로를 다시 확인한다. rename 교체나 제자리 변경이 감지되면 기존 크기 승인을 재사용하지 않고 실패시킨다. Unix ctime도 비교한다. 파일시스템이 identity·모든 시각을 보존한 비정상 변경까지 내용 snapshot으로 고정하는 계약은 아니다. 완료 전에는 client가 받은 prefix를 렌더 본문으로 제공하지 않는다.

## client 문서 모델

`tasty-remote::html_document`는 session epoch, remote surface, local binding, document generation, request, transfer를 각각 비교한다. 생성하지 않은 요청이나 다른 identity의 늦은 응답은 버린다. 중복·누락·범위 초과 chunk와 크기가 다른 complete는 해당 resource를 실패시키고 본문을 지운다. reader queue의 retained bytes에도 HTML 문자열을 포함한다.

개별 자동 기준은 100000 bytes, 문서 누적 기준은 2000000 bytes인 decimal 상수다. 누적은 수신한 raw body bytes이며 이미 발급한 credit도 예약량으로 합산한다. 누적 경계에서 자동 credit을 멈추고 Continue를 기다린다. Continue는 그 문서의 누적 정지만 해제하며 개별 큰 파일 Allow는 별개다. 크기 질문은 FIFO queue로 하나씩 소비한다. Decline은 해당 요청만 종료하고 skipped를 증가시킨다. Stop은 나머지 요청을 종료한다.

completed body만 `Resource::body`로 제공한다. 완료 시 수신 buffer의 소유권을 immutable 공유 할당으로 옮긴다. `html_body::ReceivedEntry`는 그 할당에서 기존 스캐너로 감지·전체 지문을 구하고, native 응답용 MIME·원격 source URL·여섯 부분 transfer identity와 묶는다. 검사가 끝난 본문은 같은 identity의 `EntryBodyStore`에 한 번만 게시한다. native용 가상 entry URL은 원격 파일 경로를 포함하지 않으며 정확히 그 entry만 조회한다. 상대 리소스·file·HTTP(S) 요청에 대한 filesystem/network fallback은 없다. Linux native custom scheme은 이 store의 정확한 entry URI에만 MIME·CSP·본문을 응답한다. HTTP(S)·상대 파일·frame·fetch·worker 요청을 CSP로 차단하며 navigation 경계도 별도로 검사한다. script 허용에 따른 재로드는 같은 body를 재사용한다. 사용자 Reload와 에이전트 `surface.html_reload`는 새 document generation을 만들고 수신량·크기 허용·skipped·queue를 초기화한다. close/convert/reconnect는 이전 identity의 body와 credit을 해제한다. host는 disconnect·Loss·구조에서 사라지거나 HTML role이 아닌 mapping의 문서를 정리한다. 새 transport는 이전 문서 map을 승계하지 않는다. 변경 watcher나 파일 변경 통지는 만들지 않는다.

## 검증 경계

순수 모델 시험은 identity·size boundary·누적 credit 예약·취소·재사용을 검사한다. 실제 파일 worker 시험은 1.5MB exact hash, metadata-only 대기, 종류·symlink·교체·취소·timeout을 검사한다. `tests/attach_html_transfer_loopback.rs`는 실제 서버와 raw peer 사이의 왕복을 GUI/headless 각각 실행할 수 있다. native HTML viewer 렌더, 스크립트 banner, 하위 resource와 HTTP(S) 프록시는 이 전송 시험으로 검증하지 않는다.

2026-10-23 Linux WebKitGTK 2.50.4의 별도 context probe에서는 custom scheme 요청이 65초 관측 동안 load 미완료였고 이후 비동기 finish로 완료됐다. 자체 timeout은 그 구간에서 관측하지 못했으므로 무제한 대기가 보장된다는 뜻은 아니다. http/https scheme 등록은 실제 warning으로 거절됐고 HTTP custom callback은 0회였다. 두 proxy context의 loopback 요청은 각 proxy에만 도착했으며 direct context·다른 context의 cookie와 분리됐다. 이는 Linux API 선행 실측이며 제품 proxy나 다른 OS의 실행 검증이 아니다.


## Entry 수신 상태와 native 응답 수명

`HtmlMirror`는 연결 epoch와 surface binding마다 문서 세대를 소유한다. 새 Reload는 기존 scan을 취소하고 body store를 비운 뒤 단조 증가하는 request/transfer로 entry를 다시 요청한다. script Allow는 네트워크 요청 없이 검사가 끝난 같은 공유 body를 사용한다. 응답은 source generation도 보존한다. 검사 worker는 취소를 읽기 경계에서 확인하며, 취소·문서 교체·연결 종료 뒤 늦은 결과는 게시할 수 없다.

서버의 metadata 이후 30초 credit timeout은 사용자 크기 질문 중에도 흐른다. 시간이 지나 failure를 받으면 질문과 receiving을 끝내고, 늦은 Allow는 credit을 발급하지 않는다. Reload만 새 요청을 만들며 이전 크기 허용·누적량·skipped를 초기화한다. entry Decline은 취소와 skipped를 기록하고 본문 없는 종료 상태가 된다. 연결이 끊기면 미완성 수신을 취소하고 이미 완료한 본문은 보존한다.

## Host 연결과 원격 파일 열기

`com.tasty.html`이 등록되지 않은 client에서는 deferred placeholder를 유지한다. 등록 뒤 같은 leaf를 원격 모드로 생성하며 원격 source를 client file URI로 변환하지 않는다. 현재 viewer 광고는 Linux GUI에 한정한다. macOS·Windows의 native 응답 어댑터는 구현되어 있지만 전체 host 컴파일·native 실행 증거와 구분한다. macOS 11–13의 HTTP 미지원 계약은 파일 entry mirror와 별개이며 최소 OS 버전을 올리지 않는다.

주소 띠는 원격 경로를 축약 없이 읽기 전용으로 보이고 Reload만 제공한다. 크기 배너, 스크립트 배너, 페이지 순서로 하나의 inset을 적용한다. 큰 entry는 수신 전에 Allow·Decline을 기다리며 Decline 뒤에는 receiving을 종료하고 skipped tag를 표시한다. 완료한 body는 disconnect 중에도 보존하고 Reload를 비활성화한다. reconnect는 새 connection epoch로 다시 받는다.

스크립트 설정은 client 설정만 읽고 원격 설정이나 허용 메시지를 교환하지 않는다. 기존 스캐너가 받은 전체 body의 지문을 만들고 native 응답도 같은 body를 사용한다. 파일 source의 부모 cwd는 `SurfaceCwd::Remote`이며 client filesystem·PTY 상속·최근 파일로 사용하지 않는다. 지원 연결의 Explorer와 새 HTML 피커는 원래 pane에 원격 NewTab을 보낸다. 피커 결과는 원래 request ID·pane·session projection과 일치할 때만 적용하며 취소·pane 종료·연결 교체 뒤 늦은 결과를 버린다.

## Native 하위 응답 대기와 재사용 API

공유 body store는 기본적으로 하위 리소스를 비활성화한다. 현재 viewer는 여전히 entry만 표시하며 이 API만으로 native 리소스 capability를 광고하지 않는다. 연결한 경우 native 요청은 원 entry의 scheme·origin·여섯 부분 identity 경로 안에서만 대기할 수 있다. 경로는 한 번 decode하고 traversal·encoded separator·NUL을 거절한다. 파일이나 네트워크를 client에서 열지 않는다.

문서당 URI slot256개와 URI당 대기자32개로 요청 메타데이터를 제한한다. 같은 URI의 fragment 변화와 재요청은 한 immutable 응답 할당을 공유하며 새 transfer나 누적량을 만들지 않는다. 원 request identity는 실패 뒤에도 tombstone으로 남아 다른 URI에 재할당할 수 없다. 대기자 또는 producer 소멸은 취소로 끝나며 문서 교체가 원 응답 게시 권한을 회수한다. disconnect는 미완 대기자를 끝내고 새 요청을 거절하되 완료한 cache를 보존한다. MIME·provenance를 보존하고 source generation·전체 transfer ID가 맞는 완료 body만 게시한다. 실제 native callback·credit·크기 질문 연결은 이 수명 API와 구분한다.
