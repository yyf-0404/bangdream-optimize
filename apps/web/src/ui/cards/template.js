import {designFragment} from '../approved/templates.js';
import {hydrateDesignIcons} from '../design-controls.js';
import {designIcon} from '../fidelity.js';

export function mountCatalogTemplate(root){
 const filters=designFragment('card-filters'),results=designFragment('card-results');
 root.replaceChildren(filters,results);
 const q=s=>root.querySelector(s);
 filters.classList.add('catalog-filters');results.classList.add('catalog-results');
 const classes={'#search':'catalog-search-input','.search':'catalog-search','#match-count':'catalog-count match-count','#match-detail':'match-detail','.results-dock':'catalog-sticky','.results-toolbar':'catalog-toolbar','.view-controls':'catalog-view-controls','#group-jumps':'catalog-jumps','#groups':'catalog-groups'};
 for(const [selector,names]of Object.entries(classes))q(selector)?.classList.add(...names.split(' '));
 for(const [key,id]of Object.entries({character:'character-filters',attribute:'attribute-options',rarity:'rarity-options',server:'server-options',ownership:'ownership-options'})){q('#'+id).dataset.host=key;q('#'+key+'-actions').dataset.actions=key;}
 q('#group').dataset.view='group';q('#group').setAttribute('aria-label','卡牌分组');q('#sort').dataset.view='sort';q('#sort').setAttribute('aria-label','组内排序');
 const sort=q('#sort'),sortLabel=sort.closest('label'),sortHeading=document.createElement('div'),sortRow=document.createElement('div');
 sortHeading.className='sort-heading';sortHeading.append(sortLabel.querySelector('.control-label'));
 sortHeading.querySelector('[data-icon]').remove();
 sortHeading.querySelector('.control-label').insertAdjacentHTML('afterbegin','<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" aria-hidden="true"><path d="M4 6h16M4 12h12M4 18h8"/></svg>');
 sortRow.className='catalog-sort-row';sortRow.append(sort);
 const direction=document.createElement('button');direction.type='button';direction.className='catalog-sort-direction';direction.dataset.sortDirection='';
 sortRow.append(direction);sortLabel.replaceWith(sortHeading,sortRow);
 const secondary=document.createElement('p');secondary.textContent='持有状态或稀有度相同时，默认按发布时间排序。';q('.release-popover strong').after(secondary);
 q('#filter-description').dataset.filterDescription='';q('.filter-footer a').dataset.showResults='';q('.back-to-filters').dataset.backFilter='';
 q('.filter-footer a').addEventListener('click',e=>e.preventDefault());q('.back-to-filters').addEventListener('click',e=>e.preventDefault());
 q('#empty').remove();q('#end-note').remove();
 const bulk=document.createElement('button');bulk.type='button';bulk.className='bulk-toggle';bulk.dataset.bulkToggle='';bulk.innerHTML=designIcon('cards')+'批量管理';q('.view-controls').append(bulk);
 const actions=document.createElement('div');actions.className='catalog-bulk bulk-bar';actions.hidden=true;q('.results-dock').append(actions);
 // A shared picker can coexist with the main card library. Keep design IDs
 // scoped through data attributes and classes, without duplicate document IDs.
 for(const node of root.querySelectorAll('[id]')){node.dataset.designId=node.id;node.removeAttribute('id');}
 hydrateDesignIcons(root);
}
