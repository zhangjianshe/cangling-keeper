// Transport only: all image decoding, masking and drawing happen in the Rust renderer.
window.createKeeperImageOverlay=({upload,scene})=>{
 const cache=new Map(),controllers=new Set();let generation=0;
 function cancel(){generation++;for(const controller of controllers)controller.abort();controllers.clear();}
 async function prepare(layers){
  cancel();const turn=generation,ids=new Set(layers.map(l=>l.id));
  for(const id of cache.keys())if(!ids.has(id))cache.delete(id);
  scene(layers);
  await Promise.all(layers.filter(l=>l.visible).map(async layer=>{
   const key=JSON.stringify(layer.polygon);if(cache.get(layer.id)===key)return;
   const controller=new AbortController(),timeout=setTimeout(()=>controller.abort(),8000);
   controllers.add(controller);try{const response=await fetch('/api/earth/images/'+encodeURIComponent(layer.assetId||layer.id),{signal:controller.signal,credentials:'same-origin'});if(!response.ok)throw Error('图片加载失败：'+layer.name);
    const bytes=new Uint8Array(await response.arrayBuffer());if(turn!==generation)return;
    upload(layer.id,bytes,key);cache.set(layer.id,key);
   }finally{clearTimeout(timeout);controllers.delete(controller);}
  }));
  if(turn===generation)scene(layers);
 }
 return {prepare,cancel};
};
