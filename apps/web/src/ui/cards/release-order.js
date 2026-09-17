export const datePriority = ['jp', 'cn', 'en', 'tw', 'kr'];

export function releaseTimestamp(value) {
  const timestamp = Number(value);
  return Number.isFinite(timestamp) && timestamp > 0 ? timestamp : null;
}

// Cache public release metadata only. Ownership, language and growth changes do
// not rebuild the graph; changed dates invalidate it even after an in-place edit.
let cachedKey, cachedOrder;
export function cardReleaseOrder(cards) {
  const entries = cards.map(card => ({
    id: Number(card.id),
    dates: datePriority.map(server => releaseTimestamp(card.releaseDates?.[server])),
  })).filter(card => card.dates.some(date => date !== null)).sort((a, b) => a.id - b.id);
  const key = entries.map(card => `${card.id}:${card.dates.join(',')}`).join(';');
  if (key === cachedKey) return cachedOrder;
  const order = buildReleaseOrder(entries);
  cachedKey = key;
  cachedOrder = order;
  return order;
}

function buildReleaseOrder(cards) {
  const successors = cards.map(() => new Set());
  const predecessors = cards.map(() => new Set());
  // A temporary reachability bitmap makes both cycle and redundant-edge checks
  // constant time. Only newly reachable bits propagate to the source's ancestors.
  // At 2,500 cards this uses about 0.75 MiB, released after the ranks are built.
  const words = Math.ceil(cards.length / 32);
  const reachable = new Uint32Array(cards.length * words);
  function addConstraint(from, to) {
    const fromWord = from >>> 5, fromBit = 1 << (from & 31);
    const toWord = to >>> 5, toBit = 1 << (to & 31);
    const fromRow = from * words, toRow = to * words;
    if (reachable[fromRow + toWord] & toBit) return;
    if (reachable[toRow + fromWord] & fromBit) return;
    const changedWords = [], changedBits = [];
    for (let word = 0; word < words; word++) {
      const bits = (reachable[toRow + word] | (word === toWord ? toBit : 0)) & ~reachable[fromRow + word];
      if (bits) { changedWords.push(word); changedBits.push(bits); }
    }
    for (let ancestor = 0; ancestor < cards.length; ancestor++) {
      const row = ancestor * words;
      if (ancestor !== from && !(reachable[row + fromWord] & fromBit)) continue;
      for (let i = 0; i < changedWords.length; i++) reachable[row + changedWords[i]] |= changedBits[i];
    }
    successors[from].add(to);
    predecessors[to].add(from);
  }

  // Each server contributes newest-to-oldest constraints. Adjacent date buckets
  // imply the rest transitively. Equal timestamps do not impose a date ordering.
  // Process servers by priority, and within a server process newest buckets and
  // descending IDs first, so conflicting lower-priority constraints resolve
  // deterministically without removing any previously accepted relationship.
  for (let server = 0; server < datePriority.length; server++) {
    const timeline = cards.map((card, index) => index)
      .filter(index => cards[index].dates[server] !== null)
      .sort((a, b) => cards[b].dates[server] - cards[a].dates[server] || cards[b].id - cards[a].id);
    let previous = [], current = [], time;
    for (const node of timeline) {
      const nextTime = cards[node].dates[server];
      if (nextTime !== time) {
        previous = current;
        current = [];
        time = nextTime;
      }
      for (const newer of previous) addConstraint(newer, node);
      current.push(node);
    }
  }

  const earliest = cards.map(card => Math.min(...card.dates.filter(date => date !== null)));
  function ranks(ascending) {
    const outgoing = ascending ? predecessors : successors;
    const incoming = ascending ? successors : predecessors;
    const remaining = incoming.map(nodes => nodes.size), ready = [];
    const compare = (a, b) => (ascending ? earliest[a] - earliest[b] : earliest[b] - earliest[a])
      || cards[b].id - cards[a].id;
    function push(node) {
      let index = ready.length;
      ready.push(node);
      while (index > 0) {
        const parent = (index - 1) >> 1;
        if (compare(ready[parent], node) <= 0) break;
        ready[index] = ready[parent];
        index = parent;
      }
      ready[index] = node;
    }
    function pop() {
      const first = ready[0], last = ready.pop();
      if (ready.length) {
        let index = 0;
        while (index * 2 + 1 < ready.length) {
          let child = index * 2 + 1;
          if (child + 1 < ready.length && compare(ready[child + 1], ready[child]) < 0) child++;
          if (compare(last, ready[child]) <= 0) break;
          ready[index] = ready[child];
          index = child;
        }
        ready[index] = last;
      }
      return first;
    }
    for (let node = 0; node < cards.length; node++) if (remaining[node] === 0) push(node);
    const result = new Map();
    while (ready.length) {
      const node = pop();
      result.set(cards[node].id, result.size);
      for (const next of outgoing[node]) if (--remaining[next] === 0) push(next);
    }
    return result;
  }
  return { descending: ranks(false), ascending: ranks(true) };
}
