import { createRuntime } from './runtime/index.js';
import { characterIconUrls } from './shared/assets.js';
import {
  areaItemGroupIconUrls,
  createAreaItemHelpers,
  formatAreaItemRate,
} from './domain/area.js';
import { attributeSwatch } from './shared/ui/attribute.js';
import { createActivityActions } from './pages/activity/activity-actions.js';
import { createAppLifecycle } from './app/lifecycle.js';
import { createInitialState } from './app/state.js';
import {
  createBestdoriProfileImporter,
  mainBandCardIds,
  parseBestdoriProfileExport,
} from './data/bestdori.js';
import { createCalculationActions } from './app/calculation-actions.js';
import {
  characterBonusWithRates as buildCharacterBonusWithRates,
  createCharacterBonusHelpers,
} from './domain/character.js';
import { createConfigActions } from './app/config-actions.js';
import { createCoreLoader } from './app/core.js';
import {
  createDiagnostics,
  diagnosticFileName,
} from './data/diagnostics.js';
import { createDownloadActions } from './pages/shell/download-actions.js';
import { queryElements } from './app/elements.js';
import {
  createEventModel,
  CUSTOM_EVENT_ID,
  isHiddenEventId,
  normalizedCalculationMode,
  recentUnfinishedEvent,
} from './domain/event.js';
import { createEventActions } from './pages/activity/event-actions.js';
import { createEventContext } from './app/event-context.js';
import { createFormActions } from './pages/cards/add-card-actions.js';
import { createFeedbackActions } from './pages/shell/feedback-actions.js';
import {
  numericStringSort,
  optionText,
  parseEntityId,
  parseNonNegativeInteger,
  readOptionalInteger,
} from './shared/utils.js';
import { createCardView } from './pages/cards/card-library.js';
import { createEventView } from './pages/activity/event-view.js';
import { createFormCells } from './shared/ui/form-cells.js';
import { createGameMeta } from './domain/meta.js';
import {
  cloneJson,
  createPlayerModel,
  normalizedServer as normalizeServerValue,
  readFiniteInput,
} from './domain/player.js';
import { createPageController } from './app/page-controller.js';
import { createPlayerStore } from './app/player-store.js';
import { createPlayerView } from './pages/player/player-library.js';
import { createProfileActions } from './pages/archive/profile-actions.js';
import { createProfileView } from './pages/archive/profile-view.js';
import {
  createReferenceView,
  installRecoveringDatalistInput,
} from './app/reference-view.js';
import { createReferenceData } from './data/reference.js';
import {
  renderMetrics as renderMetricsView,
  renderResultSummary as renderResultSummaryView,
} from './pages/result/result-view.js';
import {
  RESULT_CACHE_LIMIT,
  createResultCacheStorage,
} from './data/result-cache.js';
import { createResourceActions } from './pages/archive/resource-actions.js';
import { createServerContext } from './app/server-context.js';
import { createSongView } from './pages/activity/song-view.js';
import { createStatusProxy } from './app/status-proxy.js';
import { createStatusView } from './pages/shell/status-view.js';
import { createViewAdapters } from './app/view-adapters.js';
import { createArchiveUI } from './pages/archive/archive.js';
import { createResultCacheView } from './pages/result/result-cache-view.js';

import { mountShell } from './pages/shell/shell.js';
import { configureCardPresentation } from './components/cards/preview.js';
import { createCardBrief } from './components/cards/brief.js';
import { cardModel } from './components/cards/model.js';
import { createCardDetails } from './components/cards/presentation.js';
import { createTeamEditor } from './pages/activity/team-editor.js';
import { createHeroes } from './pages/shell/hero.js';
import { createActivityUI } from './pages/activity/activity.js';
import { createEquipmentUI } from './pages/activity/equipment.js';
import {installSelects} from './shared/ui/select.js';

