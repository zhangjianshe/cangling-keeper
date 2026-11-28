// Renderer IDs survive a project change only for the same immutable resource.
// Application IDs remain project-scoped in APIs, receipts and layer controls.
window.createKeeperLayerReconciler=()=>{
 let serial=0;const current={tilesets:[],images:[],models:[]};
 function resource(kind,layer){
  if(kind==='tilesets')return new URL(layer.url,document.baseURI).href;
  const identity=/^[a-f0-9]{64}$/i.test(layer.resourceSha256||'')?'sha:'+layer.resourceSha256:'id:'+layer.id;
  return identity+(kind==='images'?'|'+JSON.stringify(layer.polygon):'');
 }
 return {
  prepare(kind,layers){
   const previous=current[kind],used=new Set(),next=[];
   const result=layers.map(layer=>{const key=resource(kind,layer),existing=previous.find(item=>item.key===key&&!used.has(item.native));
    const native=existing?.native||'keeper-'+kind+'-'+(++serial);used.add(native);next.push({key,native,logical:layer.id});return {...layer,...(kind==='tilesets'?{url:key}:{}),id:native,assetId:layer.id};});
   current[kind]=next;return result;
  },
  native(kind,id){return current[kind]?.find(item=>item.logical===id)?.native||id;},
  logical(kind,id){return current[kind]?.find(item=>item.native===id)?.logical||null;}
 };
};
