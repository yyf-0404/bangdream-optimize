function resultDialog(title){
 const dialog=document.createElement('dialog');dialog.className='accepted-result-dialog';dialog.setAttribute('aria-label',title);
 dialog.innerHTML='<header class="dialog-top"><h2></h2><button type="button" class="icon-button" aria-label="关闭'+title+'">×</button></header><div class="dialog-content"></div><footer class="dialog-bottom"></footer>';
 dialog.querySelector('h2').textContent=title;dialog.querySelector('header button').onclick=()=>dialog.close();
 const trigger=document.activeElement;dialog.onclose=()=>{dialog.remove();trigger?.focus({preventScroll:true});};document.querySelector('#result-design').append(dialog);
 return dialog;
}
function action(dialog,label,onClick,primary=false){const b=document.createElement('button');b.type='button';b.className=primary?'primary':'text-button';b.textContent=label;b.onclick=onClick;dialog.querySelector('footer').append(b);return b;}
export function showResultDiagnostics(diagnostic){
 const dialog=resultDialog('计算诊断'),body=dialog.querySelector('.dialog-content');
 const text=document.createElement('p');text.textContent=diagnostic?.error?.title||'本次计算的状态、阶段与完整诊断数据。';body.append(text);
 const detail=document.createElement('dl');detail.className='detail-grid';
 for(const [name,value] of [['结果状态',diagnostic?(diagnostic.error?'计算失败':'计算完成'):'尚未计算'],['运行阶段',diagnostic?.phase||'—'],['运行环境',diagnostic?.runtime||'—']]){const cell=document.createElement('div'),label=document.createElement('dt'),content=document.createElement('dd');label.textContent=name;content.textContent=value;cell.append(label,content);detail.append(cell);}body.append(detail);
 if(diagnostic){const raw=document.createElement('details'),summary=document.createElement('summary'),pre=document.createElement('pre');summary.textContent='查看原始数据';pre.className='technical';pre.textContent=JSON.stringify(diagnostic,null,2);raw.append(summary,pre);body.append(raw);action(dialog,'导出诊断',()=>{dialog.close();document.querySelector('#export-diagnostics').click();});action(dialog,'反馈问题',()=>{dialog.close();document.querySelector('#feedback-result').click();});}
 action(dialog,'关闭',()=>dialog.close());dialog.showModal();
}
