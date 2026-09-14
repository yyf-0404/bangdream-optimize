import {createProfileMarkup} from './approved/archive-flows.js';
import {createArchiveFlow,flowHelpers} from './archive-flows.js';

export function requestProfileDetails({name,server='cn',copy=false}){
 const {dialog,body}=createArchiveFlow(copy?'复制档案':'新建档案','档案管理');
 body.innerHTML=createProfileMarkup({...flowHelpers,name,copy:copy?{server}:null});
 const form=body.querySelector('form'),input=body.querySelector('#create-name');form.noValidate=false;
 body.querySelectorAll('[data-close-flow]').forEach(b=>b.onclick=()=>dialog.close());
 let result=null;
 form.onsubmit=e=>{e.preventDefault();if(!form.reportValidity()||!input.value.trim()){body.querySelector('#create-error').textContent='请填写档案名称';input.focus();return;}result={name:input.value.trim(),server:form.querySelector('[name=create-server]:checked').value,activate:body.querySelector('#activate-created').checked};dialog.close();};
 return new Promise(resolve=>{dialog.addEventListener('close',()=>resolve(result),{once:true});dialog.showModal();input.focus();input.select();});
}