// Runtime state and deferred cross-module calls.
mountShell(document);
installSelects(document);
const state = createInitialState();
const elements = queryElements(document);
void renderConfiguredAppVersion(elements.appVersion);
const status = createStatusProxy();
const resultCacheStorage = createResultCacheStorage({ limit: RESULT_CACHE_LIMIT });
const resultCacheView = createResultCacheView({ elements, eventLabel: (...args) => eventLabel(...args) });
try {
  state.resultCache = await resultCacheStorage.loadResultCache();
} catch {
  state.resultCache = [];
}
resultCacheView.renderResultCache([], {});
const deferred = {
  activatePage: (...args) => pageController.activatePage(...args),
  ensureCore: (...args) => ensureCore(...args),
  renderAreaItems: (...args) => playerView.renderAreaItems(...args),
  renderCards: (...args) => cardView.renderCards(...args),
  renderCharacterBonuses: (...args) => playerView.renderCharacterBonuses(...args),
  renderConfigForms: (...args) => pageController.renderConfigForms(...args),
  renderResultCache: (...args) => resultCacheView.renderResultCache(...args),
  renderMetrics: (...args) => renderMetrics(...args),
  renderResultSummary: (...args) => renderResultSummary(...args),
  renderSongs: (...args) => songView.renderSongs(...args),
};

async function renderConfiguredAppVersion(element) {
  if (!element) {
    return;
  }
  try {
    const response = await fetch(new URL('../package.json', import.meta.url), {
      cache: 'no-cache',
    });
    if (!response.ok) {
      return;
    }
    const metadata = await response.json();
    const version = String(metadata?.version ?? '').trim();
    if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
      return;
    }
    element.textContent = `v${version}`;
    element.hidden = false;
  } catch {
    // Version metadata is informational and must not block application startup.
  }
}

// Server, game metadata, and shared form cells.
const {
  currentServer,
  serverIndex,
} = createServerContext({
  getPlayerServer: () => readPlayer().server,
  getServerInputValue: () => elements.playerServer?.value,
  normalizeServer: normalizeServerValue,
});

const {
  areaItemLabel,
  cardAttribute,
  cardCharacterId,
  cardIconUrls,
  cardLabel,
  cardName,
  cardRarity,
  cardTrainingStatusList,
  characterLabel,
  eventDateRange,
  eventLabel,
  maxAreaItemLevel,
  maxCardLevel,
  cardEpisodeAlwaysRead,
  cardEpisodeAvailable,
  normalizeCardTrainingStatus,
  recordWithFix,
  selectedBandId,
  serverScopedValue,
  songCoverUrls,
  songLabel,
} = createGameMeta({
  getCore: () => state.core,
  serverIndex,
});

const {
  attributeCell,
  entityCell,
  inputCell,
  percentCell,
  songSelectCell,
} = createFormCells({
  attributeFallback: attributeSwatch,
  songCoverUrls,
  songLabel,
  songSearchValue: (songId) => optionText(songId, songLabel(songId)),
  getSongRecords: () => state.core?.songs,
  installRecoveringDatalistInput,
});

// Event and player normalization models.
const {
  activityModeForEvent,
  defaultEditableEvent,
  defaultEventTypeForMode,
  defaultSongListForMode,
  ensureSongListForMode,
  eventMatchesActivityMode,
  eventSongsFromPreset,
  eventWithParameterBonusFix,
  fixedSongListForMode,
  isSupportedEventType,
  normalizedActivityMode,
  supportedEventTypeOrDefault,
} = createEventModel({
  getEventCharacterParameterBonusFix: () => state.core?.eventCharacterParameterBonusFix,
  serverScopedValue,
  cloneJson,
});

const {
  editableEventOverride,
  editableEventSnapshot: buildEditableEventSnapshot,
  normalizedCardConfig,
  normalizedCharacterBonus,
  normalizedEventAttributeAndCharacterBonus,
  normalizedEventAttributes,
  normalizedEventCharacterParameterBonus,
  normalizedEventCharacters,
  normalizedEventMembers,
  normalizedPlayer,
  normalizedServer,
  normalizedStatRate,
} = createPlayerModel({
  hasCardRecord: id => Boolean(state.core?.cards?.[id] ?? state.core?.cardsFix?.[id]),
  normalizedActivityMode,
  normalizedCalculationMode,
  eventWithParameterBonusFix,
  defaultEventTypeForMode,
  supportedEventTypeOrDefault,
  maxCardLevel,
  normalizeCardTrainingStatus,
});

