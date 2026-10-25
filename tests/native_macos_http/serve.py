#!/usr/bin/env python3
"""Owned loopback-only SOCKS fixture. Does not resolve/connect requested destination hosts."""
import socket, threading, subprocess, os, json, pathlib, sys, time
records = []
listener = socket.socket(); listener.bind(('127.0.0.1', 0)); listener.listen()
listener.settimeout(.2)
stop = threading.Event()
trap = socket.socket(); trap.bind(('127.0.0.1',0)); trap.listen(); trap.settimeout(.2)
def direct_trap():
    while not stop.is_set():
        try: s,_ = trap.accept()
        except socket.timeout: continue
        except OSError: break
        records.append({'DIRECT_CONNECTION':True})
        try:
            s.settimeout(2);s.recv(65536);s.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\ndirect trap')
        except OSError: pass
        s.close()
trap_thread=threading.Thread(target=direct_trap);trap_thread.start()

def exact(s, n):
    data = b''
    while len(data) < n:
        block = s.recv(n-len(data))
        if not block: raise EOFError()
        data += block
    return data
def connection(s):
    s.settimeout(8)
    records.append({"accepted":True})
    try:
        version, count = exact(s, 2); methods = exact(s, count)
        records.append({"greeting":list(methods),"version":version})
        assert version == 5 and 2 in methods
        s.sendall(bytes([5, 2]))
        version, n = exact(s, 2); username = exact(s, n); n = exact(s, 1)[0]; password = exact(s,n)
        assert version == 1 and username == b'fixture' and password == b'secret'
        s.sendall(bytes([1, 0]))
        version, cmd, reserved, kind = exact(s,4)
        assert (version,cmd,reserved) == (5,1,0)
        if kind == 3: host = exact(s,exact(s,1)[0]).decode()
        elif kind == 1: host = socket.inet_ntop(socket.AF_INET,exact(s,4))
        elif kind == 4: host = socket.inet_ntop(socket.AF_INET6,exact(s,16))
        else: raise AssertionError(kind)
        port = int.from_bytes(exact(s,2),'big')
        records.append({'host':host,'port':port,'authenticated':True})
        s.sendall(bytes([5,0,0,1,127,0,0,1,0,0]))
        while True:
            header=b''
            while b'\r\n\r\n' not in header:
                header+=exact(s,1)
                assert len(header)<65536
            text=header.decode('iso-8859-1'); records.append({'http':text})
            if 'Upgrade: websocket' in text or 'Upgrade: WebSocket' in text:
                s.sendall(b'HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n'); return
            size=0
            for line in text.split('\r\n')[1:]:
                if line.lower().startswith('content-length:'): size=int(line.split(':',1)[1])
            if size: records.append({'method_body_hex':exact(s,size).hex()})
            body=b'<html><body>POST exact</body></html>' if text.startswith('POST ') else b'<html><body>remote fixture</body></html>'

            cookie=b'Set-Cookie: isolated=yes; SameSite=Lax; Path=/\r\n' if ' /set ' in text else b''
            s.sendall(b'HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: '+str(len(body)).encode()+b'\r\n'+cookie+b'Cache-Control: no-cache\r\n\r\n'+body)
    except (EOFError,ConnectionError,socket.timeout): pass
    except Exception as e: records.append({'server_error':repr(e)})
    finally: s.close()
def serve():
    while not stop.is_set():
        try: s,_=listener.accept()
        except socket.timeout: continue
        except OSError: break
        threading.Thread(target=connection,args=(s,),daemon=True).start()
thread=threading.Thread(target=serve);thread.start()
try:
    env=os.environ.copy();env['FIXTURE_PROXY_PORT']=str(listener.getsockname()[1]);env['FIXTURE_TARGET_PORT']=str(trap.getsockname()[1])
    p=subprocess.Popen([sys.argv[1],*sys.argv[2:]],env=env)
    records.append({'owned_child_pid':p.pid,'start_unix':time.time()})
    try: rc=p.wait(timeout=120)
    except subprocess.TimeoutExpired:
        p.kill();p.wait();raise
    records.append({'owned_child_pid':p.pid,'rc':rc,'reaped':True})
finally:
    stop.set();listener.close();trap.close();thread.join();trap_thread.join()
    pathlib.Path('routing-records.json').write_text(json.dumps(records,indent=2)+'\n')
print('ROUTES',json.dumps(records),flush=True)
sys.exit(rc)
