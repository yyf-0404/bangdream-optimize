export function createDiagnostics({
  getRuntime,
  getCore,
  appendLog,
}) {
  async function buildDiagnostic({ player, server, eventId, result }) {
    const runtimeInfo = await readRuntimeInfo();
    const core = getCore();
    return {
      schemaVersion: 1,
      generatedAt: new Date().toISOString(),
      runtime: getRuntime()?.kind ?? runtimeInfo?.runtime ?? 'unknown',
      runtimeInfo,
      server,
      eventId: eventId ?? player.currentEvent ?? null,
      result: cloneJson(result),
      player: cloneJson(player),
      gameData: {
        cachedCore: Boolean(core),
        cardCount: Object.keys(core?.cards ?? {}).length,
        songCount: Object.keys(core?.songs ?? {}).length,
        eventCount: Object.keys(core?.events ?? {}).length,
      },
    };
  }

  async function readRuntimeInfo() {
    const runtime = getRuntime();
    if (!runtime?.runtimeInfo) {
      return undefined;
    }
    try {
      return await runtime.runtimeInfo();
    } catch (error) {
      appendLog(`runtime-info-error: ${error.message ?? String(error)}`);
      return undefined;
    }
  }

  return {
    buildDiagnostic,
  };
}

function cloneJson(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}

export function diagnosticFileName(diagnostic) {
  const eventId = diagnostic.eventId ?? 'event';
  const timestamp = diagnostic.generatedAt.replace(/[:.]/g, '-');
  return `bangdream-optimize-diagnostic-${eventId}-${timestamp}.json`;
}
