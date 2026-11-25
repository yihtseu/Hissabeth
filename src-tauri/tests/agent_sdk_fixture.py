#!/usr/bin/python3
import json,os,sys,time
from pathlib import Path
name=Path(sys.argv[0]).name
base=Path(__file__).resolve().parent
with (base/'chat-runtime-calls.jsonl').open('a') as log: log.write(json.dumps({'provider':name,'args':sys.argv[1:]})+'\n')
def out(event): print(json.dumps(event),flush=True)
if name=='codex' and 'app-server' in sys.argv:
 for line in sys.stdin:
  request=json.loads(line)
  with (base/'chat-model-requests.jsonl').open('a') as log:log.write(line)
  if request['method']=='initialize': out({'id':request['id'],'result':{'userAgent':'fixture'}})
  elif request['method']=='model/list':
   data=([{'model':'codex-hidden','displayName':'Hidden','hidden':True},{'model':'codex-fixture-a','displayName':'Codex A','hidden':False}] if not request['params'].get('cursor') else [{'model':'codex-fixture-a','displayName':'Duplicate','hidden':False},{'model':'codex-fixture-b','displayName':'Codex B','hidden':False}])
   out({'id':request['id'],'result':{'data':data,'nextCursor':None if request['params'].get('cursor') else 'page-two'}})
  else:
   if request['method']!='initialized':sys.exit(2)
elif name=='codex':
 prompt=sys.stdin.read()
 out({'type':'thread.started','thread_id':'11111111-1111-4111-8111-111111111111'})
 out({'type':'item.started','item':{'type':'mcp_tool_call','id':'t','server':'hissabeth','tool':'system_get','arguments':{},'status':'in_progress'}})
 if 'wait' in prompt:
  time.sleep(60)
 else:
  out({'type':'item.completed','item':{'type':'mcp_tool_call','id':'t','server':'hissabeth','tool':'system_get','arguments':{},'result':{'content':[],'structured_content':{'revision':1}},'status':'completed'}})
  out({'type':'item.completed','item':{'type':'agent_message','id':'a','text':'Codex SDK fixture response'}})
  out({'type':'turn.completed','usage':{'input_tokens':1,'output_tokens':1,'cached_input_tokens':0}})
else:
 for line in sys.stdin:
  event=json.loads(line)
  with (base/'chat-claude-control.jsonl').open('a') as log:log.write(line)
  if event.get('type')=='control_request':
   out({'type':'control_response','response':{'subtype':'success','request_id':event['request_id'],'response':{'commands':[],'models':[{'value':'claude-fixture-a','displayName':'Claude A','description':'Fixture A'},{'value':'claude-fixture-b','displayName':'Claude B','description':'Fixture B'}]}}})
  if event.get('type')=='user':
   out({'type':'system','subtype':'init','session_id':'22222222-2222-4222-8222-222222222222','mcp_servers':[{'name':'hissabeth','status':'connected'}],'tools':[],'model':'fixture','cwd':os.getcwd(),'permissionMode':'default','apiKeySource':'none','claude_code_version':'fixture','slash_commands':[],'agents':[],'uuid':'init'})
   out({'type':'assistant','message':{'id':'claude-answer','type':'message','role':'assistant','content':[{'type':'text','text':'Claude SDK fixture response'}],'model':'fixture','stop_reason':'end_turn','stop_sequence':None,'usage':{'input_tokens':1,'output_tokens':1}},'parent_tool_use_id':None,'session_id':'22222222-2222-4222-8222-222222222222','uuid':'assistant'})
   out({'type':'result','subtype':'success','is_error':False,'result':'Claude SDK fixture response','duration_ms':1,'duration_api_ms':1,'num_turns':1,'total_cost_usd':0,'usage':{'input_tokens':1,'output_tokens':1},'modelUsage':{},'permission_denials':[],'session_id':'22222222-2222-4222-8222-222222222222','uuid':'result'})
   break
