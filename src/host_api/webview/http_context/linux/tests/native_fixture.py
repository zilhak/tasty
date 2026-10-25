"""Own-display fixture invoking the real Rust adapter's ignored native test.

Run with a freshly built fixture test binary and an own artifact directory, under
an external exclusive build/native lock. No product capabilities are activated.
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import ssl
import subprocess
import sys
import threading
import time
import tempfile
import shutil

ROOT = Path(sys.argv[2]).resolve()
ROOT.mkdir(parents=True, exist_ok=False)
os.chmod(ROOT, 0o700)
BINARY = str(Path(sys.argv[1]).resolve())
BINARY_HASH = hashlib.sha256(Path(BINARY).read_bytes()).hexdigest()
ctypes.CDLL(None).prctl(36, 1, 0, 0, 0)
OWNED = {}
CLEANUP = []
PROFILE_HOMES = []
DIRECT = []
LOGICAL_PORTS = {}

def proc(pid):
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return {"pid": pid, "parent": int(fields[1]), "start": fields[19], "state": fields[0]}
    except (FileNotFoundError, ProcessLookupError):
        return None

def remember(pid):
    item = proc(pid)
    if item:
        OWNED[pid] = item

def descendants():
    table = []
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            item = proc(int(entry.name))
            if item:
                table.append(item)
    changed = True
    while changed:
        changed = False
        for item in table:
            if (item["parent"] in OWNED or item["parent"] == os.getpid()) and item["pid"] not in OWNED:
                OWNED[item["pid"]] = item
                changed = True

def cleanup():
    descendants()
    for pid, old in reversed(list(OWNED.items())):
        now = proc(pid)
        if not now or now["start"] != old["start"]:
            CLEANUP.append({**old, "absent_or_replaced": True})
            continue
        if now["state"] == "Z":
            try:
                os.waitpid(pid, os.WNOHANG)
            except ChildProcessError:
                pass
            CLEANUP.append({**old, "already_exited": True})
            continue
        env = Path(f"/proc/{pid}/environ").read_bytes().split(b"\0")
        assert any(e.startswith(b"TASTY_HOME=" + str(home).encode()) for e in env for home in [ROOT, *PROFILE_HOMES]), pid
        os.kill(pid, signal.SIGTERM)
        CLEANUP.append({**old, "signal": 15})
    until = time.monotonic() + 5
    while time.monotonic() < until:
        for pid, old in OWNED.items():
            now = proc(pid)
            if now and now["start"] == old["start"] and now["state"] == "Z":
                try:
                    os.waitpid(pid, os.WNOHANG)
                except ChildProcessError:
                    pass
        if all(proc(pid) is None or proc(pid)["start"] != old["start"] for pid, old in OWNED.items()):
            break
        time.sleep(.05)
    for row in CLEANUP:
        now = proc(row["pid"])
        row["absent_or_replaced"] = now is None or now["start"] != row["start"]

class DirectSentinel:
    """Own both localhost address families; a bypass can reach only this fixture."""
    def __init__(self):
        self.stop = threading.Event()
        self.accepted = 0
        self.baseline = 0
        self.lock = threading.Lock()
        for attempt in range(32):
            four = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            six = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
            six.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
            four.bind(("127.0.0.1", 0))
            self.port = four.getsockname()[1]
            try:
                six.bind(("::1", self.port))
                break
            except OSError:
                four.close()
                six.close()
        else:
            raise RuntimeError("cannot reserve controlled dual-stack destinations")
        self.sockets = [four, six]
        self.workers = []
        for listener in self.sockets:
            listener.listen(16)
            listener.settimeout(.1)
            worker = threading.Thread(target=self.accept, args=(listener,))
            worker.start()
            self.workers.append(worker)
        for host in ("127.0.0.1", "::1"):
            with socket.create_connection((host, self.port), timeout=2):
                pass
        end = time.monotonic() + 2
        while self.accepted != 2:
            assert time.monotonic() < end, "direct sentinel positive control"
            time.sleep(.01)
        self.baseline = self.accepted
    def accept(self, listener):
        while not self.stop.is_set():
            try:
                io, _ = listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            with self.lock:
                self.accepted += 1
            io.close()
    def close(self):
        self.stop.set()
        for listener in self.sockets:
            listener.close()
        for worker in self.workers:
            worker.join(2)
        assert all(not worker.is_alive() for worker in self.workers)

def certificate(name, host):
    pem, key, der = [ROOT / (name + suffix) for suffix in (".pem", ".key", ".der")]
    subprocess.run(["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes", "-days", "1", "-subj", "/CN=" + host, "-addext", "subjectAltName=DNS:" + host, "-keyout", str(key), "-out", str(pem)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    subprocess.run(["openssl", "x509", "-in", str(pem), "-outform", "DER", "-out", str(der)], check=True)
    return pem, key, hashlib.sha256(der.read_bytes()).hexdigest()

def exact(io, n):
    result = b""
    while len(result) < n:
        data = io.recv(n - len(result))
        if not data:
            raise EOFError("fixture short read")
        result += data
    return result

class Endpoint:
    def __init__(self, cert, case):
        self.certificate_hash = cert[2]
        self.password = b"next-fixture" if case == "fresh-next" else b"fixture"
        self.closed = False
        self.case = case
        self.requests = []
        self.errors = []
        self.auth = 0
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.live = set()
        self.workers = []
        self.slots = threading.BoundedSemaphore(16)
        self.socket = socket.socket()
        self.socket.bind(("127.0.0.1", 0))
        self.port = self.socket.getsockname()[1]
        self.socket.listen(16)
        self.socket.settimeout(.1)
        self.tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.tls.minimum_version = ssl.TLSVersion.TLSv1_3
        self.tls.maximum_version = ssl.TLSVersion.TLSv1_3
        self.tls.num_tickets = 0
        self.tls.options |= ssl.OP_NO_TICKET
        self.tls.load_cert_chain(str(cert[0]), str(cert[1]))
        self.other_tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.other_tls.minimum_version = ssl.TLSVersion.TLSv1_3
        self.other_tls.maximum_version = ssl.TLSVersion.TLSv1_3
        self.other_tls.num_tickets = 0
        self.other_tls.options |= ssl.OP_NO_TICKET
        self.other_tls.load_cert_chain(str(NEW_LEAF[0]), str(NEW_LEAF[1]))
        self.thread = threading.Thread(target=self.accept)
        self.thread.start()
    def accept(self):
        while not self.stop.is_set():
            try:
                io, _ = self.socket.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            if not self.slots.acquire(False):
                io.close()
                continue
            with self.lock:
                self.live.add(io)
            worker = threading.Thread(target=self.serve, args=(io,))
            self.workers.append(worker)
            worker.start()
    def serve(self, raw):
        io = raw
        try:
            io.settimeout(12)
            assert exact(io, 1) == b"\x05"
            methods = exact(io, exact(io, 1)[0])
            assert 2 in methods
            io.sendall(b"\x05\x02")
            assert exact(io, 1) == b"\x01"
            user = exact(io, exact(io, 1)[0])
            password = exact(io, exact(io, 1)[0])
            assert user == b"owned" and password == self.password
            self.auth += 1
            io.sendall(b"\x01\x00")
            header = exact(io, 4)
            assert header[:3] == b"\x05\x01\x00"
            if header[3] == 3:
                host = exact(io, exact(io, 1)[0])
            elif header[3] == 1:
                host = socket.inet_ntoa(exact(io, 4)).encode()
            else:
                raise AssertionError("unexpected address family")
            port = int.from_bytes(exact(io, 2), "big")
            assert (host == b"localhost" or host.endswith(b".localhost") or host == b"127.0.0.1") and port in LOGICAL_PORTS.values()
            io.sendall(b"\x05\x00\x00\x01\x7f\x00\x00\x01\x00\x00")
            if self.case in ("bound", "timeout"):
                self.stop.wait(12)
                return
            if port in (LOGICAL_PORTS["HTTPS"], LOGICAL_PORTS["OTHER"]):
                io = (self.tls if port == LOGICAL_PORTS["HTTPS"] else self.other_tls).wrap_socket(io, server_side=True)
                with self.lock:
                    self.live.discard(raw)
                    self.live.add(io)
            self.http(io, port)
        except (OSError, EOFError, ssl.SSLError) as error:
            self.errors.append(type(error).__name__)
        except BaseException as error:
            self.errors.append(repr(error))
        finally:
            with self.lock:
                self.live.discard(raw)
                self.live.discard(io)
            io.close()
            raw.close()
            self.slots.release()
    def http(self, io, port):
        data = b""
        while b"\r\n\r\n" not in data:
            data += exact(io, 1)
            assert len(data) < 65536
        header, body = data.split(b"\r\n\r\n", 1)
        first = header.split(b"\r\n")[0].decode()
        fields = dict(line.split(b":", 1) for line in header.lower().split(b"\r\n")[1:])
        length = int(fields.get(b"content-length", b"0").strip())
        assert length < 100000
        body += exact(io, length - len(body))
        path = first.split()[1]
        self.requests.append({"line": first, "port": port, "leaf_sha256": self.certificate_hash if port == LOGICAL_PORTS["HTTPS"] else NEW_LEAF[2] if port == LOGICAL_PORTS["OTHER"] else None, "body_sha256": hashlib.sha256(body).hexdigest(), "binary_present": bytes([0,255,1,128,13,10,42]) in body, "cookie_kept": b"owned=kept" in fields.get(b"cookie", b""), "resumed": getattr(io, "session_reused", False)})
        content, extra = response(path, self.case)
        io.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: " + str(len(content)).encode() + b"\r\nConnection: close\r\n" + extra + b"\r\n" + content)
    def close(self):
        if self.closed:
            return
        self.closed = True
        self.stop.set()
        self.socket.close()
        with self.lock:
            for io in self.live:
                try:
                    io.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        self.thread.join(5)
        for worker in self.workers:
            worker.join(5)
        assert not self.thread.is_alive() and all(not w.is_alive() for w in self.workers)

def response(path, case):
    if path in ("/retained-seed", "/retained-held-seed"):
        action = b"/held-post" if path == "/retained-held-seed" else b"/retained-post"
        html = b'<form method="POST" action="/retained-post" enctype="multipart/form-data"><input type="file" name="payload"></form><script>const d=new DataTransfer();d.items.add(new File([new Uint8Array([0,255,1,128,13,10,42])],"owned.bin"));document.querySelector("input").files=d.files;document.forms[0].submit();</script>'
        html = html.replace(b"/retained-post", action)
        return html, b"Content-Type: text/html\r\nSet-Cookie: owned=kept; SameSite=Lax; Path=/\r\n"
    if path == "/retained-post":
        return response("/check" if case == "fresh-next" else "/install", case)

    if path == "/install":
        return b'''<!doctype html><title>install</title><script>(async()=>{document.cookie='owned=kept;path=/';localStorage.setItem('owned','kept');const db=await new Promise((resolve,reject)=>{const q=indexedDB.open('owned',1);q.onupgradeneeded=()=>q.result.createObjectStore('kv');q.onsuccess=()=>resolve(q.result);q.onerror=()=>reject(q.error);});await new Promise((resolve,reject)=>{const t=db.transaction('kv','readwrite');t.objectStore('kv').put('kept','owned');t.oncomplete=resolve;t.onerror=()=>reject(t.error);});db.close();await navigator.serviceWorker.register('/sw.js');await navigator.serviceWorker.ready;if(!navigator.serviceWorker.controller)await new Promise(r=>navigator.serviceWorker.addEventListener('controllerchange',r,{once:true}));if(await(await fetch('/sw-hit')).text()!=='SW OLD')throw Error('SW');const c=await caches.open('owned');await c.put('/stored',new Response('old'));await fetch('/asset');document.title='CACHE READY';})().catch(e=>document.title='FAIL '+e);</script>''', b"Content-Type: text/html\r\n"
    if path == "/check":
        return b'''<!doctype html><title>check</title><script>(async()=>{const db=await new Promise((resolve,reject)=>{const q=indexedDB.open('owned',1);q.onsuccess=()=>resolve(q.result);q.onerror=()=>reject(q.error);});const stored=await new Promise((resolve,reject)=>{const q=db.transaction('kv').objectStore('kv').get('owned');q.onsuccess=()=>resolve(q.result);q.onerror=()=>reject(q.error);});db.close();const local=localStorage.getItem('owned');const r=await navigator.serviceWorker.getRegistrations();const k=await caches.keys();const t=await(await fetch('/sw-hit')).text();await fetch('/asset');const controller=!!navigator.serviceWorker.controller;document.title=!r.length&&!controller&&!k.length&&t==='REMOTE'&&document.cookie.includes('owned=kept')&&local==='kept'&&stored==='kept'?'CACHE CLEARED COOKIE KEPT':'FAIL STATE '+JSON.stringify({regs:r.length,controller,keys:k,sw:t,cookie:document.cookie,local:local,indexed:stored});})().catch(e=>document.title='FAIL '+e);</script>''', b"Content-Type: text/html\r\n"
    if path == "/sw.js":
        return b"self.addEventListener('install',e=>self.skipWaiting());self.addEventListener('activate',e=>e.waitUntil(clients.claim()));self.addEventListener('fetch',e=>{if(new URL(e.request.url).pathname==='/sw-hit')e.respondWith(new Response('SW OLD'));if(new URL(e.request.url).pathname==='/retained-post')e.respondWith(new Response('<title>SW OLD</title>',{headers:{'Content-Type':'text/html'}}));});", b"Content-Type: application/javascript\r\n"
    if path == "/asset":
        return b"asset", b"Cache-Control: max-age=3600\r\n"
    if path == "/sw-hit":
        return b"REMOTE", b"Content-Type: text/plain\r\n"
    return b"<!doctype html><title>POST RETAINED</title>", b"Content-Type: text/html\r\n"

def run_case(case, cert, expected, env):
    endpoint = Endpoint(cert, case)
    local = ROOT / case
    local.mkdir()
    (local / "runtime").mkdir(mode=0o700)
    profile_home = Path(tempfile.mkdtemp(prefix="tasty-profile-native-"))
    os.chmod(profile_home, 0o700)
    PROFILE_HOMES.append(profile_home)
    child_env = env | {"HOME": str(profile_home), "TASTY_HOME": str(profile_home / "tasty"), "XDG_CACHE_HOME": str(local / "cache"), "XDG_DATA_HOME": str(local / "data"), "XDG_CONFIG_HOME": str(local / "config"), "XDG_RUNTIME_DIR": str(local / "runtime"), "DE_CASE": case, "DE_EXPECTED_LEAF": expected[2], "DE_OTHER_LEAF": NEW_LEAF[2], "DE_PROXY_URI": f"socks5://owned:fixture@127.0.0.1:{endpoint.port}"}
    following = None
    child_env["DE_FIXTURE_ROOT"] = str(local)
    child_env["DE_PROFILE_HOME"] = str(profile_home)
    child_env |= {f"DE_{key}_PORT": str(value) for key, value in LOGICAL_PORTS.items()}
    try:
        with (local / "native.log").open("w") as output:
            child = subprocess.Popen(["dbus-run-session", "--", BINARY, "--ignored", "--exact", "http_context::linux::tests::native_suite", "--nocapture", "--test-threads=1"], env=child_env, stdout=output, stderr=subprocess.STDOUT)
            remember(child.pid)
            deadline = time.monotonic() + 28
            while child.poll() is None:
                descendants()
                if (case.startswith("fresh") or case in ("retained_post", "retained_https", "retained_held", "retained_epoch")) and following is None and (local / "retire-endpoint").exists():
                    endpoint.close()
                    next_cert = NEW_LEAF if case.startswith("fresh_changed") or case in ("retained_post", "retained_https", "retained_held", "retained_epoch") else cert
                    following = Endpoint(next_cert, "fresh-next")
                    next_expected = next_cert[2] if case in ("fresh_changed_valid", "retained_post", "retained_https", "retained_held", "retained_epoch") else cert[2] if case == "fresh_changed_reject" else "07" * 32
                    (local / "next-expected").write_text(next_expected)
                    (local / "next-proxy").write_text(f"socks5://owned:next-fixture@127.0.0.1:{following.port}")
                if time.monotonic() > deadline:
                    raise TimeoutError(case)
                time.sleep(.025)
        log = (local / "native.log").read_text()
        assert child.returncode == 0, (case, child.returncode, log[-6000:])
        assert f"NATIVE_CASE_PASS {case}" in log, log
        if case in ("valid", "repeat", "port", "history") or case.startswith("fresh"):
            combined = endpoint.requests + (following.requests if following else [])
            assert len(combined) == (2 if case in ("repeat", "port", "fresh_changed_valid") else 1), combined
            assert all(r["line"] == "POST /post HTTP/1.1" and r["binary_present"] and not r["resumed"] for r in combined), combined
            if case == "fresh_changed_reject":
                assert following is not None and not following.requests and following.auth == 0
        elif case in ("cache", "cache_private"):
            assert sum(r["line"] == "GET /asset HTTP/1.1" for r in endpoint.requests) == 2, endpoint.requests
            assert sum(r["line"] == "GET /sw-hit HTTP/1.1" for r in endpoint.requests) == 1, endpoint.requests
            assert any(r["line"] == "GET /check HTTP/1.1" and r["cookie_kept"] for r in endpoint.requests), endpoint.requests
        elif case == "retained_https":
            assert following is not None
            combined = endpoint.requests + following.requests
            assert len(endpoint.requests) == len(following.requests) == 1
            assert all(r["line"] == "POST /post HTTP/1.1" and r["binary_present"] for r in combined)
            assert combined[0]["body_sha256"] == combined[1]["body_sha256"]
        elif case in ("retained_held", "retained_held_cancel"):
            assert len(endpoint.requests) == 1 and endpoint.requests[0]["line"] == "GET /retained-held-seed HTTP/1.1"
            if case == "retained_held":
                assert following is not None and len(following.requests) == 1
                assert following.requests[0]["line"] == "POST /held-post HTTP/1.1"
                assert following.requests[0]["binary_present"] and following.requests[0]["cookie_kept"]
            else:
                assert following is None
        elif case == "retained_epoch":
            assert following is not None
            assert len(endpoint.requests) == len(following.requests) == 1
            assert endpoint.requests[0]["line"] == following.requests[0]["line"] == "GET /base HTTP/1.1"
        elif case == "retained_post":
            assert following is not None
            first = [r for r in endpoint.requests if r["line"] == "POST /retained-post HTTP/1.1"]
            after = [r for r in following.requests if r["line"] == "POST /retained-post HTTP/1.1"]
            assert len(first) == len(after) == 1, (first, after)
            assert all(r["cookie_kept"] and r["binary_present"] for r in first + after)
            assert first[0]["body_sha256"] == after[0]["body_sha256"]
            assert sum(r["line"] == "GET /asset HTTP/1.1" for r in following.requests) == 1
            assert sum(r["line"] == "GET /sw-hit HTTP/1.1" for r in following.requests) == 1
        elif case == "retained_no_clear":
            assert sum(r["line"] == "POST /retained-post HTTP/1.1" for r in endpoint.requests) == 1
            assert following is None
        elif case == "retained_lifecycle":
            assert all(r["line"] == "GET /base HTTP/1.1" for r in endpoint.requests)
        else:
            assert not endpoint.requests, endpoint.requests
        return {"case": case, "rc": 0, "requests": endpoint.requests, "authenticated_connections": endpoint.auth, "tls_errors": endpoint.errors, "native_log": str(local / "native.log"), "native_log_sha256": hashlib.sha256((local / "native.log").read_bytes()).hexdigest()}
    finally:
        endpoint.close()
        if following:
            following.close()
        (local / "server.json").write_text(json.dumps({"first": endpoint.requests, "next": following.requests if following else [], "auth": endpoint.auth, "next_auth": following.auth if following else 0, "first_leaf_sha256": endpoint.certificate_hash, "next_leaf_sha256": following.certificate_hash if following else None}, indent=2) + "\n")

rows = []
xvfb = None
try:
    env = {key: value for key, value in os.environ.items() if not key.startswith("TASTY_")}
    for key in ("TASTY_SURFACE_ID", "TASTY_SESSION_TOKEN", "TASTY_AGENT_ID", "DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY"):
        env.pop(key, None)
    (ROOT / "xvfb-home").mkdir(mode=0o700)
    env |= {"HOME": str(ROOT / "xvfb-home"), "TASTY_HOME": str(ROOT / "xvfb-home"), "GDK_BACKEND": "x11", "LIBGL_ALWAYS_SOFTWARE": "1"}
    read_fd, write_fd = os.pipe()
    with (ROOT / "xvfb.log").open("w") as output:
        xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1024x768x24", "-nolisten", "tcp"], env=env, pass_fds=(write_fd,), stdout=output, stderr=subprocess.STDOUT)
    remember(xvfb.pid)
    os.close(write_fd)
    display = ":" + os.read(read_fd, 32).decode().strip()
    os.close(read_fd)
    env["DISPLAY"] = display
    for kind in ("HTTP", "HTTPS", "OTHER"):
        direct = DirectSentinel()
        DIRECT.append(direct)
        LOGICAL_PORTS[kind] = direct.port
    good = certificate("owned", "localhost")
    other = certificate("other", "localhost")
    NEW_LEAF = other
    bad = certificate("bad-host", "wrong.invalid")
    cases = sys.argv[3:] or ["lifecycle", "valid", "repeat", "fresh_changed_reject", "fresh_changed_valid", "port", "history", "wrong_leaf", "extra_flags", "wrong_host", "retired", "closed", "bound", "timeout", "cache"]
    for case in cases:
        rows.append(run_case(case, other if case == "wrong_leaf" else bad if case == "extra_flags" else good, bad if case == "extra_flags" else good, env))
        (ROOT / "progress.json").write_text(json.dumps(rows, indent=2) + "\n")
    direct_counts = [{"port": direct.port, "positive_control": direct.baseline,
                      "native_direct_connections": direct.accepted - direct.baseline} for direct in DIRECT]
    assert all(row["positive_control"] == 2 and row["native_direct_connections"] == 0 for row in direct_counts), direct_counts
    result = {"rc": 0, "cases": rows, "display": display, "binary_sha256": BINARY_HASH, "direct_sentinels": direct_counts}
except BaseException as error:
    result = {"rc": 1, "error": repr(error), "cases": rows, "binary_sha256": BINARY_HASH}
finally:
    cleanup()
    if xvfb:
        xvfb.wait(timeout=5)
    for direct in DIRECT:
        direct.close()
    result["cleanup"] = CLEANUP
    result["owned_all_absent"] = all(row["absent_or_replaced"] for row in CLEANUP)
    result["private_profile_homes"] = [{"path": str(home), "removed": False} for home in PROFILE_HOMES]
    if result["owned_all_absent"]:
        for home, row in zip(PROFILE_HOMES, result["private_profile_homes"]):
            shutil.rmtree(home)
            row["removed"] = not home.exists()
    (ROOT / "result.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps({"rc": result["rc"], "cases": len(rows), "result": str(ROOT / 'result.json'), "owned_all_absent": result["owned_all_absent"]}))
raise SystemExit(result["rc"])