const {
  applyEventInputToPlayer,
  assertSupportedEvent,
  assertSupportedKnownEvent,
  editableEventSnapshot,
  eventSearchValue,
  normalizeCurrentActivityForMode,
  selectedEventId,
  supportedEventRecords,
} = createEventContext({
  state,
  elements,
  customEventId: CUSTOM_EVENT_ID,
  readOptionalInteger,
  optionText,
  eventLabel,
  normalizedCalculationMode,
  activityModeForEvent,
  isSupportedEventType,
  isHiddenEventId,
  ensureSongListForMode,
  buildEditableEventSnapshot,
});

// Domain helpers and reference data.
const {
  allCharacterBonusesAreMaxed,
  bestdoriCharacterBonusFromPoints,
  maxCharacterBonusForPlayer,
  selectedCardCharacterIds,
} = createCharacterBonusHelpers({
  getCharacterRecords: () => state.core?.characters,
  normalizedServer,
  normalizedCharacterBonus,
  cardCharacterId,
});

const {
  allAreaItemsAreMaxed,
  areaItemGroups,
  areaItemIconUrls,
} = createAreaItemHelpers({
  getAreaItemRecords: () => state.core?.areaItems,
  recordWithFix,
  serverScopedValue,
  maxAreaItemLevel,
});

const {
  cacheEventPresetFromCore,
  cacheLoadedEventPreset,
  loadEventRecord,
  normalizeReferenceData,
} = createReferenceData({
  getCore: () => state.core,
  getRuntime: () => state.runtime,
  appendLog: status.appendLog,
  cloneJson,
  eventWithParameterBonusFix,
});

const {
  buildDiagnostic,
} = createDiagnostics({
  getRuntime: () => state.runtime,
  getCore: () => state.core,
  appendLog: status.appendLog,
});

// Profile, reference, player, and result view adapters.
const {
  activeProfileName,
  nextProfileName,
  renderPlayerProfileControls,
} = createProfileView({
  profileSelect: elements.playerProfile,
  profileNameInput: elements.playerProfileName,
  deleteButton: elements.deletePlayerProfile,
  getProfiles: () => state.playerProfiles,
  getActiveId: () => state.activePlayerProfileId,
});

const {
  matchingCardPreviewIds,
  renderReferenceOptions,
  warmupCardSearchIndex,
} = createReferenceView({
  elements,
  currentServer,
  getCore: () => state.core,
  getPlayer: () => peekPlayer(),
  cardLabel,
  cardName,
  cardIconUrls,
  songLabel,
  areaItemLabel,
  characterLabel,
  supportedEventRecords,
  eventLabel,
});

const {
  cancelPendingSave,
  ensurePlayerProfiles,
  peekPlayer,
  readPlayer,
  refreshPlayerProfiles,
  initializePlayerDefaults,
  safeReadPlayer,
  savePlayerNow,
  writePlayer,
} = createPlayerStore({
  state,
  playerJson: elements.playerJson,
  normalizePlayer: normalizedPlayer,
  cacheEventPresetFromCore,
  activityModeForEvent,
  ensureSongListForMode,
  recentUnfinishedEvent,
  customEventId: CUSTOM_EVENT_ID,
  defaultEditableEvent,
  renderPlayerProfileControls,
  onError: status.setError,
});

