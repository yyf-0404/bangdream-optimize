import { samplePlayerConfig } from '../storage/user.js?v=1';

export function isDesktopRuntimeAvailable() {
  return getInvoke() != null;
}

export async function createDesktopRuntime() {
  const invoke = getInvoke();
  if (!invoke) {
    throw new Error('Tauri runtime is not available');
  }

  return {
    kind: 'desktop',
    samplePlayerConfig,
    loadPlayerConfig: async () =>
      (await invokeJson(invoke, 'load_player_config')) ?? samplePlayerConfig(),
    savePlayerConfig: (player) => invoke('save_player_config', { player }),
    listPlayerConfigs: () => invokeJson(invoke, 'list_player_configs'),
    selectPlayerConfig: async (configId) =>
      (await invokeJson(invoke, 'select_player_config', { configId })) ?? samplePlayerConfig(),
    createPlayerConfig: ({ name, player }) =>
      invokeJson(invoke, 'create_player_config', { name, player }),
    duplicatePlayerConfig: ({ name, player }) =>
      invokeJson(invoke, 'duplicate_player_config', { name, player }),
    renamePlayerConfig: (configId, name) =>
      invokeJson(invoke, 'rename_player_config', { configId, name }),
    deletePlayerConfig: async (configId) =>
      (await invokeJson(invoke, 'delete_player_config', { configId })) ?? samplePlayerConfig(),
    importBestdoriPlayerProfile: async ({ playerId, server, mode = 3 }) => {
      const payload = await invokeJson(invoke, 'import_bestdori_player_profile', {
        playerId,
        server,
        mode,
      });
      if (!payload?.result || payload?.data?.profile == null) {
        throw new Error('Bestdori 没有返回玩家资料');
      }
      return payload.data.profile;
    },
    clearGameCache: () => invoke('clear_game_cache'),
    refreshCoreGameData: () => invoke('refresh_core_game_data'),
    syncAllGameData: () => invoke('sync_all_game_data'),
    runtimeInfo: () => invokeJson(invoke, 'runtime_info'),
    syncReferenceData: () => invokeJson(invoke, 'sync_reference_data'),
    saveJsonFile: (payload) => saveJsonFileWithPicker(invoke, payload),
    calculate: ({ player, server, eventId, options }) =>
      invokeJson(invoke, 'calculate_for_config', {
        player,
        server,
        eventId,
        options,
      }),
  };
}

async function saveJsonFileWithPicker(invoke, { fileName, text }) {
  const saved = await invoke('save_json_file', { fileName, text });
  return saved ? 'saved' : 'cancelled';
}

async function invokeJson(invoke, command, args) {
  const result = await invoke(command, args);
  return typeof result === 'string' ? JSON.parse(result) : result;
}

function getInvoke() {
  return globalThis.__TAURI__?.core?.invoke
    ?? globalThis.__TAURI__?.tauri?.invoke
    ?? globalThis.__TAURI__?.invoke;
}
