(() => {
 window.createKeeperPlugins=({agent,invoke,renderMcp,status})=>{
  const add=document.querySelector('#mcp-add'),button=document.createElement('button'),file=document.createElement('input');
  button.type='button';button.className='btn';button.textContent='安装插件清单';file.type='file';file.accept='.json,application/json';file.hidden=true;add.before(button,file);button.onclick=()=>file.click();
  const element=(tag,text)=>{const node=document.createElement(tag);node.textContent=text;return node;};
  async function review(manifest){
   const epoch=agent.identityEpoch||0,project=agent.project,current=()=>epoch===(agent.identityEpoch||0)&&project===agent.project;
   const projectIds=[project],preview=await invoke('agent_plugin_preview',{manifest,projectIds});if(!current())return;
   const dialog=document.createElement('dialog');dialog.className='keeper-project-dialog';
   dialog.append(element('h2','安装 '+preview.manifest.name),element('p','版本 '+preview.manifest.version+' · 项目：'+(agent.projects.find(p=>p.id===project)?.name||project)),element('p','来源声明（未验证）：'+preview.manifest.source),element('p','规范化清单 SHA-256：'+preview.sha256));
   const local=preview.manifest.permissions.includes('executeLocal');
   dialog.append(element('p',local?'权限：运行本机进程。服务使用 Keeper 当前用户权限，可访问其文件与网络；该声明不是操作系统权限隔离。':'权限：连接远程 MCP 服务。调用时会向该地址发送工具参数。'));
   const pre=element('pre',JSON.stringify(preview.manifest.mcp,null,2));pre.style.cssText='max-height:220px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere';dialog.append(pre);
   dialog.append(element('p','仅安装当前账号的连接配置，不下载程序、不执行命令。安装后停用，需点击“启用并发现”连接；卸载仅删除配置，保留项目文件和历史记录。'));
   const label=document.createElement('label'),check=document.createElement('input');check.type='checkbox';label.append(check,document.createTextNode('我已核实来源和完整配置，允许安装'));dialog.append(label);
   const message=element('p','');message.setAttribute('role','status');const actions=document.createElement('div');actions.className='keeper-project-actions';const cancel=element('button','取消'),install=element('button','安装');cancel.type=install.type='button';cancel.className=install.className='btn';install.disabled=true;check.onchange=()=>install.disabled=!check.checked;cancel.onclick=()=>dialog.close();
   install.onclick=async()=>{if(!check.checked||!current()){message.textContent='账号或项目已切换，请重新导入清单';return;}install.disabled=true;cancel.disabled=true;check.disabled=true;try{const saved=await invoke('agent_plugin_install',{manifest:preview.manifest,projectIds,review:preview.review,confirmed:true});if(current()){renderMcp(saved.servers);status('插件已安装，尚未连接或启动。');}dialog.close();}catch(e){message.textContent=String(e);install.disabled=false;}finally{cancel.disabled=false;check.disabled=false;}};
   dialog.addEventListener('cancel',e=>{if(cancel.disabled)e.preventDefault();});dialog.addEventListener('close',()=>dialog.remove());actions.append(cancel,install);dialog.append(message,actions);document.body.append(dialog);dialog.showModal();
  }
  file.onchange=async()=>{const selected=file.files?.[0],epoch=agent.identityEpoch||0;file.value='';if(!selected)return;try{if(selected.size>32768)throw Error('插件清单最多32 KiB');const manifest=JSON.parse(await selected.text());if(epoch!==(agent.identityEpoch||0))return;await review(manifest);}catch(e){if(epoch===(agent.identityEpoch||0))status(String(e));}};
  return {review};
 };
})();
