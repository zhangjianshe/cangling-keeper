/* Form-only edits for MCP project scope and private stdio environment variables. */
(() => {
  'use strict';
  window.createMcpSettingsForm=({element})=>{
    const $=s=>document.querySelector(s),fields=$('#mcp-form .llm-dialog-fields');let removed=new Set();
    const scope=element('fieldset','mcp-setting-section');scope.id='mcp-project-scope';scope.append(element('legend','','启用项目'));
    const scopeLabel=element('label','','适用范围'),mode=element('select');mode.id='mcp-project-mode';mode.setAttribute('aria-label','MCP 项目范围');mode.append(new Option('所有项目','all'),new Option('指定项目','selected'));scopeLabel.append(mode);scope.append(scopeLabel);
    const projectList=element('div','mcp-project-options');projectList.id='mcp-project-options';scope.append(projectList,element('p','hint','仅向这些项目的会话提供此服务的工具、资料和模板。'));
    const environment=element('fieldset','mcp-setting-section');environment.id='mcp-environment';environment.append(element('legend','','环境变量'),element('p','hint','仅传给本机 MCP 子进程，配置保存在本机 SQLite。变量值不会回显，也不会随工具声明发送给模型。'));
    const rows=element('div','mcp-environment-rows');rows.id='mcp-environment-rows';const add=element('button','btn','＋ 添加环境变量');add.type='button';add.id='mcp-env-add';environment.append(rows,add);fields.append(scope,environment);
    function addRow(key='',existing=false){
      const row=element('div','mcp-env-row');row.dataset.existing=String(existing);row.dataset.savedKey=existing?key:'';
      const name=element('input');name.type='text';name.value=key;name.readOnly=existing;name.maxLength=128;name.placeholder='变量名，例如 API_KEY';name.setAttribute('aria-label','环境变量名称');name.autocomplete='off';
      const value=element('input');value.type='password';value.maxLength=8192;value.placeholder=existing?'已保存，留空保留':'变量值';value.setAttribute('aria-label',key?'环境变量 '+key+' 的值':'环境变量值');value.autocomplete='new-password';
      const remove=element('button','mcp-env-remove','×');remove.type='button';remove.setAttribute('aria-label',key?'删除环境变量 '+key:'删除环境变量');remove.onclick=()=>{if(existing)removed.add(key);value.value='';row.remove();};row.append(name,value,remove);rows.append(row);return {row,name,value};
    }
    function update(){projectList.hidden=mode.value!=='selected';environment.hidden=$('#mcp-transport').value!=='stdio';}
    function fill(server,projects=[]){
      reset();mode.value=Array.isArray(server?.projectIds)?'selected':'all';
      for(const project of projects){const label=element('label','mcp-project-option'),check=element('input');check.type='checkbox';check.value=project.id;check.checked=Array.isArray(server?.projectIds) && server.projectIds.includes(project.id);label.append(check,element('span','',project.name));projectList.append(label);}
      if(!projects.length)projectList.append(element('p','hint','暂无可选择项目，请先创建项目。'));
      for(const key of server?.environmentKeys || [])addRow(key,true);update();
    }
    function args(transport){
      const projectIds=mode.value==='all'?null:[...projectList.querySelectorAll('input:checked')].map(input=>input.value);if(Array.isArray(projectIds) && !projectIds.length)throw new Error('请选择至少一个启用项目。');
      const output={projectIds};if(transport!=='stdio')return output;
      const changes=Object.create(null),names=new Set();for(const name of removed)changes[name]=null;
      for(const row of rows.children){const [name,value]=row.querySelectorAll('input'),key=name.value.trim(),saved=row.dataset.existing==='true';if(!key && !value.value && !saved)continue;
        if(!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(key))throw new Error('环境变量名只能使用字母、数字和下划线，并以字母或下划线开头。');
        if(names.has(key))throw new Error('环境变量名称不能重复。');names.add(key);
        if(!saved || value.value!=='')changes[key]=value.value;
      }
      if(Object.keys(changes).length)output.environmentChanges=changes;return output;
    }
    function reset(){for(const input of rows.querySelectorAll('input'))input.value='';rows.replaceChildren();removed.clear();projectList.replaceChildren();mode.value='all';projectList.hidden=true;}
    add.onclick=()=>addRow().name.focus();mode.onchange=update;return {fill,args,update,reset};
  };
})();
