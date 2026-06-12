export function createReferenceData({
  getCore,
  getRuntime,
  appendLog,
  cloneJson,
  eventWithParameterBonusFix,
}) {
  function normalizeReferenceData(core) {
    const normalized = {
      ...core,
      cards: mergeReferenceFix(core?.cards, core?.cardsFix),
      skills: mergeReferenceFix(core?.skills, core?.skillsFix),
      areaItems: mergeReferenceFix(core?.areaItems, core?.areaItemsFix),
    };

    normalized.events = applyEventParameterFixes(
      core?.events,
      core?.eventCharacterParameterBonusFix,
    );
    return normalized;
  }

  function cacheEventPresetFromCore(player, eventId) {
    if (eventId == null || player.eventPresets?.[String(eventId)] != null) {
      return false;
    }
    const event = eventWithParameterBonusFix(getCore()?.events?.[String(eventId)], eventId);
    if (!event || typeof event !== 'object' || Object.keys(event).length === 0) {
      return false;
    }
    return cacheLoadedEventPreset(player, eventId, event);
  }

  function cacheLoadedEventPreset(player, eventId, event, { overwrite = false } = {}) {
    const key = String(eventId);
    player.eventPresets ??= {};
    if (!overwrite && player.eventPresets[key] != null) {
      return false;
    }
    const normalized = eventWithParameterBonusFix(event, eventId);
    if (!normalized || typeof normalized !== 'object') {
      return false;
    }
    const next = cloneJson(normalized);
    if (JSON.stringify(player.eventPresets[key]) === JSON.stringify(next)) {
      return false;
    }
    player.eventPresets[key] = next;
    return true;
  }

  async function loadEventRecord(eventId, core) {
    let event = core.events?.[String(eventId)];
    const runtime = getRuntime();
    if (runtime?.syncEventData) {
      try {
        event = await runtime.syncEventData(eventId);
      } catch (error) {
        appendLog(`event-detail-fallback: ${error.message ?? String(error)}`);
      }
    }
    if (!event) {
      throw new Error(`活动 ${eventId} 不存在`);
    }
    return eventWithParameterBonusFix(event, eventId);
  }

  return {
    cacheEventPresetFromCore,
    cacheLoadedEventPreset,
    loadEventRecord,
    normalizeReferenceData,
  };
}

function mergeReferenceFix(records, fix) {
  return {
    ...(records ?? {}),
    ...(fix ?? {}),
  };
}

function applyEventParameterFixes(events, fix) {
  const normalized = { ...(events ?? {}) };
  for (const [eventId, fixedValue] of Object.entries(fix ?? {})) {
    const event = normalized[eventId];
    if (!event || event.eventCharacterParameterBonus != null) {
      continue;
    }
    normalized[eventId] = {
      ...event,
      eventCharacterParameterBonus: cloneJsonLocal(fixedValue),
    };
  }
  return normalized;
}

function cloneJsonLocal(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}
