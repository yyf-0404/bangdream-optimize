export function createInitialState() {
  return {
    runtime: null,
    core: null,
    coreLoadPromise: null,
    resultCache: [],
    activeResultCacheKey: null,
    lastDiagnostic: null,
    playerSaveTimer: null,
    playerSaveSequence: 0,
    playerSaveQueue: Promise.resolve(),
    playerProfiles: [],
    activePlayerProfileId: undefined,
    expandedCardGroups: new Set(),
    expandedAreaItemGroups: new Set(),
    cardGroupCache: new Map(),
    characterBonusesCollapsed: true,
  };
}
