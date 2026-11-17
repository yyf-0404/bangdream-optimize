// UI preferences deliberately live outside normalizedPlayer and exported game data.
const KEY = 'bangdream-optimize:ui:v1';
const languages = ['zh-CN', 'zh-TW', 'ja', 'en', 'ko'];
const indexes = { 'zh-CN': 3, 'zh-TW': 2, ja: 0, en: 1, ko: 4 };
let memory, reportedSaveFailure=false;
export function readPreferences() {
  if (!memory) {
    try { memory = JSON.parse(globalThis.localStorage?.getItem(KEY) || '{}'); } catch { memory = {}; }
    if (!memory || typeof memory !== 'object' || Array.isArray(memory)) memory = {};
  }
  return memory;
}
export function savePreferences(patch) {
  memory = { ...readPreferences(), ...patch, version: 1 };
  try { globalThis.localStorage?.setItem(KEY, JSON.stringify(memory)); reportedSaveFailure=false; return true; } catch {
    if(!reportedSaveFailure&&globalThis.document){document.dispatchEvent(new CustomEvent('ui-preference-save-error'));reportedSaveFailure=true;}
    return false;
  }
}
export function language() { const v = readPreferences().language; return languages.includes(v) ? v : 'zh-CN'; }
export function languageOrder(selected = language()) { return [...new Set([selected, ...languages])].filter(v => languages.includes(v)); }
export function gameText(value, fallback = '', selected = language()) {
  const present = v => v != null && String(v).trim() !== '';
  if (!Array.isArray(value)) return present(value) ? String(value) : fallback;
  for (const key of languageOrder(selected)) if (present(value[indexes[key]])) return String(value[indexes[key]]);
  return fallback;
}
export function profilePreference(id, key, fallback) { return readPreferences().profiles?.[String(id)]?.[key] ?? fallback; }
export function saveProfilePreference(id, key, value) {
  const profiles = readPreferences().profiles || {};
  return savePreferences({ profiles: { ...profiles, [String(id)]: { ...profiles[String(id)], [key]: value } } });
}
