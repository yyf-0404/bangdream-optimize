export function createCoreLoader({
  state,
  elements,
  normalizePlayer,
  normalizeReferenceData,
  cacheEventPresetFromCore,
  cacheLoadedEventPreset,
  loadEventRecord,
  readPlayer,
  writePlayer,
  renderReferenceOptions,
  warmupCardSearchIndex,
  renderConfigForms,
  appendLog,
}) {
  const CUSTOM_EVENT_ID = 0;

  function isCustomEventId(eventId) {
    return Number(eventId) === CUSTOM_EVENT_ID;
  }

  async function ensureCore({ refreshManifest = false } = {}) {
    if (state.core && !refreshManifest) {
      return state.core;
    }
    if (state.coreLoadPromise && !refreshManifest) {
      return state.coreLoadPromise;
    }

    const loadPromise = state.runtime.syncReferenceData({ refreshManifest })
      .then(normalizeReferenceData)
      .then((core) => {
        state.core = core;
        renderReferenceOptions();
        warmupCardSearchIndex?.();
        if (hasLoadedPlayerConfig()) {
          cacheCurrentEventPreset({ render: true, preferDetail: true });
          renderConfigForms(readPlayer());
        }
        return core;
      })
      .finally(() => {
        if (state.coreLoadPromise === loadPromise) {
          state.coreLoadPromise = null;
        }
      });

    if (!refreshManifest) {
      state.coreLoadPromise = loadPromise;
    }
    return loadPromise;
  }

  function preloadReferenceData() {
    if (state.core || !state.runtime) {
      return;
    }
    ensureCore().catch((error) => {
      appendLog(`reference-data-load-error: ${error.message ?? String(error)}`);
    });
  }

  function cacheCurrentEventPreset({ render = false, preferDetail = false } = {}) {
    let player;
    try {
      player = normalizePlayer(readPlayer());
    } catch {
      return;
    }

    const eventId = player.currentEvent;
    if (eventId == null) {
      return;
    }
    if (isCustomEventId(eventId)) {
      return;
    }

    const changed = cacheEventPresetFromCore(player, eventId);
    if (changed) {
      writePlayer(player);
      if (render) {
        renderConfigForms(player);
      }
    }

    if (!preferDetail || !state.runtime?.syncEventData) {
      return;
    }

    loadEventRecord(eventId, state.core)
      .then((event) => {
        const latest = normalizePlayer(readPlayer());
        if (latest.currentEvent !== eventId) {
          return;
        }
        if (!cacheLoadedEventPreset(latest, eventId, event, { overwrite: true })) {
          return;
        }
        writePlayer(latest);
        if (render) {
          renderConfigForms(latest);
        }
      })
      .catch((error) => {
        appendLog(`event-preset-cache-error: ${error.message ?? String(error)}`);
      });
  }

  function hasLoadedPlayerConfig() {
    return elements.playerJson.value.trim() !== '';
  }

  return {
    cacheCurrentEventPreset,
    ensureCore,
    preloadReferenceData,
  };
}