const {
  cardEntityCell,
  characterEntityCell,
  mergedEntityIds,
  renderMetrics,
  renderResultSummary,
} = createViewAdapters({
  elements,
  numericStringSort,
  renderMetricsView,
  renderResultSummaryView,
  selectedBandId,
  areaItemGroups,
  areaItemLabel,
  formatAreaItemRate,
  songCoverUrls,
  songLabel,
  getSongRecord: (songId) => state.core?.songs?.[String(songId)],
  getCore: () => state.core,
  getRuntime: () => state.runtime,
  getProfileId: () => state.activePlayerProfileId,
  cardLabel,
  cardName,
  cardRarity,
  cardCharacterId,
  normalizedCardConfig,
  readPlayer,
  cardIconUrls,
  cardAttribute,
  attributeFallback: attributeSwatch,
  entityCell,
  characterIconUrls,
  characterLabel,
});

// Profile import/export adapters and core data loader.
const {
  bestdoriProfileToPlayerConfig,
  playerToBestdoriProfileExport,
  importEnabledAreaItems,
  importMainBandCards,
  importMainBandCharacterBonuses,
} = createBestdoriProfileImporter({
  normalizedPlayer,
  normalizedServer,
  normalizedCharacterBonus,
  normalizedStatRate,
  recordWithFix,
  maxCardLevel,
  maxAreaItemLevel,
  cardCharacterId,
  getCharacterRecords: () => state.core?.characters,
  bestdoriCharacterBonusFromPoints,
});

const {
  cacheCurrentEventPreset,
  ensureCore,
  preloadReferenceData,
} = createCoreLoader({
  state,
  elements,
  normalizePlayer: normalizedPlayer,
  normalizeReferenceData,
  cacheEventPresetFromCore,
  cacheLoadedEventPreset,
  loadEventRecord,
  readPlayer,
  writePlayer,
  renderReferenceOptions,
  warmupCardSearchIndex,
  renderConfigForms: deferred.renderConfigForms,
  appendLog: status.appendLog,
});

// User action handlers.
const configActions = createConfigActions({
  readPlayer,
  writePlayer,
  normalizedCardConfig,
  normalizedCharacterBonus,
  maxAreaItemLevel,
  maxCharacterBonusForPlayer,
  allAreaItemsAreMaxed,
  allCharacterBonusesAreMaxed,
  buildCharacterBonusWithRates,
  cardCharacterId,
  getAreaItemRecords: () => state.core?.areaItems,
  getCharacterRecords: () => state.core?.characters,
  renderCards: deferred.renderCards,
  renderAreaItems: deferred.renderAreaItems,
  renderCharacterBonuses: deferred.renderCharacterBonuses,
});

const profileActions = createProfileActions({
  initializePlayerDefaults,
  state,
  elements,
  normalizedPlayer,
  normalizedServer,
  parseEntityId,
  parseNonNegativeInteger,
  parseBestdoriProfileExport,
  ensureCore: deferred.ensureCore,
  readPlayer,
  writePlayer,
  savePlayerNow,
  refreshPlayerProfiles,
  renderPlayerProfileControls,
  renderConfigForms: deferred.renderConfigForms,
  nextProfileName,
  activeProfileName,
  bestdoriProfileToPlayerConfig,
  playerToBestdoriProfileExport,
  importMainBandCards,
  importMainBandCharacterBonuses,
  importEnabledAreaItems,
  activatePage: deferred.activatePage,
  setStatus: status.setStatus,
  setError: status.setError,
});

const calculationActions = createCalculationActions({
  state,
  elements,
  readPlayer,
  writePlayer,
  savePlayerNow,
  readOptionalInteger,
  parseEntityId,
  applyEventInputToPlayer,
  editableEventSnapshot,
  normalizeCurrentActivityForMode,
  ensureOwnedCardCharacterBonuses: configActions.ensureOwnedCardCharacterBonuses,
  ensureCore: deferred.ensureCore,
  renderConfigForms: deferred.renderConfigForms,
  renderResultSummary: deferred.renderResultSummary,
  renderMetrics: deferred.renderMetrics,
  buildDiagnostic,
  diagnosticFileName,
  activatePage: deferred.activatePage,
  setStatus: status.setStatus,
  setError: status.setError,
  eventLabel,
  normalizedPlayer,
  areaItemGroups,
  mainBandCardIds,
  importMainBandCards,
  importMainBandCharacterBonuses,
  importEnabledAreaItems,
  cardCharacterId,
  resultCacheLimit: RESULT_CACHE_LIMIT,
  renderResultCache: deferred.renderResultCache,
  persistResultCache: resultCacheStorage.saveResultCache,
  clearPersistedResultCache: resultCacheStorage.clearResultCache,
});

