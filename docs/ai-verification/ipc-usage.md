# Tasty IPC를 통한 조작

Tasty는 AI가 조작 가능한 터미널이다. 정상 모드(터미널이 뜬 상태)에서는 IPC 서버가 자동으로 뜨며, 포트 파일(`~/.tasty/tasty.port`)을 통해 접속할 수 있다. Claude Code 등 터미널 안에서 동작하는 AI도 IPC로 Tasty를 제어할 수 있다.

**IPC 조작 예시 (Python)**:
```python
import socket, json
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket()
s.connect(('127.0.0.1', port))
# 스크린샷
s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":"ui.screenshot","params":{"path":"capture.png"}}) + '\n').encode())
# 키 입력
s.sendall((json.dumps({"jsonrpc":"2.0","id":2,"method":"surface.send_key","params":{"key":"ls\r"}}) + '\n').encode())
```

**CLI 조작 예시**:
```bash
tasty list                          # 워크스페이스 목록
tasty send "hello"                  # 텍스트 전송
tasty send-key "enter"              # 키 입력
tasty notify --title "Done"         # 알림
```
