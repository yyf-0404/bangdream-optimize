

// `card.trained` in the catalog means training is supported, not the player's state.
export function cardCapabilities(card){
 const known=card.capabilities;
 return {training:known?.[0]||((card.trained??card.rarity>=3)?[false,true]:[false]),episodes:known?.[1]||[1,1]};
}
export function cardGrowth(card){
 const caps=cardCapabilities(card),raw=card.growth||{};
 const status=value=>caps.training.includes(value)?value:caps.training.includes(true);
 return {trained:status(raw.trained),illustTrained:status(raw.illustTrained),mastery:Math.max(0,Math.min(4,Math.trunc(Number(raw.mastery)||0))),episodes:caps.episodes.map((flag,i)=>flag===0?false:flag===2?true:typeof raw.episodes?.[i]==='boolean'?raw.episodes[i]:true)};
}
export function cardGrowthText(card){
 const g=cardGrowth(card),caps=cardCapabilities(card);
 const training=caps.training.length===1?(g.trained?'固定特训':'不可特训'):(g.trained?'已特训':'未特训');
 const episodes=caps.episodes.map((flag,i)=>`剧情 ${i+1}：${flag===0?'无此剧情':flag===2?'默认已读':g.episodes[i]?'已读':'未读'}`);
 return [`突破 ${g.mastery}/4`,training,`图片：${g.illustTrained?'特训后':'特训前'}`,...episodes].join(' · ');
}
export function growthSummaryMarkup(card){
 const g=cardGrowth(card),caps=cardCapabilities(card);
 const train=caps.training.length===1?(g.trained?'训✓':'训—'):(g.trained?'训✓':'训○');
 return `<span class="card-growth-summary" aria-hidden="true"><span>突${g.mastery}</span><span>${train}</span><span class="card-episode-marks">剧${caps.episodes.map((flag,i)=>`<i class="episode-mark ${!flag?'unavailable':g.episodes[i]?'read':'unread'}"></i>`).join('')}</span></span>`;
}
