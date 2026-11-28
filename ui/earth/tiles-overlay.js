// Native code owns traversal, LOD, cache and rendering. This module only transports bounded bytes.
window.createKeeperTilesOverlay=({call,upload,report})=>{
 let layers=[],revision=0,generation=0,paused=false,disposed=false,timer=null,lastStatus='',lastReport=0;
 const jobs=new Map();let queue=[];
 function cancel(key){const job=jobs.get(key);job?.controller.abort();jobs.delete(key);call('tiles_cancel',{key});}
 function stop(){generation++;clearTimeout(timer);for(const key of jobs.keys())cancel(key);queue=[];}
 function resourceUrl(layer,uri){
  const root=new URL(layer.url),url=new URL(uri||layer.url,root);
  const prefix=root.pathname.slice(0,root.pathname.lastIndexOf('/')+1);
  if(!['http:','https:'].includes(url.protocol)||url.username||url.password||url.search||url.hash||url.origin!==root.origin||!url.pathname.startsWith(prefix))throw Error('瓦片URL越出数据集目录');
  if(location.protocol==='https:'&&url.protocol!=='https:')throw Error('HTTPS地球页面不能加载HTTP瓦片');
  return url.href;
 }
 async function bytes(response,limit){
  if(!response.ok)throw Error('HTTP '+response.status);
  if(Number(response.headers.get('content-length'))>limit)throw Error('瓦片超过大小限制');
  if(!response.body)throw Error('资源没有响应体');
  const reader=response.body.getReader(),chunks=[];let length=0;
  try{for(;;){const r=await reader.read();if(r.done)break;length+=r.value.length;if(length>limit){await reader.cancel();throw Error('瓦片超过大小限制');}chunks.push(r.value);}}
  finally{reader.releaseLock();}
  const result=new Uint8Array(length);let offset=0;for(const c of chunks){result.set(c,offset);offset+=c.length;}return result;
 }
 async function load(request){
  const layer=layers.find(l=>l.id===request.layerId);if(!layer)return;
  const epoch=generation,controller=new AbortController(),job={controller};jobs.set(request.key,job);
  const timeout=setTimeout(()=>controller.abort(),15000);
  try{
   const response=await fetch(resourceUrl(layer,request.uri),{signal:controller.signal,cache:request.key.endsWith('|root')?'no-store':'default',credentials:'omit',redirect:'error',referrerPolicy:'no-referrer'});
   const data=await bytes(response,request.json?4*1024*1024:16*1024*1024);
   if(epoch!==generation||jobs.get(request.key)!==job)return;
   if(request.json)call('tiles_manifest',{key:request.key,document:JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(data))});
   else upload(request.key,data);
  }catch(error){if(epoch===generation&&jobs.get(request.key)===job)call('tiles_fail',{key:request.key,error:String(error)});}
  finally{clearTimeout(timeout);if(jobs.get(request.key)===job)jobs.delete(request.key);}
 }
 function tick(){
  clearTimeout(timer);if(paused||disposed||!layers.length)return;
  try{
   const plan=call('tiles_tick'),active=new Set(plan.active);
   for(const key of jobs.keys())if(!active.has(key))cancel(key);
   queue=queue.filter(r=>active.has(r.key));
   for(const r of plan.requests)if(!jobs.has(r.key)&&!queue.some(v=>v.key===r.key))queue.push(r);
   while(jobs.size<4&&queue.length)load(queue.shift());
   const status=JSON.stringify(plan.layers),now=Date.now();
   if(status!==lastStatus&&now-lastReport>=1000){lastStatus=status;lastReport=now;report({revision,layers:plan.layers});}
  }catch(error){report({revision,error:String(error)});}
  timer=setTimeout(tick,250);
 }
 return {
  prepare(next,rev,options={}){
   if(options.preserveExisting){
    clearTimeout(timer);const shared=new Set(next.filter(layer=>layers.some(old=>old.id===layer.id&&old.url===layer.url)).map(layer=>layer.id));
    for(const key of [...jobs.keys()])if(!shared.has(key.split('|')[0]))cancel(key);queue=queue.filter(request=>shared.has(request.layerId));
   }else stop();
   // Stable entry URLs may publish new content. A new scene revision must discard
   // the native tree within one project; a project transition explicitly retains shared resources.
   if(layers.length&&rev!==revision&&!options.preserveExisting)call('tiles_scene',[]);
   layers=next;revision=rev;lastStatus='';lastReport=0;call('tiles_scene',layers);if(!layers.length)call('tiles_tick');tick();},
  pause(value){paused=!!value;if(paused)stop();else tick();},
  dispose(){stop();disposed=true;call('tiles_scene',[]);call('tiles_tick');}
 };
};
