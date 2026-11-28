const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const c={window:{},URL,document:{baseURI:'http://localhost/earth/index.html'}};vm.runInNewContext(fs.readFileSync('ui/earth/layer-reconciler.js','utf8'),c);
const r=c.window.createKeeperLayerReconciler(),sha='a'.repeat(64),other='b'.repeat(64);
const a=r.prepare('tilesets',[{id:'a-common',url:'https://data.example/common/tileset.json',visible:true},{id:'a-only',url:'https://data.example/old/tileset.json'}]);
const b=r.prepare('tilesets',[{id:'b-common',url:'https://data.example/common/tileset.json',visible:false,maximumScreenSpaceError:8},{id:'b-new',url:'https://data.example/new/tileset.json'}]);
assert.equal(a[0].id,b[0].id,'shared URL retains the same renderer ID across project IDs');assert.equal(b[0].visible,false);assert.equal(b[0].maximumScreenSpaceError,8);assert(!b.some(v=>v.id===a[1].id),'old-only layer is absent from the new renderer scene');assert.equal(r.logical('tilesets',a[0].id),'b-common');assert.equal(r.logical('tilesets',a[1].id),null);
const changed=r.prepare('tilesets',[{id:'b-common',url:'https://data.example/different/tileset.json'}]);assert.notEqual(changed[0].id,b[0].id,'changed resource is not identified just by display/application ID');
for(const kind of ['models','images']){
 const x=r.prepare(kind,[{id:'asset-a',resourceSha256:sha,polygon:[[0,0],[1,0],[1,1]],longitude:1}]);
 const y=r.prepare(kind,[{id:'asset-b',resourceSha256:sha,polygon:[[0,0],[1,0],[1,1]],longitude:2}]);assert.equal(x[0].id,y[0].id);assert.equal(y[0].assetId,'asset-b','downloads still use the current project asset ID');assert.equal(y[0].longitude,2);
 const z=r.prepare(kind,[{id:'asset-b',resourceSha256:other,polygon:[[0,0],[1,0],[1,1]]}]);assert.notEqual(z[0].id,y[0].id,'changed content must be replaced');r.prepare(kind,[]);assert.equal(r.logical(kind,z[0].id),null);
}
r.prepare('tilesets',[]);assert.equal(r.logical('tilesets',changed[0].id),null);
console.log('PASS resource diff: common URL/hash retained, old-only and changed resources removed, settings updated, current project IDs preserved');
