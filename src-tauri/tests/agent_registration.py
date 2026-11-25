"""Verify registration deduplication using isolated CLI configuration fixtures."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
RUN = ROOT / 'target' / 'agent-cli-fixtures'
RUN.mkdir(exist_ok=True)
BINARY = ROOT / 'target/debug/hissabeth'
workspace = RUN / 'registration-workspace'
workspace.mkdir(exist_ok=True)
fixture = '''#!/usr/bin/python3
import json, os, sys
from pathlib import Path
agent=Path(sys.argv[0]).name
base=Path(os.environ['CLAUDE_CONFIG_DIR'])
file=base/('.claude.json' if agent=='claude' else 'codex-config.json')
config=json.loads(file.read_text())
entries=config['mcpServers']
args=sys.argv[1:]
with (base/'registration-commands.jsonl').open('a') as log: log.write(json.dumps([agent,*args])+'\\n')
if args[:2]==['mcp','list']:
 print(json.dumps([{'name':name,'transport':value} for name,value in entries.items()]))
elif args[:2]==['mcp','remove']:
 entries.pop(args[-1], None)
 file.write_text(json.dumps(config))
elif args[:2]==['mcp','add']:
 end=args.index('--')
 entries[args[2]]={'command':args[end+1], 'args':args[end+2:], 'env':dict([args[args.index('--env')+1].split('=',1)])}
 file.write_text(json.dumps(config))
else: sys.exit(2)
'''
for name in ('codex', 'claude'):
    path = RUN / name
    path.write_text(fixture)
    path.chmod(0o755)
env = dict(os.environ, PATH=str(RUN)+os.pathsep+os.environ['PATH'], CLAUDE_CONFIG_DIR=str(RUN), HISSABETH_DATA_DIR=str(workspace))

def config_file(agent):
    return RUN / ('.claude.json' if agent == 'claude' else 'codex-config.json')

def own(command=str(BINARY), data=str(workspace), args=None):
    return {'command':command, 'args': ['--mcp'] if args is None else args, 'env':{'HISSABETH_DATA_DIR':data}}

def seed(entries):
    for agent in ('codex','claude'):
        config_file(agent).write_text(json.dumps({'mcpServers':entries,'unrelatedSetting':42}))

def run():
    result = subprocess.run([str(BINARY),'--configure-agents'], env=env, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)

seed({'hissabeth':own(), 'alias':own(), 'older-path':own('/old/install/hissabeth'), 'other-project':own(data='/another/project'), 'unrelated':own('/apps/other', args=['--mcp'])})
assert all(item['success'] for item in run())
for agent in ('codex','claude'):
    config=json.loads(config_file(agent).read_text())
    assert set(config['mcpServers']) == {'hissabeth','other-project','unrelated'}, config
    assert config['unrelatedSetting'] == 42
before = (RUN/'registration-commands.jsonl').read_text()
assert all(item['success'] for item in run())
after = (RUN/'registration-commands.jsonl').read_text()[len(before):]
assert 'add' not in after and 'remove' not in after, after
seed({'hissabeth':own('/apps/other'), 'alias':own()})
assert all(not item['success'] for item in run())
assert 'alias' in json.loads(config_file('claude').read_text())['mcpServers']
seed({'hissabeth':own('/old/install/hissabeth')})
assert all(item['success'] for item in run())
for agent in ('codex','claude'):
    assert json.loads(config_file(agent).read_text())['mcpServers']['hissabeth']['command'] == str(BINARY)
print('PASS: canonical registration, duplicate aliases, old executable, other workspace, unrelated service, repeated registration, collision, settings retention')
