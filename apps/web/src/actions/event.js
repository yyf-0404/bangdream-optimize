export function createEventActions({
  readPlayer,
  normalizePlayer,
  selectedEventId,
  editableEventSnapshot,
  defaultEventTypeForMode,
  assertSupportedEvent,
  eventMatchesActivityMode,
  editableEventOverride,
  ensureSongListForMode,
  writePlayer,
  renderConfigForms,
  normalizedEventAttributes,
  normalizedEventCharacters,
  normalizedEventMembers,
  fixedSongListForMode,
  renderSongs,
}) {
  function updateCurrentEvent(mutator) {
    const player = normalizePlayer(readPlayer());
    const eventId = selectedEventId(player);
    const event = editableEventSnapshot(eventId, player);
    if (!event) {
      throw new Error('未设置活动参数');
    }

    mutator(event);
    event.eventType = defaultEventTypeForMode(player.activityMode);
    assertSupportedEvent(event);
    if (!eventMatchesActivityMode(event, player.activityMode)) {
      throw new Error(`当前模式不能选择 ${event.eventType} 活动`);
    }
    player.currentEvent = eventId;
    player.eventOverrides[String(eventId)] = editableEventOverride(event);
    ensureSongListForMode(player, eventId, event);
    writePlayer(player);
    renderConfigForms(player);
  }

  function updateEventAttribute(index, patch) {
    updateCurrentEvent((event) => {
      event.attributes = normalizedEventAttributes(event.attributes);
      event.attributes[index] = {
        ...event.attributes[index],
        ...patch,
      };
    });
  }

  function deleteEventAttribute(index) {
    updateCurrentEvent((event) => {
      event.attributes = normalizedEventAttributes(event.attributes);
      event.attributes.splice(index, 1);
    });
  }

  function updateEventCharacter(index, patch) {
    updateCurrentEvent((event) => {
      event.characters = normalizedEventCharacters(event.characters);
      event.characters[index] = {
        ...event.characters[index],
        ...patch,
      };
    });
  }

  function deleteEventCharacter(index) {
    updateCurrentEvent((event) => {
      event.characters = normalizedEventCharacters(event.characters);
      event.characters.splice(index, 1);
    });
  }

  function updateEventMember(index, patch) {
    updateCurrentEvent((event) => {
      event.members = normalizedEventMembers(event.members);
      event.members[index] = {
        ...event.members[index],
        ...patch,
      };
    });
  }

  function deleteEventMember(index) {
    updateCurrentEvent((event) => {
      event.members = normalizedEventMembers(event.members);
      event.members.splice(index, 1);
    });
  }

  function updateSong(eventId, index, patch) {
    const player = readPlayer();
    const event = editableEventSnapshot(eventId, player);
    const songs = fixedSongListForMode(
      player.eventSongs[String(eventId)],
      player.activityMode,
      event,
    );
    songs[index] = {
      ...songs[index],
      ...patch,
    };
    player.eventSongs[String(eventId)] = songs;
    writePlayer(player);
    renderSongs(player);
  }

  return {
    deleteEventAttribute,
    deleteEventCharacter,
    deleteEventMember,
    updateCurrentEvent,
    updateEventAttribute,
    updateEventCharacter,
    updateEventMember,
    updateSong,
  };
}
