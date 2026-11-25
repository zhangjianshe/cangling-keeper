import {keeper_command} from './pkg/cangling_desktop.js';
const call=(method,args={})=>JSON.parse(keeper_command(method,JSON.stringify(args)));
const indicator=document.createElement('div');indicator.className='earth-navigation-mode';indicator.setAttribute('role','img');document.body.append(indicator);
const icons={orbit:'<circle cx="12" cy="12" r="9"/><ellipse cx="12" cy="12" rx="4" ry="9"/><path d="M3 12h18"/>',first:'<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>',third:'<circle cx="12" cy="5" r="3"/><path d="M7 21v-5a5 5 0 0 1 10 0v5M3 10v8m18-8v8"/>'};
let walking=false,ready=false,dragging=false,lastCheck=0,speed=12,thirdPerson=false;const keys=new Set(),speeds=[3,12,30,100];
try{const saved=JSON.parse(localStorage.getItem('earth.walk.preferences')||'{}');if(speeds.includes(saved.speed))speed=saved.speed;thirdPerson=saved.thirdPerson===true;}catch{}
function remember(){try{localStorage.setItem('earth.walk.preferences',JSON.stringify({speed,thirdPerson}));}catch{}}
function render(){const mode=walking?(thirdPerson?'third':'first'):'orbit';indicator.dataset.mode=mode;indicator.innerHTML=`<svg viewBox="0 0 24 24" aria-hidden="true">${icons[mode]}</svg>`;indicator.title=(walking?`${thirdPerson?'第三人称':'第一人称'}漫游 · ${speed} m/s · P 切换人称 · 右键释放鼠标`:'地球浏览')+' · M 切换漫游 · F1 帮助';indicator.setAttribute('aria-label',indicator.title);}
const controls=(x=0,y=0)=>{if(!walking)return;try{call('walk_controls',{forward:Number(keys.has('KeyW'))-Number(keys.has('KeyS')),right:Number(keys.has('KeyD'))-Number(keys.has('KeyA')),sprint:keys.has('ShiftLeft')||keys.has('ShiftRight'),x,y});}catch{}};
const stop=()=>{keys.clear();dragging=false;controls();};
function release(){stop();if(document.pointerLockElement)document.exitPointerLock();}
function state(value){if(walking&&!value)release();walking=value;render();}
function change(args){try{const value=call('navigation_mode',args);state(value.walking);remember();}catch(e){indicator.title=String(e);indicator.setAttribute('aria-label',String(e));}}
render();
window.addEventListener('cangling-earth-ready',()=>{ready=true;});
window.addEventListener('cangling-earth-frame',()=>{if(!ready||performance.now()-lastCheck<300)return;lastCheck=performance.now();try{const value=call('navigation_mode').walking;if(value!==walking)state(value);}catch{}});
const movement=new Set(['KeyW','KeyA','KeyS','KeyD','ShiftLeft','ShiftRight']);
window.addEventListener('keydown',e=>{
 if(!ready||e.ctrlKey||e.metaKey||e.altKey||e.target.closest?.('input,textarea,select,[contenteditable]:not([contenteditable="false"])'))return;
 if(e.code==='KeyM'||(walking&&['KeyP','BracketLeft','BracketRight'].includes(e.code))){
  e.preventDefault();e.stopImmediatePropagation();if(e.repeat)return;stop();
  if(e.code==='KeyM'){change({walking:!walking,speed,thirdPerson});document.querySelector('canvas')?.focus();}
  else if(e.code==='KeyP'){thirdPerson=!thirdPerson;change({thirdPerson});}
  else{speed=speeds[Math.max(0,Math.min(speeds.length-1,speeds.indexOf(speed)+(e.code==='BracketRight'?1:-1)))];change({speed});}
  return;
 }
 if(!walking)return;
 if(e.code==='Space'){e.preventDefault();e.stopImmediatePropagation();if(!e.repeat)try{call('walk_jump');}catch{}return;}
 if(movement.has(e.code)){e.preventDefault();e.stopImmediatePropagation();keys.add(e.code);controls();}
 if(e.code==='Escape'||e.code==='F1')release();
},true);
window.addEventListener('keyup',e=>{if(movement.has(e.code)){keys.delete(e.code);controls();}},true);
window.addEventListener('blur',stop);document.addEventListener('visibilitychange',()=>{if(document.hidden)stop();});
document.addEventListener('pointerlockchange',()=>{if(!document.pointerLockElement)stop();});
document.addEventListener('pointerdown',e=>{if(!walking||e.target.tagName!=='CANVAS')return;if(e.button===2){e.preventDefault();release();return;}if(e.button!==0)return;dragging=true;e.target.focus();try{const request=e.target.requestPointerLock?.();request?.catch(()=>{});}catch{}});
document.addEventListener('contextmenu',e=>{if(walking&&e.target.tagName==='CANVAS'){e.preventDefault();release();}});
document.addEventListener('pointerup',()=>{dragging=false;});
document.addEventListener('pointermove',e=>{if(walking&&(document.pointerLockElement?.tagName==='CANVAS'||dragging))controls(e.movementX,e.movementY);});
