import { emptyMessage } from '../ui/dom.js?v=3';
import { difficultyLabel } from '../utils.js?v=3';

export function createSongView({
  rows,
  selectedEventId,
  editableEventSnapshot,
  getSongRecord,
  normalizedActivityMode,
  fixedSongListForMode,
  eventSongsFromPreset,
  songSelectCell,
  updateSong,
}) {
  function renderSongs(player) {
    rows.textContent = '';
    let eventId;
    try {
      eventId = selectedEventId(player);
    } catch {
      rows.append(emptySongMessage('未设置活动'));
      return;
    }
    const event = editableEventSnapshot(eventId, player);
    const mode = normalizedActivityMode(player.activityMode);
    const songs = fixedSongListForMode(player.eventSongs[String(eventId)], mode, event);
    const presetSongs = songPresetForEvent(event);

    songs.forEach((song, index) => {
      const item = document.createElement('article');
      item.className = 'activity-song-item';

      const indexLabel = document.createElement('span');
      indexLabel.className = 'activity-song-index';
      indexLabel.textContent = `第 ${index + 1} 首`;

      const songCell = songSelectCell(
        song.songId,
        (songId) => updateSong(eventId, index, { songId }),
        { presetSongs },
      );
      const songControl = document.createElement('div');
      songControl.className = 'activity-song-control';
      songControl.append(...Array.from(songCell.childNodes));

      const inputCell = songControl.querySelector('.song-input-cell');
      const difficultyList = renderDifficultyList(
        getSongRecord(song.songId),
        song.difficulty,
        (difficulty) => updateSong(eventId, index, { difficulty }),
      );
      if (inputCell) {
        inputCell.append(difficultyList);
      } else {
        songControl.append(difficultyList);
      }

      item.append(indexLabel, songControl);
      rows.append(item);
    });
  }

  function songPresetForEvent(event) {
    const songs = eventSongsFromPreset(event);
    const count = {
      medley: 3,
      challenge: 3,
      versus: 1,
      festival: 1,
    }[String(event?.eventType)] ?? 3;
    return songs.slice(0, count);
  }

  function emptySongMessage(message) {
    return emptyMessage(message, 'activity-song-empty');
  }

  return {
    renderSongs,
  };
}

export function renderDifficultyList(songRecord, selectedDifficulty, onSelect) {
  const list = document.createElement('div');
  list.className = 'activity-song-difficulty-list';

  const entries = Object.entries(songRecord?.difficulty ?? {})
    .map(([difficulty, detail]) => ({
      difficulty: Number.parseInt(difficulty, 10),
      level: Number.parseInt(detail?.playLevel, 10),
    }))
    .filter((entry) =>
      Number.isInteger(entry.difficulty)
      && entry.difficulty >= 0
      && Number.isInteger(entry.level),
    )
    .sort((left, right) => left.difficulty - right.difficulty);

  if (entries.length === 0) {
    list.append(emptyMessage('无难度数据', 'activity-song-difficulty-empty', 'span'));
    return list;
  }

  for (const entry of entries) {
    const interactive = typeof onSelect === 'function';
    const item = document.createElement(interactive ? 'button' : 'span');
    if (interactive) {
      item.type = 'button';
      item.addEventListener('click', () => onSelect(entry.difficulty));
    }
    item.className = `activity-song-difficulty difficulty-${entry.difficulty}`;
    item.classList.toggle('is-selected', entry.difficulty === Number(selectedDifficulty));
    item.setAttribute(
      'aria-label',
      `${difficultyLabel(entry.difficulty)} ${entry.level}`,
    );
    item.title = `${difficultyLabel(entry.difficulty)} ${entry.level}`;
    item.textContent = String(entry.level);
    list.append(item);
  }
  return list;
}
