// North-up geographic image masks, subdivided before projection onto the globe.
window.createKeeperImageOverlay=({project,report})=>{
 const canvas=document.createElement('canvas');canvas.id='earth-image-overlay';canvas.style.cssText='position:fixed;inset:0;width:100%;height:100%;pointer-events:none;z-index:2';document.body.append(canvas);
 const cache=new Map();let generation=0;
 function bounds(points){const x=points.map(p=>p[0]),y=points.map(p=>p[1]);return [Math.min(...x),Math.min(...y),Math.max(...x),Math.max(...y)];}
 async function prepare(layers){const turn=++generation,ids=new Set(layers.map(l=>l.id));for(const id of cache.keys())if(!ids.has(id))cache.delete(id);
  await Promise.all(layers.filter(l=>l.visible).map(async layer=>{
   const key=JSON.stringify(layer.polygon);if(cache.get(layer.id)?.key===key)return;
   const img=new Image();img.decoding='async';await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>{img.onload=null;img.onerror=null;img.src='';reject(Error('图片加载超时：'+layer.name));},8000);img.onload=()=>{clearTimeout(timeout);resolve();};img.onerror=()=>{clearTimeout(timeout);reject(Error('图片加载失败：'+layer.name));};img.src='/api/earth/images/'+encodeURIComponent(layer.id);});
   if(turn!==generation)return;
   const box=bounds(layer.polygon),texture=document.createElement('canvas'),ratio=Math.min(1,2048/Math.max(img.naturalWidth,img.naturalHeight));texture.width=Math.max(1,Math.round(img.naturalWidth*ratio));texture.height=Math.max(1,Math.round(img.naturalHeight*ratio));const ctx=texture.getContext('2d');
   ctx.beginPath();layer.polygon.forEach((p,i)=>{const x=(p[0]-box[0])/(box[2]-box[0])*texture.width,y=(box[3]-p[1])/(box[3]-box[1])*texture.height;i?ctx.lineTo(x,y):ctx.moveTo(x,y);});ctx.closePath();ctx.clip();ctx.drawImage(img,0,0,texture.width,texture.height);cache.set(layer.id,{key,box,texture});
  }));
 }
 function triangle(ctx,texture,source,target){
  const [s0,s1,s2]=source,[d0,d1,d2]=target,ux=s1[0]-s0[0],uy=s1[1]-s0[1],vx=s2[0]-s0[0],vy=s2[1]-s0[1],det=ux*vy-vx*uy;if(Math.abs(det)<1e-8)return;
  const a=((d1[0]-d0[0])*vy-(d2[0]-d0[0])*uy)/det,c=((d2[0]-d0[0])*ux-(d1[0]-d0[0])*vx)/det,b=((d1[1]-d0[1])*vy-(d2[1]-d0[1])*uy)/det,d=((d2[1]-d0[1])*ux-(d1[1]-d0[1])*vx)/det;
  ctx.save();ctx.beginPath();ctx.moveTo(...d0);ctx.lineTo(...d1);ctx.lineTo(...d2);ctx.closePath();ctx.clip();ctx.setTransform(a,b,c,d,d0[0]-a*s0[0]-c*s0[1],d0[1]-b*s0[0]-d*s0[1]);ctx.drawImage(texture,0,0);ctx.restore();
 }
 function draw(layers,width,height){canvas.width=width;canvas.height=height;const ctx=canvas.getContext('2d');ctx.clearRect(0,0,width,height);
  for(const layer of layers){const item=cache.get(layer.id);if(!layer.visible||!item)continue;const {texture,box}=item,n=24,points=[],uv=[];
   for(let y=0;y<=n;y++)for(let x=0;x<=n;x++){points.push([box[0]+(box[2]-box[0])*x/n,box[3]-(box[3]-box[1])*y/n]);uv.push([texture.width*x/n,texture.height*y/n]);}
   const screen=project(points).map(p=>p?[p[0]*width,p[1]*height]:null);
   for(let y=0;y<n;y++)for(let x=0;x<n;x++){const a=y*(n+1)+x,b=a+1,c=a+n+1,d=c+1;for(const ids of [[a,b,d],[a,d,c]])if(ids.every(i=>screen[i]&&screen[i].every(Number.isFinite)))triangle(ctx,texture,ids.map(i=>uv[i]),ids.map(i=>screen[i]));}
  }
 }
 return {canvas,prepare,draw};
};
