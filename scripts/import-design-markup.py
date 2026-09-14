"""Copy approved HTML and render templates; no preview sessions or fixture data.

Run with the same source workspace as sync-approved-design.py. Keeping the
templates generated makes markup changes reviewable against the design source.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path
from bs4 import BeautifulSoup

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
source = parser.parse_args().source.resolve()
dest = Path(__file__).resolve().parents[1] / 'apps/web/src/ui/approved'
dest.mkdir(parents=True, exist_ok=True)
templates = {}
manifest = {}

def read(name):
    content = (source / name).read_text(encoding='utf-8')
    manifest[name] = hashlib.sha256(content.encode()).hexdigest()
    return content

def document(page):
    return BeautifulSoup(read('boundary-design/' + page + '.html'), 'html.parser')

def markup(name, node):
    for el in node.select('script,style,.page-footer,.bo-mobile-action'):
        el.decompose()
    # Preserve SVG attribute casing after HTML parsing.
    templates[name] = str(node).replace('viewbox=', 'viewBox=')

markup('activity-target', document('activity').select_one('.bo-content > .bo-section'))
cards = document('cards')
markup('card-filters', cards.select_one('#filters'))
markup('card-results', cards.select_one('#results'))
player = document('player')
markup('player-area', player.select_one('#area-section'))
markup('player-characters', player.select_one('#character-section'))
markup('archive-directory', document('archive').select_one('.profile-directory'))
markup('archive-import-guide', document('archive').select_one('.import-guide'))
markup('confirm-dialog', document('archive').select_one('#confirm-dialog'))
markup('flow-dialog', document('archive').select_one('#flow-dialog'))
markup('result-content', document('result').select_one('.result-content'))

# The profile form is filled by the real archive controller after mounting.
archive = read('shared-design/shared.js')
form = re.search(r'<form id="profile-form".*?</form>', archive, re.S).group(0)
form = re.sub(r'\$\{escapeHtml\(p\.(?:name|playerId)\)\}', '', form)
form = form.replace("${serverChoices('profile-server',p.server)}", '<div class="server-choices"></div>')
templates['archive-form'] = form.replace('<form ', '<div ').replace('</form>', '</div>')
flow_functions=[]
step=re.search(r'const stepList = .*?;\n',archive).group(0)
for function,name in [('openCreate','createProfileMarkup'),('renderImport','importSourceMarkup'),('renderImportReview','importReviewMarkup'),('renderImportDone','importDoneMarkup'),('openExport','exportMarkup')]:
    section=archive[archive.index('function '+function+'('):]
    line=next(line for line in section.splitlines() if 'showFlow(' in line)
    body=re.search(r'showFlow\([^,]+,(`.*`),.*\);',line).group(1)
    body=re.sub(r'<div class="sample-tools">.*?</div>', '', body)
    body=re.sub(r'<label class="compact-check"><input id="source-error-toggle".*?</label>', '', body)
    body=body.replace(' · 演示资料','').replace('以上数量为设计样例。','').replace('（仅为设计示意）','')
    body=body.replace('<p class="dialog-note">本预览的复制和下载仅展示反馈，不操作剪贴板或生成真实配置。</p>','')
    body=body.replace('<p class="dialog-note">交互演示已完成，未读取或修改真实游戏账号。</p>','')
    body=body.replace('../card-library-unified/index.html','#cards').replace('../aurora-soft-study.html','#activity')
    # All regions use public Bestdori profiles; never restore the retired CN importer hint.
    body=body.replace("${d.server==='cn'?'读取国服账号配置':'仅导入主乐队的公开资料'}", '仅导入主乐队的公开资料')
    body=body.replace("${d.server==='cn'?'导入账号返回的卡牌、区域道具和角色加成；相同项目更新，未返回的项目保留。':'通过 Bestdori 读取主乐队卡牌、对应角色加成与已装备道具。公开资料不包含完整持有卡牌列表。'}", '通过 Bestdori 读取主乐队卡牌、对应角色加成与已装备道具。公开资料不包含完整持有卡牌列表。')
    flow_functions.append('export function '+name+'({d,p,r,name="",copy=null,isNew=false,compact=true,payload="",escapeHtml,serverChoices,flagInline,flag,icon,importScope,cardStrip=()=>"",servers,number=String}) {\n'+step+'return '+body+';\n}\n')
archive_icons=archive[archive.index('const paths ='):archive.index('function hydrate(')].replace('const icon =','export const archiveIcon =')
(dest/'archive-flows.js').write_text('// Copied final archive flow markup, with demonstration controls removed.\n'+archive_icons+''.join(flow_functions),encoding='utf-8')
feedback=read('preview-feedback.js')
feedback=re.search(r'dialog.innerHTML=`(.*?)`;\n',feedback,re.S).group(1)
feedback=re.sub(r'\$\{esc\(draft\.\w+\)\}', '', feedback)
feedback=feedback.replace('${icon}', '<span data-icon="message"></span>')
feedback=re.sub(r'<div class="pf-options">.*?</div>', '', feedback, flags=re.S)
feedback=re.sub(r'\$\{diagnostic\?.*?\}', '', feedback)
templates['feedback-dialog']='<dialog id="feedback-dialog" aria-labelledby="pf-title">'+feedback+'</dialog>'
(dest/'feedback.css').write_text(read('preview-feedback.css').replace('#preview-feedback-dialog','#feedback-dialog'),encoding='utf-8')

# Keep the final presentation-only result decorator, excluding preview startup.
decoration=read('unified-design/result-decoration.js')
start=decoration.index(' function displayEquipment()')
end=decoration.index(' // Only direct renderer replacements')
(dest/'result-decoration.js').write_text(
    '// Copied presentation functions; no fixture values or preview handlers.\n'
    + 'export function decorateResult(document) {\n' + decoration[start:end]
    + '\n decorate();\n}\n', encoding='utf-8')

# The final design creates these fragments at runtime. Copy its template
# functions verbatim, supplying only real application state at the boundary.
conditions = read('preview-conditions.js')
templates['activity-live'] = '<section id="bo-live-settings" class="bo-section pc-section">' + re.search(r"live.innerHTML='(.*?)';", conditions).group(1) + '</section>'
equipment = re.search(r'dialog.innerHTML=`(.*?)`;\n', conditions, re.S).group(1)
equipment = equipment.replace('${esc(profile.name)}', '').replace("${session.url('player',{return:'activity'})}", '#player')
templates['equipment-dialog'] = '<dialog id="pc-equipment-dialog" aria-labelledby="pc-equipment-title">'+equipment+'</dialog>'
teams = read('preview-teams.js')
templates['team-dialog'] = '<dialog id="team-picker" class="shared-card-library" aria-labelledby="pt-picker-title" aria-describedby="pt-picker-context">'+re.search(r'dialog.innerHTML=`(.*?)`;\n', teams, re.S).group(1)+'</dialog>'
bulk = read('card-library-unified/bulk.mjs')
bulk_helpers = bulk[bulk.index(' const row='):bulk.index(' function open(command,button)')]
bulk_body = re.search(r'  dialog.innerHTML=(`.*?`);\n', bulk, re.S).group(1)
(dest/'bulk-template.js').write_text('// Copied approved bulk dialog markup; persistence is bound separately.\nexport function bulkMarkup({action,reviewIds,owned,missing,eligible,isClear=false}) {\n'+bulk_helpers+'\nreturn '+bulk_body+';\n}\n',encoding='utf-8')
shell = read('unified-design/shell.js')
sidebar = re.search(r'mount.innerHTML=(`.*?`);\n', shell, re.S).group(1)
icons=shell[shell.index(' const paths='):shell.index(' const legacy=')].replace(' const svg=', ' export const sidebarIcon=')
(dest/'sidebar-template.js').write_text('// Copied sidebar HTML and SVG; hrefs and labels are supplied by the SPA.\n'+icons+'\nexport function sidebarMarkup({session,page,svg}) {return '+sidebar+';}\n',encoding='utf-8')
(dest/'sidebar.css').write_text(read('unified-design/sidebar.css'),encoding='utf-8')
radio = re.search(r' function radio\(.*?\n', conditions).group(0)
live_body = re.search(r"\$\('pc-live-fields'\).innerHTML=(`.*?`);\n", conditions).group(1)
live_note = re.search(r"\$\('pc-score-note'\).textContent=(.*?);\n", conditions).group(1)
(dest / 'live-template.js').write_text(
    '// Generated verbatim from preview-conditions.js; real data is supplied by the caller.\n'
    + 'export function liveMarkup({a,allowed,config,profile,eventType,R}) {\n' + radio
    + 'return {fields:' + live_body + ',note:' + live_note + '};\n}\n', encoding='utf-8')

calculation = read('preview-calculation-settings.js')
helpers = calculation[calculation.index(' const escape='):calculation.index(' function render(){')]
helpers = helpers.replace(" const write=(path,v)=>{const keys=path.split('.'),last=keys.pop();keys.reduce((o,k)=>o[k],state)[last]=v;};\n", '')
body = calculation[calculation.index("  let body='';"):calculation.index('  feedback();')]
body = body.replace('  section.innerHTML=', '  return ')
(dest / 'calculation-template.js').write_text(
    '// Generated verbatim from preview-calculation-settings.js.\n'
    + 'export function calculationMarkup({state,server,m,t,l,R}) {\n' + helpers
    + 'const p=state.ptMaximize;\n' + body + '\n}\n', encoding='utf-8')
(dest / 'templates.js').write_text(
    '// Generated from the approved design HTML. Edit the source or import bindings, not these fragments.\n'
    + 'export const designTemplates = ' + json.dumps(templates, ensure_ascii=False, indent=2) + ';\n'
    + 'export function designFragment(name) { const t=document.createElement("template");t.innerHTML=designTemplates[name];return t.content.firstElementChild;}\n', encoding='utf-8')
(dest / 'markup-sources.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
print('Copied', len(templates), 'HTML fragments and two render templates.')
