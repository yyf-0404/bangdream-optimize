export function createEventContext({
  state,
  elements,
  customEventId,
  readOptionalInteger,
  optionText,
  eventLabel,
  normalizedActivityMode,
  isSupportedEventType,
  eventMatchesActivityMode,
  ensureSongListForMode,
  buildEditableEventSnapshot,
}) {
  function normalizeCurrentActivityForMode(player) {
    const eventId = selectedEventId(player);
    assertSupportedKnownEvent(eventId);
    const event = editableEventSnapshot(eventId, player);
    if (event && !eventMatchesActivityMode(event, player.activityMode)) {
      throw new Error(`当前模式不能选择 ${event.eventType} 活动`);
    }
    ensureSongListForMode(player, eventId, event);
  }

  function supportedEventRecords() {
    if (!state.core?.events) {
      return undefined;
    }
    const mode = normalizedActivityMode(elements.activityMode.value);
    const records = {};
    for (const [eventId, event] of Object.entries(state.core.events)) {
      if (isSupportedEventType(event?.eventType) && eventMatchesActivityMode(event, mode)) {
        records[eventId] = event;
      }
    }
    return records;
  }

  function assertSupportedEvent(event) {
    if (!isSupportedEventType(event?.eventType)) {
      throw new Error(`不支持的活动类型：${event?.eventType ?? '未知'}`);
    }
  }

  function assertSupportedKnownEvent(eventId) {
    if (Number(eventId) === customEventId) {
      return;
    }
    const event = state.core?.events?.[String(eventId)];
    if (event) {
      assertSupportedEvent(event);
      const mode = normalizedActivityMode(elements.activityMode.value);
      if (!eventMatchesActivityMode(event, mode)) {
        throw new Error(`当前模式不能选择 ${event.eventType} 活动`);
      }
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
