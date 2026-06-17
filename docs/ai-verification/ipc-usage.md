# IPC 로 tasty 조작·검증

tasty 는 AI 가 조작 가능한 터미널이다. 정상 모드면 IPC 서버가 자동으로 뜨고 포트 파일(`~/.tasty/tasty.port`)로 접속한다 — tasty 터미널 안에서 도는 AI 도 IPC 로 자신을 제어할 수 있다. 전체 메서드는 [reference/api](../reference/api.md).

```python
import socket, json, os
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket(); s.connect(('127.0.0.1', port))
def call(method, params=None):
    s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params or {}})+'\n').encode())
    return json.loads(s.makefile('rb').readline())
```

```bash
tasty list workspaces
tasty send text "hello" && tasty send key enter
```

## 함정 1 — `\r`(Enter) 전송

| 전송 경로 | `"text\r"` 의미 | Enter 전송법 |
|-----------|----------------|-------------|
| CLI `tasty send text "..."` | **리터럴 `\`+`r`** (셸 `"..."` 안에서 이스케이프 안 됨) | `send text` + `send key enter`, 또는 `$'...\r'` |
| Python/JSON-RPC | CR (0x0D) ✅ | `{"text": "...\r"}` |
| CLI `tasty send text $'...'` | CR (0x0D) ✅ | `$'text\r'` |

혼동하면 `\r` 이 리터럴로 전송돼 셸 명령이 실행되지 않거나 화면에 `\r` 이 그대로 보인다. **Bash 도구에서 `tasty send text "cat\r"` 는 리터럴 전송** — `tasty send text "cat" && tasty send key enter` 또는 Python IPC 를 쓴다.

## 함정 2 — 응답은 `read_line` 으로 (`read_to_end` 금지)

IPC 서버는 응답을 한 줄(`\n` 종료)로 보낸 뒤 **connection 을 즉시 닫지 않는다.** `read_to_end`/TCP EOF 대기로 읽으면 응답을 받고도 read timeout 만큼 더 기다린다(10초 설정이면 정확히 10초). "모든 IPC 호출이 10초씩 걸린다 → throttling 이다" 같은 잘못된 가설로 시간을 낭비하는 위험한 함정이다.

```rust
// ❌ server close 까지 read timeout 만큼 대기
stream.read_to_end(&mut buf).ok();
// ✅ 한 줄만
BufReader::new(&stream).read_line(&mut line).ok();
```

```python
line = s.makefile('rb').readline()   # ✅ 줄 단위
```

`tests/common/mod.rs::call()` 가 표준 구현 — 새 테스트/디버그 repro 도구는 이것을 따른다.