const feedbackActions = createFeedbackActions({
  state,
  elements,
  diagnosticFileName,
});

const resourceActions = createResourceActions({
  state,
  elements,
  cancelPendingSave,
  readPlayer,
  writePlayer,
  refreshPlayerProfiles,
  initializePlayerDefaults,
  renderConfigForms: deferred.renderConfigForms,
  renderReferenceOptions,
  renderResultSummary: deferred.renderResultSummary,
  renderMetrics: deferred.renderMetrics,
  renderResultCache: deferred.renderResultCache,
  clearPersistedResultCache: resultCacheStorage.clearResultCache,
  invalidateResultCache: calculationActions.invalidateResultCache,
  hasActiveCalculation: calculationActions.hasActiveCalculation,
  ensureCore: deferred.ensureCore,
  setStatus: status.setStatus,
  setError: status.setError,
});

const downloadActions = createDownloadActions({
  state,
  elements,
  setStatus: status.setStatus,
  setError: status.setError,
});

// Page views.
const loadCardDetail = id => state.runtime.syncCardDetail(id);
const readOnlyCardDetails = createCardDetails({loadCardDetail});
configureCardPresentation(({id,config,player,captain,order,teamCardIds}) => {
  const currentPlayer = player ?? safeReadPlayer();
  const card = cardModel(state.core, currentPlayer, id, config, state.activePlayerProfileId);
  const teamCards = Array.isArray(teamCardIds) ? teamCardIds.map(teamId => teamId
    ? cardModel(state.core, currentPlayer, teamId, undefined, state.activePlayerProfileId) : null) : undefined;
  return createCardBrief(card, {captain,order,teamCards,onOpen:c=>readOnlyCardDetails.open(c,{context:config?'本次计算':'当前档案'})});
});
const cardView = createCardView({
  loadCardDetail,
  rows: elements.cardRows,
  getProfileId: () => state.activePlayerProfileId,
  writePlayer,
  expandedGroups: state.expandedCardGroups,
  groupCache: state.cardGroupCache,
  getCore: () => state.core,
  getPlayer: peekPlayer,
  entityCell,
  inputCell,
  cardLabel,
  cardName,
  cardRarity,
  cardIconUrls,
  normalizedCardConfig,
  cardTrainingStatusList,
  maxCardLevel,
  cardEpisodeAlwaysRead,
  cardEpisodeAvailable,
  cardCharacterId,
  cardAttribute,
  characterLabel,
  updateCard: configActions.updateCard,
  updateCardEpisode: configActions.updateCardEpisode,
  deleteCard: configActions.deleteCard,
  clearCards: configActions.clearCards,
});

const eventActions = createEventActions({
  readPlayer,
  normalizePlayer: normalizedPlayer,
  selectedEventId,
  editableEventSnapshot,
  defaultEventTypeForMode,
  assertSupportedEvent,
  eventMatchesActivityMode,
  editableEventOverride,
  ensureSongListForMode,
  writePlayer,
  renderConfigForms: deferred.renderConfigForms,
  normalizedEventAttributes,
  normalizedEventCharacters,
  normalizedEventMembers,
  fixedSongListForMode,
  renderSongs: deferred.renderSongs,
});

