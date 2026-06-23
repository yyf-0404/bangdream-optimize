import { createRuntime } from './runtime/index.js?v=2';
import { characterIconUrls } from './assets/index.js?v=2';
import {
  areaItemGroupIconUrls,
  createAreaItemHelpers,
  formatAreaItemRate,
} from './domain/area.js?v=2';
import { attributeSwatch } from './ui/attribute.js?v=2';
import { createActivityActions } from './actions/activity.js?v=2';
import { createAppLifecycle } from './app/lifecycle.js?v=2';
import { createInitialState } from './app/state.js?v=2';
import {
  createBestdoriProfileImporter,
  parseBestdoriProfileExport,
} from './data/bestdori.js?v=2';
import { createCalculationActions } from './actions/calculation.js?v=2';
import {
  characterBonusWithRates as buildCharacterBonusWithRates,
  createCharacterBonusHelpers,
} from './domain/character.js?v=2';
import { createConfigActions } from './actions/config.js?v=2';
import { createCoreLoader } from './app/core.js?v=2';
import {
  createDiagnostics,
  diagnosticFileName,
} from './data/diagnostics.js?v=2';
import { createDownloadActions } from './actions/download.js?v=2';
import { queryElements } from './app/elements.js?v=2';
import {
  createEventModel,
  CUSTOM_EVENT_ID,
} from './models/event.js?v=2';
import { createEventActions } from './actions/event.js?v=2';
import { createEventContext } from './app/event.js?v=2';
import { createFormActions } from './actions/form.js?v=2';
import {
  numericStringSort,
  optionText,
  parseEntityId,
  parseNonNegativeInteger,
  readOptionalInteger,
} from './utils.js?v=2';
import { createCardView } from './views/card.js?v=2';
import { createEventView } from './views/event.js?v=2';
import { createFormCells } from './ui/form.js?v=2';
import { createGameMeta } from './domain/meta.js?v=2';
import {
  cloneJson,
  createPlayerModel,
  normalizedServer as normalizeServerValue,
  readFiniteInput,
} from './models/player.js?v=2';
import { createPageController } from './app/page.js?v=2';
import { createPlayerStore } from './app/player.js?v=2';
import { createPlayerView } from './views/player.js?v=2';
import { createProfileActions } from './actions/profile.js?v=2';
import { createProfileView } from './views/profile.js?v=2';
import {
  createReferenceView,
  installRecoveringDatalistInput,
} from './views/reference.js?v=2';
import { createReferenceData } from './data/reference.js?v=2';
import {
  renderMetrics as renderMetricsView,
  renderResultSummary as renderResultSummaryView,
} from './views/result.js?v=2';
import {
  RESULT_CACHE_LIMIT,
  createResultCacheStorage,
} from './data/result-cache.js?v=2';
import { createResourceActions } from './actions/resource.js?v=2';
import { createServerContext } from './app/server.js?v=2';
import { createSongView } from './views/song.js?v=2';
import { createStatusProxy } from './app/status.js?v=2';
import { createStatusView } from './views/status.js?v=2';
import { createViewAdapters } from './views/adapters.js?v=2';
import { createResultCacheView } from './views/result-cache.js?v=2';

// Runtime state and deferred cross-module calls.
const state = createInitialState();
const elements = queryElements(document);
const status = createStatusProxy();
const resultCacheStorage = createResultCacheStorage({ limit: RESULT_CACHE_LIMIT });
const resultCacheView = createResultCacheView({ elements });
try {
  state.resultCache = await resultCacheStorage.loadResultCache();
} catch {
  state.resultCache = [];
}
resultCacheView.renderResultCache(state.resultCache, {});
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
  getSongRecords: () => state.core?.songs,
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
  normalizedActivityMode,
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
  normalizedActivityMode,
  isSupportedEventType,
  eventMatchesActivityMode,
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
  normalizedActivityMode,
});

const {
  cancelPendingSave,
  ensurePlayerProfiles,
  peekPlayer,
  readPlayer,
  refreshPlayerProfiles,
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
  songCoverUrls,
  songLabel,
  getSongRecord: (songId) => state.core?.songs?.[String(songId)],
  cardLabel,
  cardName,
  cardRarity,
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
  applyEventInputToPlayer,
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
  resultCacheLimit: RESULT_CACHE_LIMIT,
  renderResultCache: deferred.renderResultCache,
  persistResultCache: resultCacheStorage.saveResultCache,
  clearPersistedResultCache: resultCacheStorage.clearResultCache,
});

const resourceActions = createResourceActions({
  state,
  elements,
  cancelPendingSave,
  readPlayer,
  writePlayer,
  refreshPlayerProfiles,
  renderConfigForms: deferred.renderConfigForms,
  renderReferenceOptions,
  renderResultSummary: deferred.renderResultSummary,
  renderMetrics: deferred.renderMetrics,
  renderResultCache: deferred.renderResultCache,
  clearPersistedResultCache: resultCacheStorage.clearResultCache,
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
const cardView = createCardView({
  rows: elements.cardRows,
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
  state,
  elements,
  customEventId: CUSTOM_EVENT_ID,
  normalizedPlayer,
  normalizedActivityMode,
  defaultEditableEvent,
  defaultEventTypeForMode,
  defaultSongListForMode,
  ensureSongListForMode,
  eventMatchesActivityMode,
  editableEventOverride,
  fixedSongListForMode,
  normalizedEventAttributes,
  normalizedEventCharacters,
  normalizedEventMembers,
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
  isSupportedEventType,
  eventLabel,
  eventSongsFromPreset,
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
const pageController = createPageController({
  elements,
  normalizePlayer: normalizedPlayer,
  editableEventSnapshot,
  normalizedActivityMode,
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
  safeReadPlayer,
});

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
  readPlayer,
  writePlayer,
  renderConfigForms: pageController.renderConfigForms,
  configureRuntimeControls: resourceActions.configureRuntimeControls,
  activatePage: pageController.activatePage,
  preloadReferenceData,
  warmupCardSearchIndex,
  handlers: {
    ...activityActions,
    ...calculationActions,
    ...configActions,
    ...formActions,
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

await bootstrap();
