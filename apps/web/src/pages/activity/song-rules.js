// Song and character catalogs assign band IDs independently.
export const songBandOrder = [1, 2, 4, 5, 3, 21, 18, 45];

export function songBandCategory(song) {
  const bandId = Number(song?.bandId);
  return songBandOrder.includes(bandId) ? String(bandId) : 'other';
}

export function compareSongRelease(a, b, server) {
  const index = {jp:0,en:1,tw:2,cn:3,kr:4}[server] ?? 3;
  const time = song => {
    const value = Number(song.record?.publishedAt?.[index]);
    return Number.isFinite(value) && value > 0 ? value : 0;
  };
  return time(b) - time(a) || b.id - a.id;
}

export function compareEventSongOrder(a, b, songIds) {
  const index = id => {
    const position = songIds.indexOf(Number(id));
    return position < 0 ? Number.POSITIVE_INFINITY : position;
  };
  return index(a.id) - index(b.id) || 0;
}
