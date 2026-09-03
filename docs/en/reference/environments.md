<!-- source-hash: b99f47a413bc -->
# Environment notes (agent bootstrap)

Per-OS paths, liveness checks, and connection patterns an AI agent needs before operating tasty. The method table is [api.md](api.md).

## Paths (every OS)

| File | Path | Description |
|------|------|------|
| Port file | `~/.tasty/tasty.port` | The dynamic IPC port. Created at launch |
| Config | `~/.tasty/config.toml` | User settings |

(On Windows, `~` = `%USERPROFILE%`. Debug builds have a separate root and keep the same files under `~/.tasty-debug/` — isolation details in [dev-guide/independent-verification](../dev-guide/independent-verification.md).)

## Checking whether it is running

```bash
pgrep -x tasty >/dev/null && echo running || echo "not running"
# A port file with no process is stale — delete it
[ -f ~/.tasty/tasty.port ] && ! pgrep -x tasty >/dev/null && rm ~/.tasty/tasty.port
```

A debug instance for verification (`cargo run`) has its root at `~/.tasty-debug/`, so its port file is `~/.tasty-debug/tasty.port` — it can run alongside without colliding with the release check above.

## Launch / wait / shutdown

```bash
tasty &
until tasty list info 2>/dev/null; do sleep 0.2; done    # until the port is up (a condition check, not a sleep loop)
tasty list tree            # structure (with IDs)
tasty send text "ls -la\r" --surface <id>
# shutdown: the system.shutdown IPC (or the tasty command)
```

## Direct IPC (Python)

```python
import socket, json, os
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket(); s.settimeout(5); s.connect(("127.0.0.1", port))
def call(m, p=None):
    s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p or {}}) + "\n").encode())
    return json.loads(s.recv(1<<16).decode())
call("system.info")
```

## Screenshots

`tasty screenshot --path <png>` (IPC `ui.screenshot {path}`) is an official release feature that works in
**GUI mode** (focus-independent — `--surface <id>` captures a terminal surface off-screen,
`--window <id>` captures a window frame). The result is a PNG. Details: [screenshot-methods](../ai-verification/screenshot-methods.md).

## Related

- [api.md](api.md) — the whole IPC/CLI · [dev-guide/self-verification](../dev-guide/self-verification.md) — verification scenarios
