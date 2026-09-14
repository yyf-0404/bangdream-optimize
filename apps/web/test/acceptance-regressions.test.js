import test from 'node:test';
import assert from 'node:assert/strict';
import {teamChoiceReason,placeTeamCard} from '../src/ui/team-rules.js';
import {compareSongRelease,compareEventSongOrder} from '../src/ui/song-rules.js';
import {createEventModel} from '../src/models/event.js';
import {songCoverUrls} from '../src/assets/index.js';
import {createResourceActions} from '../src/actions/resource.js';
import {allowedLiveVariants} from '../src/models/player-settings.js';

test('fixed calculation modes never offer live variants ignored by their solver',()=>{
 assert.deepEqual(allowedLiveVariants('challenge','scoreRange'),['solo']);
 assert.deepEqual(allowedLiveVariants('festival','scoreRange'),['solo']);
 assert.deepEqual(allowedLiveVariants('versus','scoreRange'),['versus']);
 assert.deepEqual(allowedLiveVariants('medley','scoreRange'),['medley']);
 assert.deepEqual(allowedLiveVariants('challenge','maximize'),['challenge_cp']);
 assert.deepEqual(allowedLiveVariants('challenge','ptEvaluate'),['solo','challenge_cp']);
 assert.deepEqual(allowedLiveVariants('challenge','ptMaximize'),['solo','cooperative','challenge_cp']);
});

test('full team replaces the active slot without blocking the existing character',()=>{
 const teams=[[1,2,3,4,5],[6,7,8,9,10]],characterOf=id=>id%5;
 assert.equal(teamChoiceReason({id:11,characterId:1,owned:true},teams,0,0,characterOf),'');
 assert.match(teamChoiceReason({id:11,characterId:1,owned:true},teams,0,1,characterOf),/同一角色/);
 assert.match(teamChoiceReason({id:6,characterId:1,owned:true},teams,0,0,characterOf),/其他队伍/);
 assert.match(teamChoiceReason({id:11,characterId:1,owned:false},teams,0,0,characterOf),/持有/);
 assert.deepEqual(placeTeamCard(teams[0],0,11),{team:[11,2,3,4,5],slot:0});
 assert.deepEqual(teams[0],[1,2,3,4,5]);
});
test('team draft advances to the next empty slot, wrapping when needed',()=>{
 assert.deepEqual(placeTeamCard([0,2,0,0,5],2,3),{team:[0,2,3,0,5],slot:3});
 assert.deepEqual(placeTeamCard([0,2,3,4,0],4,5),{team:[0,2,3,4,5],slot:0});
});
test('partial songs and unknown IDs/difficulties survive normalization instead of preset replacement',()=>{
 const model=createEventModel({getSongRecords:()=>({1:{},2:{},3:{}}),getEventCharacterParameterBonusFix:()=>({}),serverScopedValue:x=>x,cloneJson:structuredClone});
 const draft=[{songId:0,difficulty:3},{songId:99999,difficulty:4}];
 assert.deepEqual(model.fixedSongListForMode(draft,'medley',{musicIds:[1,2,3]}),[...draft,{songId:0,difficulty:3}]);
 assert.deepEqual(draft,[{songId:0,difficulty:3},{songId:99999,difficulty:4}]);
});
test('manual song sort uses current server publication, keeps future and undated songs, then ID',()=>{
 const songs=[{id:9999,record:{publishedAt:[300,0,0,100]}},{id:1,record:{publishedAt:[100,0,0,200]}},{id:2,record:{publishedAt:[]}},{id:3,record:{publishedAt:[0,0,0,200]}}];
 assert.deepEqual([...songs].sort((a,b)=>compareSongRelease(a,b,'cn')).map(s=>s.id),[3,1,9999,2]);
 assert.deepEqual([...songs].sort((a,b)=>compareSongRelease(a,b,'jp')).map(s=>s.id),[9999,1,3,2]);
});
test('event song candidates and restored slots follow preset order, not ID or publication',()=>{
 const model=createEventModel({getSongRecords:()=>({1:{},42:{},900:{}}),getEventCharacterParameterBonusFix:()=>({}),serverScopedValue:x=>x[3],cloneJson:structuredClone});
 const event={musics:[null,null,null,[{musicId:900,seq:2,difficulty:'special'},{musicId:1,seq:3},{musicId:42,seq:1}]]};
 const preset=model.eventSongsFromPreset(event);
 assert.deepEqual(preset,[{songId:42,difficulty:3},{songId:900,difficulty:4},{songId:1,difficulty:3}]);
 const candidates=[{id:1},{id:900},{id:42}];
 assert.deepEqual(candidates.sort((a,b)=>compareEventSongOrder(a,b,preset.map(s=>s.songId))).map(s=>s.id),[42,900,1]);
 assert.deepEqual(model.fixedSongListForMode(preset,'medley',event),preset);
 assert.deepEqual(model.fixedSongListForMode(preset,'single',event),[preset[0]]);
 assert.deepEqual(model.fixedSongListForMode(preset.slice(0,1),'medley',event),[preset[0],{songId:0,difficulty:3},{songId:0,difficulty:3}]);
 assert.deepEqual(model.eventSongsFromPreset({musicIds:[42,900,1]}).map(s=>s.songId),[42,900,1]);
 assert.deepEqual(event.musics[3].map(s=>s.musicId),[900,1,42]);
});

test('song covers include server-specific jackets with deduplicated bucket fallback',()=>{
 const urls=songCoverUrls({songId:10,song:{jacketImage:['jp-art',null,null,'cn-art']}});
 assert.ok(urls.some(u=>u.includes('/cn/')&&u.endsWith('-cn-art-jacket.png')));
 assert.ok(urls.some(u=>u.includes('/jp/')&&u.endsWith('-jp-art-jacket.png')));
 assert.equal(new Set(urls).size,urls.length);
});

test('clearing game cache reloads core before repainting while keeping the player archive',async()=>{
 const steps=[],player={cardList:{1:{skillLevel:5}}},state={core:{old:true},resultCache:[],runtime:{clearGameCache:async()=>steps.push('clear')}};
 const actions=createResourceActions({state,elements:{log:{}},clearPersistedResultCache:async()=>{},ensureCore:async()=>{steps.push('load');state.core={ready:true};},renderResultCache:()=>{},renderResultSummary:()=>{},renderMetrics:()=>{},renderReferenceOptions:()=>{assert.equal(state.core.ready,true);steps.push('render');},readPlayer:()=>player,renderConfigForms:p=>assert.equal(p,player),setStatus:()=>{},setError:e=>{throw e;}});
 await actions.handleClearGameCache();assert.deepEqual(steps,['clear','load','render']);assert.equal(player.cardList[1].skillLevel,5);
});
