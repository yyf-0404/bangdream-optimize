import { clearFieldValidationMessage, setFieldValidationMessage } from '../ui/validation.js';
import { ATTRIBUTE_VALUES } from '../utils.js?v=2';

const DEFAULT_EVENT_BONUS_PERCENT = 10;
const DEFAULT_EVENT_ATTRIBUTES = ATTRIBUTE_VALUES;

export function createActivityActions({
  state,
  elements,
  customEventId,
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
  ensureCore,
  loadEventRecord,
  cacheLoadedEventPreset,
  assertSupportedEvent,
  assertSupportedKnownEvent,
  editableEventSnapshot,
  eventSearchValue,
  readPlayer,
  writePlayer,
  updateCurrentEvent,
  renderReferenceOptions,
  renderConfigForms,
  setStatus,
  setError,
}) {
  function parseEventIdFromSearch(value) {
    const trimmed = value.trim();
    const match = trimmed.match(/^(\d+)(?:\b|\s|$|[·-])/);
    if (!match) {
      throw new Error(`活动 无效：${value}`);
    }
    const parsed = Number.parseInt(match[1], 10);
    if (!Number.isSafeInteger(parsed) || parsed < 0) {
      throw new Error(`活动 无效：${value}`);
    }
    return parsed;
  }

  function readNumericField(input, label) {
    try {
      clearFieldValidationMessage(input);
      return readFiniteInput(input, label);
    } catch (error) {
      setFieldValidationMessage(input, error);
      return undefined;
    }
  }

  function defaultAttributeBonus(attributes) {
    const usedAttributes = new Set(
      normalizedEventAttributes(attributes).map((bonus) => bonus.attribute),
    );
    return DEFAULT_EVENT_ATTRIBUTES.find((attribute) => !usedAttributes.has(attribute))
      ?? DEFAULT_EVENT_ATTRIBUTES[0];
  }

  function entityOptions(datalist) {
    return Array.from(datalist?.options ?? [])
      .map((option) => Number.parseInt(String(option.value).match(/^\d+/)?.[0] ?? '', 10))
      .filter((id) => Number.isInteger(id) && id > 0);
  }

  function defaultEntityId(datalist, usedIds) {
    const options = entityOptions(datalist);
    return options.find((id) => !usedIds.has(id)) ?? options[0] ?? 1;
  }

  function switchToCustomEvent(player) {
    const sourceEventId = player.currentEvent;
    const sourceEvent = editableEventSnapshot(sourceEventId, player)
      ?? defaultEditableEvent(player.activityMode);
    const sourceSongs = sourceEventId == null
      ? []
      : player.eventSongs[String(sourceEventId)] ?? [];
    player.currentEvent = customEventId;
    player.eventOverrides[customEventId] = {
      ...editableEventOverride(sourceEvent),
      eventType: defaultEventTypeForMode(player.activityMode),
    };
    player.eventSongs[customEventId] = fixedSongListForMode(
      sourceSongs,
      player.activityMode,
      sourceEvent,
    );
    writePlayer(player);
  }

  function eventTimestamp(event) {
    const values = [
      event?.endAt,
      event?.startAt,
    ];
    for (const value of values) {
      const sources = Array.isArray(value) ? value : [value];
      const numbers = sources
        .map(Number)
        .filter((number) => Number.isFinite(number) && number > 0);
      if (numbers.length > 0) {
        return Math.max(...numbers);
      }
    }
    return 0;
  }

  function recentEventForMode(events, mode) {
    let selected;
    for (const [eventId, event] of Object.entries(events ?? {})) {
      if (!eventMatchesActivityMode(event, mode)) {
        continue;
      }
      const id = Number.parseInt(eventId, 10);
      if (!Number.isInteger(id) || id <= 0) {
        continue;
      }
      const timestamp = eventTimestamp(event);
      if (
        !selected
        || timestamp > selected.timestamp
        || (timestamp === selected.timestamp && id > selected.id)
      ) {
        selected = { id, event, timestamp };
      }
    }
    return selected;
  }

  async function handleActivityModeChange() {
    try {
      const requestedMode = normalizedActivityMode(elements.activityMode.value);
      const core = await ensureCore({ refreshManifest: true });
      const player = normalizedPlayer(readPlayer());
      player.activityMode = requestedMode;
      const eventId = player.currentEvent;
      const event = core?.events?.[String(eventId)];
      if (eventId === customEventId) {
        const key = String(customEventId);
        player.eventOverrides[key] = {
          ...defaultEditableEvent(player.activityMode),
          ...(player.eventOverrides[key] ?? {}),
          eventType: defaultEventTypeForMode(player.activityMode),
        };
        ensureSongListForMode(player, customEventId, player.eventOverrides[key]);
      } else if (!event || !eventMatchesActivityMode(event, player.activityMode)) {
        const recent = recentEventForMode(core?.events, player.activityMode);
        if (!recent) {
          throw new Error('没有可用于当前模式的活动');
        }
        player.currentEvent = recent.id;
        cacheLoadedEventPreset(player, recent.id, recent.event, { overwrite: true });
        player.eventSongs[String(recent.id)] = defaultSongListForMode(
          player.activityMode,
          recent.event,
        );
        delete player.eventOverrides[String(recent.id)];
      } else if (eventId != null) {
        ensureSongListForMode(player, eventId, event);
      }
      writePlayer(player);
      renderReferenceOptions();
      renderConfigForms(player);
      if (player.currentEvent !== eventId) {
        setStatus('已切换到最近活动');
      }
    } catch (error) {
      setError(error);
    }
  }

  async function handleEventSearchChange() {
    const input = elements.eventSearch;
    clearFieldValidationMessage(input);
    if (!input.value.trim()) {
      return;
    }
    let eventId;
    try {
      eventId = parseEventIdFromSearch(input.value);
    } catch (error) {
      setFieldValidationMessage(input, error);
      return;
    }

    if (eventId === customEventId) {
      const player = normalizedPlayer(readPlayer());
      switchToCustomEvent(player);
      renderConfigForms(player);
      setStatus('已切换为自定义活动');
      return;
    }

    setStatus('加载活动预设');
    try {
      const core = await ensureCore({ refreshManifest: true });
      const player = normalizedPlayer(readPlayer());
      const event = await loadEventRecord(eventId, core);
      assertSupportedEvent(event);
      if (!eventMatchesActivityMode(event, player.activityMode)) {
        throw new Error(`当前模式不能选择 ${event.eventType} 活动`);
      }
      player.currentEvent = eventId;
      cacheLoadedEventPreset(player, eventId, event);
      player.eventSongs[String(eventId)] = defaultSongListForMode(player.activityMode, event);
      delete player.eventOverrides[String(eventId)];
      writePlayer(player);
      renderConfigForms(player);
      setStatus('活动预设已加载');
    } catch (error) {
      const message = error?.message ?? '';
      if (
        message.includes('活动 无效') || message.includes('不能选择')
      ) {
        setFieldValidationMessage(input, error);
        return;
      }
      setError(error);
    }
  }

  function handleCustomEvent() {
    try {
      const player = normalizedPlayer(readPlayer());
      player.activityMode = normalizedActivityMode(elements.activityMode.value);
      switchToCustomEvent(player);
      renderConfigForms(player);
      setStatus('已切换为自定义活动');
    } catch (error) {
      setError(error);
    }
  }

  function handleEventScalarChange() {
    const parameterPercent = readNumericField(
      elements.eventCombinedPercent,
      '综合力',
    );
    if (parameterPercent === undefined) {
      return;
    }
    updateCurrentEvent((event) => {
      event.eventAttributeAndCharacterBonus ??= {};
      event.eventAttributeAndCharacterBonus.parameterPercent = parameterPercent;
    });
  }

  function handleEventCharacterParamChange() {
    const performance = readNumericField(
      elements.eventCharacterParamPerformance,
      '演出',
    );
    const technique = readNumericField(
      elements.eventCharacterParamTechnique,
      '技巧',
    );
    const visual = readNumericField(
      elements.eventCharacterParamVisual,
      '形象',
    );
    if (performance === undefined || technique === undefined || visual === undefined) {
      return;
    }
    updateCurrentEvent((event) => {
      event.eventCharacterParameterBonus = {
        performance,
        technique,
        visual,
      };
    });
  }

  function handleAddEventAttribute() {
    try {
      updateCurrentEvent((event) => {
        const attribute = defaultAttributeBonus(event.attributes);
        event.attributes = normalizedEventAttributes(event.attributes)
          .filter((bonus) => bonus.attribute !== attribute);
        event.attributes.push({
          attribute,
          percent: DEFAULT_EVENT_BONUS_PERCENT,
        });
      });
    } catch (error) {
      setError(error);
    }
  }

  function handleAddEventCharacter() {
    updateCurrentEvent((event) => {
      const usedCharacterIds = new Set(
        normalizedEventCharacters(event.characters).map((bonus) => bonus.characterId),
      );
      const characterId = defaultEntityId(elements.characterOptions, usedCharacterIds);
      event.characters = normalizedEventCharacters(event.characters)
        .filter((bonus) => bonus.characterId !== characterId);
      event.characters.push({
        characterId,
        percent: DEFAULT_EVENT_BONUS_PERCENT,
      });
    });
  }

  function handleAddEventMember() {
    updateCurrentEvent((event) => {
      const usedCardIds = new Set(
        normalizedEventMembers(event.members).map((bonus) => bonus.situationId),
      );
      const cardId = defaultEntityId(elements.cardOptions, usedCardIds);
      event.members = normalizedEventMembers(event.members)
        .filter((bonus) => bonus.situationId !== cardId);
      event.members.push({
        situationId: cardId,
        percent: DEFAULT_EVENT_BONUS_PERCENT,
      });
    });
  }

  function handleEventIdChange() {
    const input = elements.eventId;
    clearFieldValidationMessage(input);
    try {
      const player = normalizedPlayer(readPlayer());
      const value = (() => {
        if (!input.value.trim()) {
          return undefined;
        }
        const parsed = readOptionalInteger(input.value);
        clearFieldValidationMessage(input);
        return parsed;
      })();
      if (value !== undefined) {
        assertSupportedKnownEvent(value);
        const event = editableEventSnapshot(value, player);
        player.currentEvent = value;
        ensureSongListForMode(player, value, event);
        writePlayer(player);
        elements.eventSearch.value = eventSearchValue(value, player);
      }
      renderConfigForms(player);
    } catch (error) {
      setFieldValidationMessage(input, error);
    }
  }

  return {
    handleActivityModeChange,
    handleAddEventAttribute,
    handleAddEventCharacter,
    handleAddEventMember,
    handleCustomEvent,
    handleEventCharacterParamChange,
    handleEventIdChange,
    handleEventScalarChange,
    handleEventSearchChange,
  };
}
