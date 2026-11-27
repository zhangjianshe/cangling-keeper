import http.server,threading,json,subprocess,tempfile,pathlib
calls=[]
class API(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  calls.append(self.path);self.send_response(200);self.end_headers();self.wfile.write(json.dumps({'ok':True,'result':{'servers':[{'id':'test','enabled':True,'model':'mock','contextLimit':131072}]}}).encode())
s=http.server.HTTPServer(('127.0.0.1',0),API);threading.Thread(target=s.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory() as tmp:
 out=pathlib.Path(tmp)/'untouched'
 args=['python3','scripts/eval-agent-real.py','--preflight','--base-url',f'http://127.0.0.1:{s.server_port}','--model-id','test','--output',str(out)]
 p=subprocess.run(args,capture_output=True,text=True);assert p.returncode==0,p.stderr;assert not out.exists();assert calls==['/api/invoke/agent_model_config']
 p=subprocess.run(args+['--model-id','missing'],capture_output=True,text=True);assert p.returncode!=0;assert not out.exists()
s.shutdown();print('PASS read-only preflight, missing model rejection, no output directory or mutation')