const activityActions = createActivityActions({
  getProfileId: () => state.activePlayerProfileId,
  elements,
  customEventId: CUSTOM_EVENT_ID,
  normalizedPlayer,
  normalizedCalculationMode,
  activityModeForEvent,
  defaultEditableEvent,
  defaultEventTypeForMode,
  defaultSongListForMode,
  ensureSongListForMode,
  eventMatchesActivityMode,
  isHiddenEventId,
  editableEventOverride,
  fixedSongListForMode,
  normalizedEventAttributes,
  normalizedEventCharacters,
  normalizedEventMembers,
  serverIndex,
  readFiniteInput,
  readOptionalInteger,
  ensureCore: deferred.ensureCore,
  loadEventRecord,
  cacheLoadedEventPreset,
  assertSupportedEvent,
  assertSupportedKnownEvent,
  editableEventSnapshot,
  eventSearchValue,
  readPlayer,
  writePlayer,
  updateCurrentEvent: eventActions.updateCurrentEvent,
  renderReferenceOptions,
  renderConfigForms: deferred.renderConfigForms,
  setStatus: status.setStatus,
  setError: status.setError,
});

const playerView = createPlayerView({
  elements,
  recordWithFix,
  serverScopedValue,
  writePlayer,
  getProfileId: () => state.activePlayerProfileId,
  getCore: () => state.core,
  maxCharacterBonusForPlayer,
  expandedAreaItemGroups: state.expandedAreaItemGroups,
  isCharacterBonusesCollapsed: () => state.characterBonusesCollapsed,
  setCharacterBonusesCollapsed: (collapsed) => {
    state.characterBonusesCollapsed = collapsed;
  },
  readPlayer,
  areaItemGroups,
  areaItemGroupIconUrls,
  areaItemIconUrls,
  areaItemLabel,
  maxAreaItemLevel,
  formatAreaItemRate,
  hasAreaItemResources: () => Object.keys(state.core?.areaItems ?? {}).length > 0,
  hasCharacterResources: () => Object.keys(state.core?.characters ?? {}).length > 0,
  allAreaItemsAreMaxed,
  allCharacterBonusesAreMaxed,
  characterIdsForPlayer: (player) => mergedEntityIds(state.core?.characters, player.characterBouns),
  selectedCardCharacterIds,
  normalizedCharacterBonus,
  entityCell,
  characterEntityCell,
  inputCell,
  updateAreaItem: configActions.updateAreaItem,
  updateCharacterBonus: configActions.updateCharacterBonus,
});

const eventView = createEventView({
  elements,
  customEventId: CUSTOM_EVENT_ID,
  readPlayer,
  editableEventSnapshot,
  isSupportedEventType: (eventType) => isSupportedEventType(
    eventType,
    readPlayer().calculationMode,
  ),
  eventLabel,
  eventDateRange,
  normalizedEventAttributeAndCharacterBonus,
  normalizedEventCharacterParameterBonus,
  normalizedEventAttributes,
  normalizedEventCharacters,
  normalizedEventMembers,
  attributeCell,
  percentCell,
  characterEntityCell,
  cardEntityCell,
  cardAttribute,
  attributeFallback: attributeSwatch,
  updateEventAttribute: eventActions.updateEventAttribute,
  deleteEventAttribute: eventActions.deleteEventAttribute,
  updateEventCharacter: eventActions.updateEventCharacter,
  deleteEventCharacter: eventActions.deleteEventCharacter,
  updateEventMember: eventActions.updateEventMember,
  deleteEventMember: eventActions.deleteEventMember,
});

const songView = createSongView({
  getCore:()=>state.core,
  getProfileId:()=>state.activePlayerProfileId,
  readPlayer,writePlayer,
  songLabel,songCoverUrls,
  rows: elements.songRows,
  selectedEventId,
  editableEventSnapshot,
  getSongRecord: (songId) => state.core?.songs?.[String(songId)],
  normalizedActivityMode,
  fixedSongListForMode,
  eventSongsFromPreset,
  songSelectCell,
  updateSong: eventActions.updateSong,
});

