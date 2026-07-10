export function createEventContext({
  state,
  elements,
  customEventId,
  readOptionalInteger,
  optionText,
  eventLabel,
  normalizedCalculationMode,
  activityModeForEvent,
  isSupportedEventType,
  isHiddenEventId,
  ensureSongListForMode,
  buildEditableEventSnapshot,
}) {
  function normalizeCurrentActivityForMode(player) {
    const eventId = selectedEventId(player);
    assertSupportedKnownEvent(eventId, player.calculationMode);
    const event = editableEventSnapshot(eventId, player);
    if (event) {
      assertSupportedEvent(event, player.calculationMode);
      player.activityMode = activityModeForEvent(event);
    }
    ensureSongListForMode(player, eventId, event);
  }

  function supportedEventRecords() {
    if (!state.core?.events) {
      return undefined;
    }
    const calculationMode = selectedCalculationMode();
    const records = {};
    for (const [eventId, event] of Object.entries(state.core.events)) {
      if (
        !isHiddenEventId(eventId)
        && isSupportedEventType(event?.eventType, calculationMode)
      ) {
        records[eventId] = event;
      }
    }
    return records;
  }

  function assertSupportedEvent(event, calculationMode = selectedCalculationMode()) {
    if (!isSupportedEventType(event?.eventType, calculationMode)) {
      throw new Error(`不支持的活动类型：${event?.eventType ?? '未知'}`);
    }
  }

  function assertSupportedKnownEvent(eventId, calculationMode = selectedCalculationMode()) {
    if (Number(eventId) === customEventId) {
      return;
    }
    if (isHiddenEventId(eventId)) {
      throw new Error(`活动 ${eventId} 不可用`);
    }
    const event = state.core?.events?.[String(eventId)];
    if (event) {
      assertSupportedEvent(event, calculationMode);
    }
  }

  function eventSearchValue(eventId, player) {
    if (Number(eventId) === customEventId) {
      return optionText(customEventId, '自定义活动');
    }
    const event = player?.eventPresets?.[String(eventId)]
      ?? state.core?.events?.[String(eventId)];
    return event ? optionText(eventId, eventLabel(eventId, player)) : String(eventId);
  }

  function selectedEventId(player) {
    const fromInput = readOptionalInteger(elements.eventId.value);
    const eventId = fromInput ?? player.currentEvent;
    if (eventId == null) {
      throw new Error('未设置活动 ID');
    }
    return eventId;
  }

  function applyEventInputToPlayer(player) {
    const eventId = readOptionalInteger(elements.eventId.value);
    if (eventId !== undefined) {
      player.currentEvent = eventId;
      player.eventSongs[String(eventId)] ??= [];
    }
  }

  function selectedCalculationMode() {
    return normalizedCalculationMode(
      elements.calculationMode.querySelector('input[name="calculation-mode"]:checked')?.value,
    );
  }

  function editableEventSnapshot(eventId, player) {
    return buildEditableEventSnapshot(eventId, player, state.core?.events);
  }

  return {
    applyEventInputToPlayer,
    assertSupportedEvent,
    assertSupportedKnownEvent,
    editableEventSnapshot,
    eventSearchValue,
    normalizeCurrentActivityForMode,
    selectedEventId,
    supportedEventRecords,
  };
}
