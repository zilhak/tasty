"""Full functional test for tasty via IPC. Run with a GUI instance already started."""
import socket, json, time, os, sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 0

PASS = 0
FAIL = 0
ERRORS = []

def ipc(method, params={}):
    s = socket.socket()
    s.settimeout(10)
    s.connect(('127.0.0.1', PORT))
    s.sendall((json.dumps({'jsonrpc':'2.0','method':method,'params':params,'id':1}) + '\n').encode())
    data = b''
    while b'\n' not in data:
        chunk = s.recv(4096)
        if not chunk: break
        data += chunk
    s.close()
    resp = json.loads(data.decode().strip())
    if 'error' in resp:
        return None
    return resp.get('result')

def ipc_raw(method, params={}):
    s = socket.socket()
    s.settimeout(5)
    s.connect(('127.0.0.1', PORT))
    s.sendall((json.dumps({'jsonrpc':'2.0','method':method,'params':params,'id':1}) + '\n').encode())
    data = b''
    while b'\n' not in data:
        chunk = s.recv(4096)
        if not chunk: break
        data += chunk
    s.close()
    return json.loads(data.decode().strip())

def screenshot(name):
    path = os.path.join('E:/workspace/tasty', f'test_{name}.ppm')
    ipc('ui.screenshot', {'path': path})
    time.sleep(0.5)
    return path

def check(name, condition, detail=''):
    global PASS, FAIL, ERRORS
    if condition:
        PASS += 1
        print(f'  PASS: {name}')
    else:
        FAIL += 1
        msg = f'  FAIL: {name}' + (f' ({detail})' if detail else '')
        print(msg)
        ERRORS.append(msg)

def analyze_halves(ppm_path):
    try:
        with open(ppm_path, 'rb') as f:
            f.readline()
            while True:
                line = f.readline().strip()
                if not line.startswith(b'#'): break
            dims = line.split()
            w, h = int(dims[0]), int(dims[1])
            f.readline()
            pixels = f.read()
        mid = w // 2
        left = right = 0
        for y in range(0, h, 5):
            for x in range(0, mid):
                idx = (y*w+x)*3
                if pixels[idx]>40 or pixels[idx+1]>40 or pixels[idx+2]>40: left += 1
            for x in range(mid, w):
                idx = (y*w+x)*3
                if pixels[idx]>40 or pixels[idx+1]>40 or pixels[idx+2]>40: right += 1
        return left, right
    except:
        return 0, 0

print('=' * 60)
print('TASTY GUI FULL FUNCTIONAL TEST')
print('=' * 60)

# 1. Initial state
print('\n[1] Initial State')
info = ipc('system.info')
check('system.info returns version', info and 'version' in info)
ui = ipc('ui.state')
check('starts with 1 workspace', ui and ui['workspace_count'] == 1)
check('starts with 1 pane', ui and ui['pane_count'] == 1)
check('starts with 1 tab', ui and ui['tab_count'] == 1)
check('settings closed', ui and not ui['settings_open'])
check('notification panel closed', ui and not ui['notification_panel_open'])
surfaces = ipc('surface.list')
check('1 surface initially', surfaces and len(surfaces) == 1)
if surfaces:
    check('surface has valid cols', surfaces[0]['cols'] > 10)
    check('surface has valid rows', surfaces[0]['rows'] > 5)

# 2. Terminal I/O
print('\n[2] Terminal Input/Output')
ipc('surface.set_mark')
ipc('surface.send', {'text': 'echo hello_test\r\n'})
time.sleep(1)
output = ipc('surface.read_since_mark', {'strip_ansi': True})
check('echo reaches terminal', output and 'hello_test' in output.get('text', ''))
screen = ipc('surface.screen_text')
check('screen_text not empty', screen and len(screen.get('text', '').strip()) > 0)
cursor = ipc('surface.cursor_position')
check('cursor position valid', cursor and 'x' in cursor and 'y' in cursor)

# 3. Key combos
print('\n[3] Key Combos')
for key, mods in [('c', ['ctrl']), ('z', ['ctrl']), ('d', ['ctrl']), ('x', ['alt'])]:
    r = ipc('surface.send_combo', {'key': key, 'modifiers': mods})
    label = '+'.join(mods) + '+' + key
    check(f'send_combo {label}', r and r.get('sent'))

# 4. Pane Split (vertical)
print('\n[4] Pane Split Vertical')
r = ipc('pane.split', {'direction': 'vertical'})
check('vertical split succeeds', r and r['pane_count'] == 2)
time.sleep(1.5)
surfaces = ipc('surface.list')
check('2 surfaces after split', surfaces and len(surfaces) == 2)
if surfaces and len(surfaces) == 2:
    max_cols = max(s['cols'] for s in surfaces)
    check('surfaces resized (cols < 60)', max_cols < 60, f'max_cols={max_cols}')
ss = screenshot('split_v')
left, right = analyze_halves(ss)
check('both panes render (vertical)', left > 50 and right > 50, f'left={left} right={right}')

