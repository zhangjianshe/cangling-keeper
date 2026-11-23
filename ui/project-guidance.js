/* Project guidance shares the project file editor and its save safeguards. */
(() => {
 'use strict';
 window.createKeeperGuidance=({agent,element})=>{
  const trigger=element('button','guidance-trigger','项目指南');trigger.id='agent-guidance-open';trigger.type='button';trigger.disabled=true;
  document.querySelector('.agent-header-tools').prepend(trigger);
  async function open(){const project=agent.projects.find(p=>p.id===(agent.detail.session?.projectId||agent.project));if(project&&agent.selected)await agent.projectBrowser?.openGuidance(project);}
  trigger.onclick=open;
  return {open,update(){trigger.disabled=!agent.selected;},reset(){trigger.disabled=true;}};
 };
})();
