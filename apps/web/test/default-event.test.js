import assert from 'node:assert/strict';
import test from 'node:test';

import {
  normalizedCalculationMode,
  recentUnfinishedEvent,
} from '../src/models/event.js';
import { createPlayerStore } from '../src/app/player.js';

const CN = 3;
const scoped = (value) => [null, null, null, value, null];
const event = (eventType, startAt, endAt) => ({
  eventType,
  startAt: scoped(startAt),
  endAt: scoped(endAt),
});

test('missing calculation mode defaults to maximum average PT', () => {
  assert.equal(normalizedCalculationMode(undefined), 'ptMaximize');
  assert.equal(normalizedCalculationMode('maximize'), 'maximize');
});

test('default event prefers the most recently started active CN event', () => {
  const now = 10_000;
  const selected = recentUnfinishedEvent({
    1: event('challenge', 1_000, 9_000),
    2: event('challenge', 7_000, 12_000),
    3: event('festival', 8_000, 11_000),
    4: event('medley', 11_000, 13_000),
    5: event('unsupported', 9_000, 20_000),
  }, {
    serverIndex: CN,
    now,
  });

  assert.equal(selected?.id, 3);
});

test('default event uses the nearest upcoming CN event when none is active', () => {
  const now = 10_000;
  const selected = recentUnfinishedEvent({
    1: event('challenge', 1_000, 9_000),
    2: event('medley', 12_000, 15_000),
    3: event('festival', 11_000, 14_000),
  }, {
    serverIndex: CN,
    now,
  });

  assert.equal(selected?.id, 3);
});

test('default event does not fall back to an ended activity', () => {
  const selected = recentUnfinishedEvent({
    1: event('challenge', 1_000, 9_000),
  }, {
    serverIndex: CN,
    now: 10_000,
  });

  assert.equal(selected, undefined);
});

test('initial player selection persists event context and songs together', () => {
  const selectedEvent = event('challenge', 8_000, 12_000);
  const normalizePlayer = (value) => ({
    server: value.server ?? 'cn',
    calculationMode: normalizedCalculationMode(value.calculationMode),
    activityMode: value.activityMode ?? 'single',
    currentEvent: value.currentEvent,
    eventPresets: value.eventPresets ?? {},
    eventSongs: value.eventSongs ?? {},
  });
  const store = createPlayerStore({
    state: {
      core: { events: { 3: selectedEvent } },
      playerSaveSequence: 0,
      playerSaveQueue: Promise.resolve(),
    },
    playerJson: { value: '{}' },
    normalizePlayer,
    cacheEventPresetFromCore(player, eventId) {
      player.eventPresets[String(eventId)] = selectedEvent;
      return true;
    },
    activityModeForEvent: () => 'single',
    ensureSongListForMode(player, eventId) {
      player.eventSongs[String(eventId)] = [{ songId: 1, difficulty: 3 }];
    },
    recentUnfinishedEvent: (events, options) => recentUnfinishedEvent(events, {
      ...options,
      now: 10_000,
    }),
    renderPlayerProfileControls() {},
    onError() {},
  });

  const initialized = store.initializePlayerDefaults({});
  assert.equal(initialized.changed, true);
  assert.equal(initialized.player.server, 'cn');
  assert.equal(initialized.player.calculationMode, 'ptMaximize');
  assert.equal(initialized.player.currentEvent, 3);
  assert.equal(initialized.player.eventPresets['3'], selectedEvent);
  assert.deepEqual(initialized.player.eventSongs['3'], [{ songId: 1, difficulty: 3 }]);
});
