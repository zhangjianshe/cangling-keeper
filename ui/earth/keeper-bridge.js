import './image-overlay.js';
import {keeper_command} from './pkg/cangling_desktop.js';
const bridgeOrigin=new URL(document.baseURI).origin;
let ready=false,applied=null,measuring=false,points=[],lastDraw='';
const call=(method,args={})=>JSON.parse(keeper_command(method,JSON.stringify(args)));
const send=(event,value)=>parent.postMessage({earth:true,event,value},bridgeOrigin);
const ns='http://www.w3.org/2000/svg',svg=document.createElementNS(ns,'svg');
svg.id='business-overlay';svg.style.cssText='position:fixed;inset:0;width:100%;height:100%;pointer-events:none;z-index:3';document.body.append(svg);
const images=window.createKeeperImageOverlay({project:points=>call('project',points),report:send});
function report(value){send('selection',value);}
function lineParts(g){switch(g.type){case 'Point':return [[g.coordinates]];case 'MultiPoint':return g.coordinates.map(p=>[p]);case 'LineString':return [g.coordinates];case 'MultiLineString':case 'Polygon':return g.coordinates;case 'MultiPolygon':return g.coordinates.flat();default:return [];}}
function draw(){if(!ready||document.hidden||!applied)return;const width=innerWidth,height=innerHeight,key=JSON.stringify([call('get_view'),applied.revision,width,height,measuring]);if(key===lastDraw)return;lastDraw=key;svg.setAttribute('viewBox',`0 0 ${width} ${height}`);svg.replaceChildren();images.draw(applied.images||[],width,height);
 for(const layer of applied.overlays||[]){if(!layer.visible)continue;const data=layer.data,features=data.type==='FeatureCollection'?data.features:[data];
  features.forEach((feature,index)=>{const g=feature.type==='Feature'?feature.geometry:feature;
   for(const coordinates of lineParts(g)){const projected=call('project',coordinates);let shape;
    if(coordinates.length===1){if(!projected[0])continue;shape=document.createElementNS(ns,'circle');shape.setAttribute('cx',projected[0][0]*width);shape.setAttribute('cy',projected[0][1]*height);shape.setAttribute('r','6');shape.setAttribute('fill','#36e1bb');}
    else{let pen=false;const path=projected.map(p=>{if(!p){pen=false;return '';}const cmd=pen?'L':'M';pen=true;return `${cmd}${p[0]*width} ${p[1]*height}`;}).join(' ');shape=document.createElementNS(ns,'path');shape.setAttribute('d',path);shape.setAttribute('fill','none');}
    shape.setAttribute('stroke','#57f5cf');shape.setAttribute('stroke-width','3');shape.style.pointerEvents=measuring?'none':'visiblePainted';shape.style.cursor='pointer';shape.addEventListener('click',event=>{event.stopPropagation();const value={layer:layer.path,feature:index,properties:feature.properties||{},selectedAt:Date.now()};if(JSON.stringify(value).length>18000)value.properties={notice:'属性过大，未展开'};report(value);});svg.append(shape);
   }
  });
 }
}
setInterval(()=>{try{draw();}catch(error){send('error',String(error));}},120);
function nextFrame(callback){return new Promise((resolve,reject)=>{const timeout=setTimeout(()=>{window.removeEventListener('cangling-earth-frame',run);reject(Error('等待GPU画面超时'));},5000);function run(){clearTimeout(timeout);window.removeEventListener('cangling-earth-frame',run);try{resolve(callback?.());}catch(e){reject(e);}}window.addEventListener('cangling-earth-frame',run);});}
async function screenshot(){const output=document.createElement('canvas');output.width=innerWidth;output.height=innerHeight;const ctx=output.getContext('2d');await nextFrame(()=>ctx.drawImage(document.querySelector('canvas:not(#earth-image-overlay)'),0,0,output.width,output.height));draw();ctx.drawImage(images.canvas,0,0,output.width,output.height);const url=URL.createObjectURL(new Blob([new XMLSerializer().serializeToString(svg)],{type:'image/svg+xml'}));try{const img=new Image();await new Promise((res,rej)=>{img.onload=res;img.onerror=()=>rej(Error('无法合成业务图层'));img.src=url;});ctx.drawImage(img,0,0);return output.toDataURL('image/png');}finally{URL.revokeObjectURL(url);}}
window.addEventListener('cangling-earth-ready',()=>{ready=true;send('ready');});
window.addEventListener('message',async e=>{
 if(e.origin!==bridgeOrigin||e.source!==parent||!e.data?.earth||typeof e.data.id!=='string')return;
 const {id,method,args}=e.data;try{
  if(!ready)throw Error('地球尚未就绪');let result;
  if(method==='apply'){call('set_view',args.camera);call('layers',args.layers);applied=args;await images.prepare(args.images||[]);lastDraw='';await nextFrame();draw();result=call('get_view');}
  else if(['get_view','pick','pause'].includes(method))result=call(method,args);
  else if(method==='measure'){measuring=!!args;points=[];result=true;}
  else if(method==='screenshot')result=await screenshot();
  else throw Error('不支持的地球操作');
  parent.postMessage({earth:true,id,result},bridgeOrigin);
 }catch(error){parent.postMessage({earth:true,id,error:String(error)},bridgeOrigin);}
});
window.addEventListener('click',e=>{if(!ready||!measuring||e.target!==document.querySelector('canvas:not(#earth-image-overlay)'))return;const p=call('pick',{x:e.clientX/innerWidth,y:e.clientY/innerHeight});if(!p)return;points.push(p);if(points.length===1)return;const[a,b]=points,rad=Math.PI/180,dlat=(b.latitude-a.latitude)*rad,dlon=(b.longitude-a.longitude)*rad,h=Math.sin(dlat/2)**2+Math.cos(a.latitude*rad)*Math.cos(b.latitude*rad)*Math.sin(dlon/2)**2;report({measurement:'近似球面距离，不含地形起伏',meters:Math.round(6371008.8*2*Math.asin(Math.min(1,Math.sqrt(h)))),points});points=[];measuring=false;send('measure_finished');});
document.addEventListener('visibilitychange',()=>{if(ready)call('pause',document.hidden);});

// Double-click a terrain location to make its WGS84 coordinates available to chat.
window.addEventListener('dblclick',e=>{if(!ready||measuring||e.target!==document.querySelector('canvas:not(#earth-image-overlay)'))return;const point=call('pick',{x:e.clientX/innerWidth,y:e.clientY/innerHeight});if(point)report({type:'location',...point,selectedAt:Date.now()});});