# 5. Pane Focus
print('\n[5] Pane Focus')
panes = ipc('pane.list')
if panes and len(panes) >= 2:
    first_id = panes[0]['id']
    r = ipc('pane.focus', {'pane_id': first_id})
    check('focus pane by ID', r and r.get('focused'))

# 6. Close Pane
print('\n[6] Close Pane')
r = ipc('pane.close')
check('close pane', r and r.get('closed'))
ui = ipc('ui.state')
check('back to 1 pane', ui and ui['pane_count'] == 1)

# 7. Horizontal Split
print('\n[7] Pane Split Horizontal')
r = ipc('pane.split', {'direction': 'horizontal'})
check('horizontal split', r and r['pane_count'] == 2)
time.sleep(1)
ipc('pane.close')

# 8. Tabs
print('\n[8] Tab Operations')
r = ipc('tab.create')
check('create tab', r and r['tab_count'] == 2)
tabs = ipc('tab.list')
check('tab list has 2', tabs and len(tabs) == 2)
r = ipc('tab.close')
check('close tab', r and r.get('closed'))

# 9. Workspaces
print('\n[9] Workspace Operations')
r = ipc('workspace.create', {'name': 'ws_test'})
check('create workspace', r and 'id' in r)
wlist = ipc('workspace.list')
check('2 workspaces', wlist and len(wlist) == 2)
r = ipc('workspace.select', {'index': 0})
check('switch to ws 0', r and r['active_workspace'] == 0)
r = ipc('workspace.select', {'index': 1})
check('switch to ws 1', r and r['active_workspace'] == 1)
ipc('workspace.select', {'index': 0})

# 10. Notifications
print('\n[10] Notifications')
r = ipc('notification.create', {'title': 'Test', 'body': 'Hello'})
check('create notification', r and r.get('created'))
nlist = ipc('notification.list')
check('notification exists', nlist and len(nlist) >= 1)

# 11. Hooks
print('\n[11] Hooks')
r = ipc('hook.set', {'surface_id': 1, 'event': 'bell', 'command': 'echo test'})
check('set hook', r and 'hook_id' in r)
hid = r.get('hook_id', 0) if r else 0
hlist = ipc('hook.list')
check('hook listed', hlist and len(hlist) >= 1)
r = ipc('hook.unset', {'hook_id': hid})
check('unset hook', r and r.get('removed'))

# 12. Tree
print('\n[12] Tree View')
tree = ipc('tree')
check('tree returns data', tree and len(tree) >= 1)

# 13. Surface targeting by ID
print('\n[13] Surface Targeting')
surfaces = ipc('surface.list')
if surfaces:
    sid = surfaces[0]['id']
    r = ipc('surface.focus', {'surface_id': sid})
    check('focus surface by ID', r and r.get('focused'))
    ipc('surface.set_mark', {'surface_id': sid})
    ipc('surface.send', {'surface_id': sid, 'text': 'echo targeted\r\n'})
    time.sleep(0.5)
    out = ipc('surface.read_since_mark', {'surface_id': sid, 'strip_ansi': True})
    check('send to specific surface', out and 'targeted' in out.get('text', ''))
    st = ipc('surface.screen_text', {'surface_id': sid})
    check('screen_text by ID', st and len(st.get('text', '').strip()) > 0)

# 14. Special keys
print('\n[14] Special Keys')
keys = ['enter','tab','escape','backspace','up','down','left','right',
        'home','end','pageup','pagedown','delete','insert',
        'f1','f2','f3','f4','f5','f6','f7','f8','f9','f10','f11','f12']
for key in keys:
    r = ipc('surface.send_key', {'key': key})
    check(f'send_key {key}', r and r.get('sent'))

# 15. Error paths
print('\n[15] Error Paths')
r = ipc_raw('nonexistent.method')
check('unknown method -> error', 'error' in r)
r = ipc_raw('pane.focus', {'pane_id': 99999})
check('bad pane_id -> error', 'error' in r)
r = ipc_raw('surface.focus', {'surface_id': 99999})
check('bad surface_id -> error', 'error' in r)
r = ipc_raw('workspace.select', {'index': 99999})
check('bad ws index -> error', 'error' in r)
r = ipc_raw('surface.send_combo', {'modifiers': ['ctrl']})
check('missing key -> error', 'error' in r)
r = ipc_raw('pane.close')
# Last pane should not close
check('close last pane -> not closed', 'result' in r and not r['result'].get('closed', True))
r = ipc_raw('tab.close')
check('close last tab -> not closed', 'result' in r and not r['result'].get('closed', True))

# 16. Screenshot
print('\n[16] Screenshot API')
ss = screenshot('final')
check('screenshot created', os.path.exists(ss) and os.path.getsize(ss) > 1000)

# Summary
print('\n' + '=' * 60)
print(f'RESULTS: {PASS} passed, {FAIL} failed')
print('=' * 60)
if ERRORS:
    print('\nFailed tests:')
    for e in ERRORS:
        print(e)

# Cleanup
for f in os.listdir('E:/workspace/tasty/'):
    if f.startswith('test_') and f.endswith('.ppm'):
        os.remove(os.path.join('E:/workspace/tasty/', f))

sys.exit(1 if FAIL > 0 else 0)
