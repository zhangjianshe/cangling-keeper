/* Export a readable snapshot, without credentials, private contexts or binary attachments. */
(() => {
  const plain=value=>String(value || '').replace(/[\r\n]+/g,' ').replace(/[\\`*_{}\[\]<>#]/g,'\\$&');
  window.keeperConversationMarkdown=detail=>{
    const messages=detail.messages || [],groups=detail.replyGroups || [],alternatives=new Set(groups.flatMap(g=>(g.allVersionIds || g.versionIds || []).slice(1)));
    const lines=['# '+plain(detail.session?.title || '会话'),'','> 当前选中版本的阅读副本；附件仅列名称，完整恢复请使用对话备份。',''];
    if(detail.modelPending)lines.push('> 导出时回复仍在生成，以下为当前已保存的内容。','');
    for(const root of messages){
      if(alternatives.has(root.id))continue;
      const group=groups.find(g=>g.rootId===root.id),entry=group?messages.find(m=>m.id===group.selectedId):root;if(!entry)continue;
      const meta=entry.modelMetadata || {},role={user:'用户',assistant:'助手',error:'错误'}[entry.role] || '消息';
      lines.push('## '+role,'');
      if(meta.model)lines.push('模型：'+plain([meta.serverName,meta.model].filter(Boolean).join(' · ')),'');
      if(Number.isFinite(entry.createdAt) && entry.createdAt>0)lines.push('时间：'+new Date(entry.createdAt).toISOString(),'');
      if(entry.content)lines.push(entry.content,'');
      for(const attachment of entry.attachments || [])lines.push('- 附件：'+plain(attachment.name)+'（'+Number(attachment.size || 0)+' 字节）');
      lines.push('');
    }
    return lines.join('\n');
  };
})();