// Page controller, form actions, status view, and lifecycle.
const teamEditor = createTeamEditor({getCore:()=>state.core,getPlayer:safeReadPlayer,getProfileId:()=>state.activePlayerProfileId,writePlayer,onApply:deferred.renderConfigForms,importDraft:calculationActions.loadMainBandDraft,openDetails:c=>readOnlyCardDetails.open(c,{context:'当前档案'})});
const heroes = createHeroes({readAsset:path=>state.runtime.loadHeaderAsset(path),getCore:()=>state.core,getPlayer:safeReadPlayer,getProfileId:()=>state.activePlayerProfileId});
const activityUI = createActivityUI({elements,getPlayer:readPlayer,writePlayer,renderForms:deferred.renderConfigForms,eventSnapshot:editableEventSnapshot,activityModeForEvent,getCore:()=>state.core,getProfileId:()=>state.activePlayerProfileId});
const equipmentUI = createEquipmentUI({elements,getPlayer:readPlayer,getProfileId:()=>state.activePlayerProfileId,writePlayer,renderForms:deferred.renderConfigForms,areaItemGroups,areaItemLabel});
const pageController = createPageController({
  elements,
  onRender: player => {activityUI.render(player);equipmentUI.render(player);archiveUI.render();void heroes.update();calculationActions.syncProfileResult();},
  renderTeams: teamEditor.renderTeams,
  normalizePlayer: normalizedPlayer,
  editableEventSnapshot,
  normalizedActivityMode,
  normalizedCalculationMode,
  activityModeForEvent,
  eventSearchValue,
  renderReferenceOptions,
  renderPlayerProfileControls,
  renderEventSummary: eventView.renderEventSummary,
  renderEventParameters: eventView.renderEventParameters,
  renderSongs: songView.renderSongs,
  renderCards: cardView.renderCards,
  renderAreaItems: playerView.renderAreaItems,
  renderCharacterBonuses: playerView.renderCharacterBonuses,
  areaItemGroups,
  areaItemGroupIconUrls,
  formatAreaItemRate,
  cardAttribute,
  cardIconUrls,
  cardLabel,
  cardName,
  cardRarity,
  safeReadPlayer,
});

const archiveUI=createArchiveUI({state,elements,readPlayer,writePlayer,refreshProfiles:refreshPlayerProfiles,renderForms:player=>pageController.renderConfigForms(player),setError:status.setError,setStatus:status.setStatus});

const formActions = createFormActions({
  elements,
  ensureCore: deferred.ensureCore,
  parseEntityId,
  normalizedPlayer,
  normalizedCharacterBonus,
  readPlayer,
  writePlayer,
  renderConfigForms: pageController.renderConfigForms,
  maxCardLevel,
  cardCharacterId,
  expandCardGroupForCard: cardView.expandCardGroupForCard,
  matchingCardPreviewIds,
  setStatus: status.setStatus,
  setError: status.setError,
});

const statusView = createStatusView({
  elements,
  renderResultSummary,
  renderMetrics,
});
status.attach(statusView);

const { bootstrap } = createAppLifecycle({
  state,
  elements,
  createRuntime,
  installRecoveringDatalistInput,
  ensureCore,
  ensurePlayerProfiles,
  initializePlayerDefaults,
  readPlayer,
  writePlayer,
  renderConfigForms: pageController.renderConfigForms,
  configureRuntimeControls: resourceActions.configureRuntimeControls,
  activatePage: pageController.activatePage,
  preloadReferenceData,
  warmupCardSearchIndex,
  renderReferenceOptions,
  handlers: {
    ...activityActions,
    ...calculationActions,
    ...configActions,
    ...formActions,
    ...feedbackActions,
    ...profileActions,
    ...downloadActions,
    ...resourceActions,
    handleToggleAreaItems: playerView.handleToggleAreaItems,
    handleToggleCharacterBonuses: playerView.handleToggleCharacterBonuses,
  },
  appendLog: status.appendLog,
  setStatus: status.setStatus,
  setError: status.setError,
});

document.addEventListener('player-config-change', () => {calculationActions.syncProfileResult();void heroes.update();});
document.addEventListener('game-language-change', () => {
  pageController.renderConfigForms(readPlayer());
  calculationActions.refreshResultView();
});
await bootstrap();
pageController.activatePage(location.hash.slice(1) || 'activity');
window.addEventListener('hashchange', () => pageController.activatePage(location.hash.slice(1)));
