(() => {
 window.keeperMcpImagePreview=(card,run,session,invoke)=>{
  let output;try{output=JSON.parse(run.output || '{}');}catch{return;}
  if(!Array.isArray(output.mcpImages))return;
  for(const reference of output.mcpImages.slice(0,4)){
   if(!/^[a-f0-9]{64}$/.test(reference.sha256))continue;
   const section=document.createElement('div'),button=document.createElement('button');button.type='button';button.className='btn';button.textContent='预览图片 · '+(reference.width || '?')+' × '+(reference.height || '?');section.append(button);card.append(section);
   button.onclick=async()=>{button.disabled=true;try{
    const image=await invoke('agent_mcp_image',{sessionId:session,sha256:reference.sha256});
    if(!section.isConnected)return;
    if(!['image/png','image/jpeg'].includes(image.mimeType)||typeof image.data!=='string'||image.data.length>1398104||!/^[A-Za-z0-9+/]*={0,2}$/.test(image.data))throw Error('图片格式无效');
    const img=document.createElement('img');img.alt='MCP 工具返回的图片';img.style.maxWidth='100%';img.style.maxHeight='360px';img.src='data:'+image.mimeType+';base64,'+image.data;section.replaceChildren(img);
   }catch(error){button.disabled=false;button.textContent='预览失败，点击重试';button.title=String(error);}};
  }
 };
})();
