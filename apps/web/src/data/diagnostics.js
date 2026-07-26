import { explainCalculationError } from './calculation-errors.js?v=1';

export function createDiagnostics({
  getRuntime,
  getCore,
  appendLog,
}) {
  async function buildDiagnostic({
    player,
    server,
    eventId,
    result,
    error,
    phase,
    calculationRequest,
  }) {
    const runtimeInfo = await readRuntimeInfo();
    const core = getCore();
    return {
      schemaVersion: 2,
      generatedAt: new Date().toISOString(),
      status: error == null ? 'success' : 'failed',
      phase: phase ?? (error == null ? 'completed' : 'calculation'),
      runtime: getRuntime()?.kind ?? runtimeInfo?.runtime ?? 'unknown',
      runtimeInfo,
      server,
      eventId: eventId ?? player.currentEvent ?? null,
      ...(result === undefined ? {} : { result: cloneJson(result) }),
      ...(calculationRequest === undefined
        ? {}
        : { calculationRequest: cloneJson(calculationRequest) }),
      ...(error == null
        ? {}
        : {
            error: serializeError(error, {
              player,
              calculationRequest,
            }),
          }),
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

function serializeError(error, context) {
  const diagnostic = {
    name: typeof error?.name === 'string' && error.name ? error.name : 'Error',
    message: error?.message ?? String(error),
    ...explainCalculationError(error, context),
  };
  if (typeof error?.stack === 'string' && error.stack) {
    diagnostic.stack = error.stack;
  }
  if (typeof error?.executionContext === 'string' && error.executionContext) {
    diagnostic.executionContext = error.executionContext;
  }
  if (error?.cause != null) {
    diagnostic.cause = error.cause?.message ?? String(error.cause);
  }
  return diagnostic;
}

export function diagnosticFileName(diagnostic) {
  const eventId = diagnostic.eventId ?? 'event';
  const timestamp = diagnostic.generatedAt.replace(/[:.]/g, '-');
  return `bangdream-optimize-diagnostic-${eventId}-${timestamp}.json`;
}
