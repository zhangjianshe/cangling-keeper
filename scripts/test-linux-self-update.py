from pathlib import Path
import tempfile,subprocess,socket,urllib.request,time,json,shutil,os,signal,sys
source=Path(sys.argv[1]).resolve()
probe=subprocess.check_output([str(source),'--self-update-probe'],text=True).strip()
assert probe.startswith('keeper-update-probe:');version=probe.split(':',1)[1]
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
for fail in [False,True]:
 with tempfile.TemporaryDirectory(prefix='keeper actual update ') as directory:
  root=Path(directory);data=root/'user data';data.mkdir();(data/'sentinel').write_text('preserve')
  target=root/'keeper';stage=root/'new';helper=root/'helper'
  for p in [target,helper]:shutil.copy2(source,p)
  if fail:stage.write_text('invalid executable');stage.chmod(0o755)
  else:shutil.copy2(source,stage)
  s=socket.socket();s.bind(('127.0.0.1',0));port=s.getsockname()[1];s.close()
  args=['--listen',f'127.0.0.1:{port}','--data-dir',str(data),'--no-open'];url=f'http://127.0.0.1:{port}/api/health'
  def health():
   with opener.open(url,timeout=1) as r:return json.load(r)
  old=subprocess.Popen([str(target),*args],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);updater=None;new_pid=None
  try:
   for _ in range(100):
    try:h=health();break
    except OSError:time.sleep(.1)
   else:raise RuntimeError('old process did not start')
   assert h['pid']==old.pid
   if not fail:assert subprocess.check_output([str(stage),'--self-update-probe'],text=True).strip()==probe
   plan={'pid':old.pid,'target':str(target),'staged':str(stage),'backup':str(root/'previous'),'args':args,'cwd':str(root),'health_url':url,'latest':'v'+version,'result':str(data/'app-update-result.json')}
   path=root/'plan.json';path.write_text(json.dumps(plan));updater=subprocess.Popen([str(helper),'--apply-self-update',str(path)],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
   time.sleep(.2);assert updater.poll() is None,'helper exited before old process'
   old.terminate();old.wait(timeout=5)
   assert updater.wait(timeout=40)==0,updater.stderr.read().decode()
   result=json.loads(Path(plan['result']).read_text());assert result['phase']==('rolled_back' if fail else 'complete'),result
   for _ in range(100):
    try:h=health();break
    except OSError:time.sleep(.1)
   else:raise RuntimeError('replacement process did not start')
   new_pid=h['pid'];assert new_pid!=old.pid and h['version']==version;assert (data/'sentinel').read_text()=='preserve'
   print('Actual executable test passed:',result['phase'],'old PID',old.pid,'new PID',new_pid,'data preserved')
  finally:
   if not new_pid:
    try:new_pid=health()['pid']
    except OSError:pass
   if new_pid:
    try:os.kill(new_pid,signal.SIGTERM)
    except ProcessLookupError:pass
   if old.poll() is None:old.terminate();old.wait(timeout=5)
   if updater and updater.poll() is None:updater.kill();updater.wait()
