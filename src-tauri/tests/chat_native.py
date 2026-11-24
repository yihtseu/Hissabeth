"""Exercise real Tauri IPC and bundled SDKs with isolated executable fixtures."""
import json
import os
from pathlib import Path
import subprocess
import sys
sys.dont_write_bytecode = True
import time
from native_settings import Atspi, NativeInput, until, app_for_pid, find, activate, descendants, select_with_keys, save_tree

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'src-tauri/target'
resume_models = '--resume-models' in sys.argv
models_only = '--models-only' in sys.argv or resume_models
WORKSPACE = TARGET / ('chat-model-native-workspace' if models_only else 'chat-native-workspace')
FIXTURE = TARGET / 'agent-cli-fixtures'
resume = '--resume-cancel' in sys.argv or resume_models
WORKSPACE.mkdir(exist_ok=True)
if not resume: (WORKSPACE/'chat-history.json').unlink(missing_ok=True)
FIXTURE.mkdir(exist_ok=True)
for provider in ('codex', 'claude'):
    path = FIXTURE/provider
    path.write_bytes((ROOT/'src-tauri/tests/agent_sdk_fixture.py').read_bytes())
    path.chmod(0o755)
if not resume: (FIXTURE/'chat-runtime-calls.jsonl').unlink(missing_ok=True)
preferences = {
    'locale':'en-US','appearance':{'theme':'light','accent':'blue','density':'compact','fontSize':13,'reducedMotion':False},
    'workspace':{'defaultView':'Layout','restoreLastView':False,'showNavigator':True,'showPrescriptionDock':True,'showMetrics':True,'showStatusBar':True,'navigatorSize':18,'dockSize':35},
    'display':{'decimals':4,'layoutRayArrows':True,'plotGrid':True},
    'analysis':{'samplingKind':'grid','gridSize':33,'hexRings':8,'reportPrecision':6,'pollIntervalMs':1500},
    'agents':{'targets':[],'autoRegister':False},
}
(WORKSPACE/'preferences.json').write_text(json.dumps({'snapshot':{'preferences':preferences,'revision':0}}))
env = dict(os.environ, PATH=str(FIXTURE)+os.pathsep+os.environ['PATH'], HISSABETH_DATA_DIR=str(WORKSPACE), HISSABETH_SKIP_AGENT_REGISTRATION='1', NO_AT_BRIDGE='0', GTK_A11Y='always', GDK_BACKEND='x11', WEBKIT_DISABLE_COMPOSITING_MODE='1')
log = (TARGET/'chat-native-app.log').open('w')
process = subprocess.Popen([str(TARGET/'debug/hissabeth')], cwd=ROOT, env=env, stdout=log, stderr=log)
app = None
try:
    native = NativeInput()
    app = until(lambda: app_for_pid(process.pid), 30)
    native.app = app
    message = until(lambda: find(app,'Message','entry'), 30)
    def send(text):
        assert message.get_component_iface().grab_focus()
        for char in text: native.press('space' if char==' ' else char)
        activate(until(lambda:find(app,'Send','button')))
    def text_of(node):
        try:
            text=node.get_text_iface()
            return Atspi.Text.get_text(text,0,-1) if text else node.get_name()
        except Exception: return node.get_name()
    def has_text(text): return any(text in text_of(node) for node in descendants(app))
    def reply_count():
        records=json.loads((WORKSPACE/'chat-history.json').read_text())
        return sum(message.get('text')=='Codex SDK fixture response' for record in records for message in record['messages'])
    if models_only:
        def model_picker(): return next((node for node in descendants(app) if node.get_role_name()=='combo box' and node.get_name().startswith('Model ')),None)
        if not resume_models:
            until(lambda:model_picker() and 'Codex A' in [node.get_name() for node in descendants(app)],20)
            picker=model_picker()
            select_with_keys(picker,'Home',native)
            assert picker.get_component_iface().grab_focus()
            native.press('Down');native.press('Return')
            until(lambda:'Codex A' in model_picker().get_name(),15)
            send('codexmodel')
            until(lambda:has_text('Codex SDK fixture response') and has_text('Finished'),20)
        agent=until(lambda:next((node for node in descendants(app) if node.get_role_name()=='combo box' and node.get_name().startswith('Agent ')),None))
        select_with_keys(agent,'End',native)
        until(lambda:not has_text('Loading models…'),25)
        picker=model_picker()
        select_with_keys(picker,'Home',native)
        assert picker.get_component_iface().grab_focus()
        native.press('Down');native.press('Return')
        until(lambda:'Claude A' in model_picker().get_name(),15)
        send('claudemodel')
        until(lambda:has_text('Claude SDK fixture response') and has_text('Finished'),20)
        saved=json.loads((WORKSPACE/'chat-history.json').read_text())
        assert {record['model'] for record in saved}=={'codex-fixture-a','claude-fixture-a'},saved
        calls=[json.loads(line) for line in (FIXTURE/'chat-runtime-calls.jsonl').read_text().splitlines()]
        codex=next(call for call in calls if call['provider']=='codex' and 'exec' in call['args'])
        claude=next(call for call in calls if call['provider']=='claude' and '--model=claude-fixture-a' in call['args'])
        assert codex['args'][codex['args'].index('--model')+1]=='codex-fixture-a',codex
        assert '--model=claude-fixture-a' in claude['args'],claude
        save_tree(app,'chat-model-native-tree.json')
        print('PASS: native model discovery through Tauri, both dropdown selections, SDK --model arguments and saved conversation models',flush=True)
    else:
        if resume:
            previous_calls=[json.loads(line) for line in (FIXTURE/'chat-runtime-calls.jsonl').read_text().splitlines()]
            previous_codex=sum(call['provider']=='codex' for call in previous_calls)
            assert reply_count()==3
            history_button=until(lambda:next((node for node in descendants(app) if node.get_role_name() in ('button','toggle button') and node.get_name().startswith('inspect')),None))
            activate(history_button)
            until(lambda:has_text('Codex SDK fixture response'),20)
            send('wait')
            until(lambda:len([json.loads(line) for line in (FIXTURE/'chat-runtime-calls.jsonl').read_text().splitlines() if json.loads(line)['provider']=='codex' and 'exec' in json.loads(line)['args']])==previous_codex+1,20)
            activate(until(lambda:find(app,'Stop','button'),20))
            until(lambda:has_text('Stopped. Completed optical edits remain saved.'),20)
            saved=json.loads((WORKSPACE/'chat-history.json').read_text())
            record=next(record for record in saved if record['provider']=='codex')
            assert record['outcome']=='cancelled',record
            assert not any(message.get('status')=='in_progress' for message in record['messages'])
        else:
            send('inspect')
            until(lambda:has_text('Codex SDK fixture response'),20)
            until(lambda:has_text('Finished'),20)
            send('continue')
            until(lambda:reply_count()==2 and has_text('Finished'),20)
            agent=until(lambda:next((node for node in descendants(app) if node.get_role_name()=='combo box' and node.get_name().startswith('Agent ')),None))
            select_with_keys(agent,'End',native)
            send('claude')
            until(lambda:has_text('Claude SDK fixture response'),20)
            until(lambda:has_text('Finished'),20)
            history_button=until(lambda:next((node for node in descendants(app) if node.get_role_name() in ('button','toggle button') and node.get_name().startswith('inspect')),None))
            activate(history_button)
            send('wait')
            until(lambda:len([json.loads(line) for line in (FIXTURE/'chat-runtime-calls.jsonl').read_text().splitlines() if json.loads(line)['provider']=='codex' and 'exec' in json.loads(line)['args']])==3,20)
            activate(until(lambda:find(app,'Stop','button'),20))
            until(lambda:has_text('Stopped. Completed optical edits remain saved.'),20)
            saved=json.loads((WORKSPACE/'chat-history.json').read_text())
            assert len(saved)==2,saved
            assert saved[0]['session'] and saved[1]['session'],saved
            process.terminate();process.wait(timeout=15)
            process=subprocess.Popen([str(TARGET/'debug/hissabeth')],cwd=ROOT,env=env,stdout=log,stderr=log)
            app=until(lambda:app_for_pid(process.pid),30);native.app=app
            message=until(lambda:find(app,'Message','entry'),30)
            history_button=until(lambda:next((node for node in descendants(app) if node.get_role_name() in ('button','toggle button') and node.get_name().startswith('inspect')),None))
            activate(history_button)
            send('restart')
            until(lambda:reply_count()==3 and has_text('Finished'),20)
        calls=[json.loads(line) for line in (FIXTURE/'chat-runtime-calls.jsonl').read_text().splitlines()]
        codex=[call for call in calls if call['provider']=='codex' and 'exec' in call['args']]
        assert len(codex)==4,calls
        assert 'resume' not in codex[0]['args'],codex
        assert 'resume' in codex[1]['args'],codex
        assert 'resume' in codex[-1]['args'],codex
        assert '11111111-1111-4111-8111-111111111111' in codex[1]['args'],codex
        claude=[call for call in calls if call['provider']=='claude' and '--no-session-persistence' not in call['args']]
        assert len(claude)==1,calls
        assert '--strict-mcp-config' in claude[0]['args'],claude
        assert '--mcp-config' in claude[0]['args'],claude
        save_tree(app,'chat-native-tree.json')
        print('PASS: native Tauri chat IPC, bundled Codex and Claude SDK subprocesses, Codex resume token, strict Claude MCP, streamed text, provider isolation, cancellation, saved history and resume after application restart',flush=True)

finally:
    if app:
        try: save_tree(app,'chat-native-final-tree.json')
        except Exception: pass
    process.terminate()
    process.wait(timeout=15)
    log.close()
