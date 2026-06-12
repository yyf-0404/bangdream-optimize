export function createPlayerStore({
  state,
  playerJson,
  normalizePlayer,
  cacheEventPresetFromCore,
  activityModeForEvent,
  ensureSongListForMode,
  renderPlayerProfileControls,
  onError,
}) {
  function readPlayer() {
    return normalizePlayer(JSON.parse(playerJson.value));
  }

  function writePlayer(player, { autosave = true } = {}) {
    const normalized = normalizedWritablePlayer(player);
    playerJson.value = JSON.stringify(normalized, null, 2);
    if (autosave) {
      schedulePlayerSave();
    }
  }

  function schedulePlayerSave() {
    if (!state.runtime) {
      return;
    }
    state.playerSaveSequence += 1;
    const sequence = state.playerSaveSequence;
    clearTimeout(state.playerSaveTimer);
    state.playerSaveTimer = setTimeout(() => {
      if (sequence !== state.playerSaveSequence) {
        return;
      }
      savePlayerNow().catch(onError);
    }, 250);
  }

  function savePlayerNow(player = readPlayer()) {
    clearTimeout(state.playerSaveTimer);
    state.playerSaveSequence += 1;
    const normalized = normalizedWritablePlayer(player);
    state.playerSaveQueue = state.playerSaveQueue
      .catch(() => {})
      .then(() => state.runtime.savePlayerConfig(normalized));
    return state.playerSaveQueue;
  }

  function cancelPendingSave() {
    clearTimeout(state.playerSaveTimer);
    state.playerSaveSequence += 1;
  }

  async function ensurePlayerProfiles(player) {
    await refreshPlayerProfiles({ defaultPlayer: player });
  }

  async function refreshPlayerProfiles({ defaultPlayer } = {}) {
    let result = await state.runtime.listPlayerConfigs();
    let profiles = Array.isArray(result?.profiles) ? result.profiles : [];
    let activeId = result?.activeId;

    if (profiles.length === 0) {
      await state.runtime.createPlayerConfig({
        name: '默认配置',
        player: normalizePlayer(defaultPlayer ?? safeReadPlayer()),
      });
      result = await state.runtime.listPlayerConfigs();
      profiles = Array.isArray(result?.profiles) ? result.profiles : [];
      activeId = result?.activeId;
    }

    if (profiles.length > 0 && !profiles.some((profile) => profile.id === activeId)) {
      const selected = await state.runtime.selectPlayerConfig(profiles[0].id);
      writePlayer(selected, { autosave: false });
      result = await state.runtime.listPlayerConfigs();
      profiles = Array.isArray(result?.profiles) ? result.profiles : [];
      activeId = result?.activeId;
    }

    state.playerProfiles = profiles;
    state.activePlayerProfileId = activeId;
    renderPlayerProfileControls(readPlayer());
  }

  function safeReadPlayer() {
    try {
      return readPlayer();
    } catch {
      return state.runtime?.samplePlayerConfig?.() ?? {};
    }
  }

  function normalizedWritablePlayer(player) {
    const normalized = normalizePlayer(player);
    cacheEventPresetFromCore(normalized, normalized.currentEvent);
    syncActivityModeFromCurrentEvent(normalized);
    syncSongListFromCurrentEvent(normalized);
    return normalized;
  }

  function currentEventForPlayer(player) {
    const eventId = player.currentEvent;
    if (eventId == null) {
      return undefined;
    }
    const key = String(eventId);
    return player.eventOverrides?.[key] ?? player.eventPresets?.[key];
  }

  function syncActivityModeFromCurrentEvent(player) {
    const event = currentEventForPlayer(player);
    if (event?.eventType) {
      player.activityMode = activityModeForEvent(event);
    }
  }

  function syncSongListFromCurrentEvent(player) {
    if (player.currentEvent == null) {
      return;
    }
    const event = currentEventForPlayer(player);
    if (event) {
      ensureSongListForMode(player, player.currentEvent, event);
    }
  }

  return {
    cancelPendingSave,
    ensurePlayerProfiles,
    readPlayer,
    refreshPlayerProfiles,
    safeReadPlayer,
    savePlayerNow,
    writePlayer,
  };
}
