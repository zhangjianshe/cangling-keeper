import {keeper_command} from './pkg/cangling_desktop.js';
const call=(method,args={})=>JSON.parse(keeper_command(method,JSON.stringify(args)));
const panel=document.createElement('section');panel.className='earth-navigation-mode';panel.setAttribute('aria-label','地球视图模式');
const button=document.createElement('button');button.type='button';button.textContent='地面漫游';button.disabled=true;button.setAttribute('aria-pressed','false');
const hint=document.createElement('span');hint.hidden=true;hint.textContent='点击画面控制方向 · WASD 行走 · Shift 四倍加速 · Esc 释放鼠标（暂不含建筑碰撞）';hint.setAttribute('role','status');panel.append(button,hint);document.body.append(panel);
let walking=false,ready=false,dragging=false,busy=false,lastCheck=0;const keys=new Set();
const settings=document.createElement('div');settings.className='earth-walk-settings';settings.hidden=true;
const speedLabel=document.createElement('label');speedLabel.textContent='移动速度 ';const speed=document.createElement('select');speed.setAttribute('aria-label','漫游移动速度');
for(const [value,text] of [[3,'慢速 3 m/s'],[12,'标准 12 m/s'],[30,'快速 30 m/s'],[100,'巡航 100 m/s']]){const option=document.createElement('option');option.value=String(value);option.textContent=text;speed.append(option);}speed.value='12';
const perspective=document.createElement('button');perspective.type='button';perspective.textContent='切换第三人称';let thirdPerson=false;
try{const saved=JSON.parse(localStorage.getItem('earth.walk.preferences')||'{}');if([3,12,30,100].includes(saved.speed))speed.value=String(saved.speed);thirdPerson=saved.thirdPerson===true;}catch{}
function remember(){try{localStorage.setItem('earth.walk.preferences',JSON.stringify({speed:Number(speed.value),thirdPerson}));}catch{}}
function updatePerspective(){perspective.textContent=thirdPerson?'切换第一人称':'切换第三人称';perspective.setAttribute('aria-pressed',String(thirdPerson));}
updatePerspective();speedLabel.append(speed);settings.append(speedLabel,perspective);panel.insertBefore(settings,hint);
speed.onchange=()=>{stop();call('navigation_mode',{speed:Number(speed.value)});remember();};
perspective.onclick=()=>{stop();thirdPerson=!thirdPerson;call('navigation_mode',{thirdPerson});updatePerspective();remember();};

const controls=(x=0,y=0)=>{if(!walking)return;try{call('walk_controls',{forward:Number(keys.has('KeyW'))-Number(keys.has('KeyS')),right:Number(keys.has('KeyD'))-Number(keys.has('KeyA')),sprint:keys.has('ShiftLeft')||keys.has('ShiftRight'),x,y});}catch{}};
const stop=()=>{keys.clear();dragging=false;controls();};
function state(value){if(walking&&!value){stop();if(document.pointerLockElement)document.exitPointerLock();}walking=value;button.textContent=value?'返回地球浏览':'地面漫游';button.setAttribute('aria-pressed',String(value));hint.hidden=!value;settings.hidden=!value;}
button.onclick=()=>{if(!ready||busy)return;busy=true;try{state(call('navigation_mode',{walking:!walking,speed:Number(speed.value),thirdPerson}).walking);if(walking)document.querySelector('canvas')?.focus();}catch(e){hint.hidden=false;hint.textContent=String(e);}finally{busy=false;}};
window.addEventListener('cangling-earth-ready',()=>{ready=true;button.disabled=false;});
window.addEventListener('cangling-earth-frame',()=>{if(!ready||performance.now()-lastCheck<300)return;lastCheck=performance.now();try{const value=call('navigation_mode').walking;if(value!==walking)state(value);}catch{}});
const movement=new Set(['KeyW','KeyA','KeyS','KeyD','ShiftLeft','ShiftRight']);
window.addEventListener('keydown',e=>{if(!walking||e.ctrlKey||e.metaKey||e.altKey||e.target.closest?.('input,textarea,select,[contenteditable="true"]'))return;if(movement.has(e.code)){e.preventDefault();e.stopImmediatePropagation();keys.add(e.code);controls();}if(e.code==='Escape')stop();},true);
window.addEventListener('keyup',e=>{if(movement.has(e.code)){keys.delete(e.code);controls();}},true);
window.addEventListener('blur',stop);document.addEventListener('visibilitychange',()=>{if(document.hidden)stop();});
document.addEventListener('pointerlockchange',()=>{if(!document.pointerLockElement)stop();});
document.addEventListener('pointerdown',e=>{if(!walking||e.target.tagName!=='CANVAS'||e.button!==0)return;dragging=true;e.target.focus();try{const request=e.target.requestPointerLock?.();request?.catch(()=>{});}catch{}});
document.addEventListener('pointerup',()=>{dragging=false;});
document.addEventListener('pointermove',e=>{if(walking&&(document.pointerLockElement?.tagName==='CANVAS'||dragging))controls(e.movementX,e.movementY);});
