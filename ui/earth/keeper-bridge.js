import './image-overlay.js';
import {keeper_command,keeper_image} from './pkg/cangling_desktop.js';
const bridgeOrigin=new URL(document.baseURI).origin;
let ready=false,applied=null,measuring=false,points=[];
const call=(method,args={})=>JSON.parse(keeper_command(method,JSON.stringify(args)));
const send=(event,value)=>parent.postMessage({earth:true,event,value},bridgeOrigin);
const images=window.createKeeperImageOverlay({upload:keeper_image,scene:layers=>call('image_scene',layers)});
function report(value){send('selection',value);}
function nextFrame(callback){return new Promise((resolve,reject)=>{const timeout=setTimeout(()=>{window.removeEventListener('cangling-earth-frame',run);reject(Error('等待GPU画面超时'));},5000);function run(){clearTimeout(timeout);window.removeEventListener('cangling-earth-frame',run);try{resolve(callback?.());}catch(e){reject(e);}}window.addEventListener('cangling-earth-frame',run);});}
async function screenshot(){const output=document.createElement('canvas');output.width=innerWidth;output.height=innerHeight;const ctx=output.getContext('2d');await nextFrame(()=>ctx.drawImage(document.querySelector('canvas'),0,0,output.width,output.height));return output.toDataURL('image/png');}
window.addEventListener('cangling-earth-ready',()=>{ready=true;send('ready');});
window.addEventListener('message',async e=>{
 if(e.origin!==bridgeOrigin||e.source!==parent||!e.data?.earth||typeof e.data.id!=='string')return;
 const {id,method,args}=e.data;try{
  if(!ready)throw Error('地球尚未就绪');let result;
  if(method==='apply'){call('set_view',args.camera);call('layers',args.layers);call('vector_scene',args.overlays||[]);applied=args;await images.prepare(args.images||[]);await nextFrame();result=call('get_view');}
  else if(['get_view','pick','pause'].includes(method))result=call(method,args);
  else if(method==='measure'){measuring=!!args;points=[];result=true;}
  else if(method==='screenshot')result=await screenshot();
  else throw Error('不支持的地球操作');
  parent.postMessage({earth:true,id,result},bridgeOrigin);
 }catch(error){parent.postMessage({earth:true,id,error:String(error)},bridgeOrigin);}
});
window.addEventListener('click',e=>{if(!ready||!measuring||e.target!==document.querySelector('canvas'))return;const p=call('pick',{x:e.clientX/innerWidth,y:e.clientY/innerHeight});if(!p)return;points.push(p);if(points.length===1)return;const[a,b]=points,rad=Math.PI/180,dlat=(b.latitude-a.latitude)*rad,dlon=(b.longitude-a.longitude)*rad,h=Math.sin(dlat/2)**2+Math.cos(a.latitude*rad)*Math.cos(b.latitude*rad)*Math.sin(dlon/2)**2;report({measurement:'近似球面距离，不含地形起伏',meters:Math.round(6371008.8*2*Math.asin(Math.min(1,Math.sqrt(h)))),points});points=[];measuring=false;send('measure_finished');});
document.addEventListener('visibilitychange',()=>{if(ready)call('pause',document.hidden);});

// Double-click a terrain location to make its WGS84 coordinates available to chat.
window.addEventListener('dblclick',e=>{if(!ready||measuring||e.target!==document.querySelector('canvas'))return;const point=call('pick',{x:e.clientX/innerWidth,y:e.clientY/innerHeight});if(point)report({type:'location',...point,selectedAt:Date.now()});});

window.addEventListener('click',e=>{if(!ready||measuring||e.target!==document.querySelector('canvas'))return;const selected=call('vector_pick',{x:e.clientX/innerWidth,y:e.clientY/innerHeight});if(selected){selected.selectedAt=Date.now();report(selected);}});

window.addEventListener('keydown',e=>{if(!e.ctrlKey&&!e.metaKey&&!e.altKey&&!e.repeat&&e.key.toLowerCase()==='l'){e.preventDefault();e.stopImmediatePropagation();send('layers_toggle');}},true);
