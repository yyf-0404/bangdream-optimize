// Small saved range sets only; full solver results stay in the existing history.
import {RANGE_COORDINATES} from '../domain/timing-plan.js';
let database;
function open() {
  database ??= new Promise((resolve, reject) => {
    const request = indexedDB.open('bangdream-optimize-hit-plans-v1', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('plans', {keyPath: 'key'});
    request.onsuccess = () => {
      request.result.onversionchange = () => { request.result.close(); database = undefined; };
      resolve(request.result);
    };
    request.onerror = () => { database = undefined; reject(request.error); };
    request.onblocked = () => { database = undefined; reject(new Error('方案存储暂时被其他窗口占用')); };
  });
  return database;
}
export async function timingPlanStorageKey(result, profileId = '') {
  if (!globalThis.crypto?.subtle) throw new Error('当前环境无法保存方案标识，可先导出 JSON 留存');
  const identity = JSON.stringify([profileId, result.scoringRevision, result.eventId, result.items,
    result.songs.map(s => [s.songId, s.difficulty, s.teamCardIds]), result.hitTiming]);
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(identity));
  return Array.from(new Uint8Array(digest), n => n.toString(16).padStart(2, '0')).join('');
}
export async function loadTimingPlanRanges(key) {
  const db = await open();
  return new Promise((resolve, reject) => {
    const request = db.transaction('plans', 'readonly').objectStore('plans').get(key);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
export async function saveTimingPlanRanges(key, ranges) {
  const db = await open();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction('plans', 'readwrite');
    const store = transaction.objectStore('plans');
    store.put({key, ranges, coordinates:RANGE_COORDINATES, updatedAt: Date.now()});
    const list = store.getAll();
    list.onsuccess = () => {
      const oldest = list.result.filter(p => p.key !== key).sort((a, b) => b.updatedAt - a.updatedAt).slice(19);
      for (const entry of oldest) store.delete(entry.key);
    };
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error || new Error('保存未完成'));
  });
}
