# TCP 통신 도구

`ncat`, `nc`, `netcat`이 설치되어 있지 않다. IPC 테스트 등에서 TCP 통신이 필요하면 Python socket을 사용할 것.

```python
import socket, json
s = socket.socket()
s.settimeout(5)
s.connect(('127.0.0.1', PORT))
s.sendall((json.dumps(request) + '\n').encode())
data = s.recv(4096)
s.close()
```
