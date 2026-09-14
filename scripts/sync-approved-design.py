"""Import the accepted visual styles, scoped to each real application page.

This development-only script needs the local design workspace. It deliberately
does not import fixture/session scripts or demonstration data into the app.
"""
import argparse
import re
from pathlib import Path

import tinycss2
from bs4 import BeautifulSoup

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
args = parser.parse_args()
source = args.source.resolve()
destination = Path(__file__).resolve().parents[1] / 'apps/web/src/ui/approved'
destination.mkdir(parents=True, exist_ok=True)

pages = {
 'activity': ('#aurora-soft-study', ['@activity', 'card-library-unified/shared.css', 'preview-songs.css', 'preview-teams.css', 'preview-conditions.css', 'preview-calculation-settings.css', 'preview-main-band.css', 'preview-detail-style.css', 'preview-difficulty.css', 'preview-bonuses.css', 'activity-body-study/ink.css', 'preview-boundaries.css']),
 'cards': ('#card-library-unified', ['card-library-unified/toolbar.css', 'card-library-unified/shared.css', 'card-library-unified/unified.css', 'card-body-study/study.css', 'card-library-unified/bulk.css']),
 'player': ('#player-library', ['player-design/study.css', 'player-design/compact.css', 'player-design/material-polish.css']),
 'result': ('#result-design', ['player-design/study.css', 'result-design/result.css', 'preview-conditions.css', 'preview-difficulty.css', 'result-body-study/study.css', 'result-body-study/record.css', 'result-body-study/composition.css', 'result-body-study/coherent.css']),
 'archive': ('#archive-design', ['player-design/study.css', 'shared-design/shared.css', 'archive-body-study/study.css']),
}

def scope_selector(selector, scope):
    selector = re.sub(r'html\[data-(?:activity|cards)-skin=[^\]]+\]\s*', '', selector)
    selector = re.sub(r'html\[data-preview-page=[^\]]+\]\s*', '', selector)
    selector = re.sub(r'html\[data-preview-page\]\s*', '', selector)
    selector = re.sub(r'(?<![\w-])(?::root|html|body)(?![\w-])', scope, selector)
    if scope not in selector:
        selector = scope + ' ' + selector
    return selector

def rules(css, scope):
    output=[]
    for rule in tinycss2.parse_stylesheet(css, skip_whitespace=True, skip_comments=True):
        if rule.type == 'qualified-rule':
            groups=[[]]
            for token in rule.prelude:
                if token.type == 'literal' and token.value == ',': groups.append([])
                else: groups[-1].append(token)
            selectors=[scope_selector(tinycss2.serialize(g).strip(), scope) for g in groups]
            output.append(','.join(selectors)+'{'+tinycss2.serialize(rule.content)+'}')
        elif rule.type == 'at-rule':
            if rule.lower_at_keyword == 'import': continue  # Shared card presentation is loaded once by design.css.
            head='@'+rule.at_keyword+' '+tinycss2.serialize(rule.prelude)
            if rule.content is None: output.append(head+';')
            elif rule.lower_at_keyword in ['media','supports','container','layer']:
                output.append(head+'{'+rules(tinycss2.serialize(rule.content),scope)+'}')
            else: output.append(head+'{'+tinycss2.serialize(rule.content)+'}')
    return '\n'.join(output)

for page,(scope,files) in pages.items():
    parts=[]
    for filename in files:
        if filename.startswith('@'):
            doc=BeautifulSoup((source/'boundary-design/activity.html').read_text('utf-8'),'html.parser')
            css='\n'.join(s.get_text() for s in doc.select('style'))
        else: css=(source/filename).read_text('utf-8')
        # Decorative references use the same public game assets, never the
        # localhost preview server. Keep shipped headers local.
        css=re.sub(r'url\([\'"]?(?:\.\./)*page-hero-candidates/assets/(bg\d+\.png)[\'"]?\)',r'url("../../../assets/headers/\1")',css)
        css=re.sub(r'url\([\'"]?(?:\.\./)*(?:assets/)?(band_\d+\.svg)[\'"]?\)',r'url("https://bestdori.com/res/icon/\1")',css)
        parts.append('/* Accepted source: '+filename+' */\n'+rules(css,scope))
    content='\n'.join(parts)+'\n'
    if page=='cards':
        content=content.replace('#card-library-unified :where(.shared-card-library)', '#card-library-unified').replace('#card-library-unified',':is(#card-library-unified,.accepted-card-picker)')
        # Imported catalog fragments keep their design IDs as data attributes
        # so inventory and picker instances can be mounted together.
        for name in ['filters','results','search','match-count','match-detail','group','sort','group-jumps','groups','release-help','filter-description','character-filters','character-count']:
            content=re.sub(r'#'+re.escape(name)+r'(?![\w-])', ':is(#'+name+',[data-design-id="'+name+'"])', content)
    if page=='archive':content=content.replace('#archive-design', ':is(#archive-design,#archive-dialog-host)')
    (destination/f'{page}.css').write_text(content,encoding='utf-8')
    print(page, 'styles imported')
