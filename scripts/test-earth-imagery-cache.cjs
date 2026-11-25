const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
let matches=0,puts=0;const requests=[],replies=[];
const ctx={Request,Response,Uint8Array,self:{caches:{},postMessage:v=>replies.push(v)},caches:{open:async()=>({match:async()=>{matches++;return new Response('old')},put:async()=>puts++})},fetch:async r=>{requests.push(r);return new Response('new')}};
vm.runInNewContext(fs.readFileSync('ui/earth/data-worker.js','utf8'),ctx);
(async()=>{
 await ctx.self.onmessage({data:{id:1,url:'https://example.test/18/1/2.png',cacheName:'cangling-imagery-agri-v1'}});
 assert.equal(new TextDecoder().decode(replies[0].bytes),'new');assert.equal(requests[0].cache,'no-cache');assert.equal(matches,0);assert.equal(puts,0);
 await ctx.self.onmessage({data:{id:2,url:'https://example.test/terrain',cacheName:'terrain-v1'}});
 assert.equal(new TextDecoder().decode(replies[1].bytes),'old');assert.equal(matches,1);assert.equal(requests.length,1);
 ctx.fetch=async()=>new Response('error',{status:503});
 await ctx.self.onmessage({data:{id:3,url:'https://example.test/18/1/2.png',cacheName:'cangling-imagery-agri-v1'}});
 assert.match(replies[2].error,/503/);assert.equal(matches,1,'network error must not silently reuse stale imagery');
 console.log('PASS mutable imagery revalidation, immutable cache reuse, no stale fallback');
})().catch(e=>{console.error(e);process.exitCode=1});
