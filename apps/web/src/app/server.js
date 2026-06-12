const SERVER_INDEX = {
  jp: 0,
  en: 1,
  tw: 2,
  cn: 3,
  kr: 4,
};

export function createServerContext({
  getPlayerServer,
  getServerInputValue,
  normalizeServer,
}) {
  function serverIndex() {
    return SERVER_INDEX[currentServer()] ?? SERVER_INDEX.cn;
  }

  function currentServer() {
    try {
      return getPlayerServer();
    } catch {
      return normalizeServer(getServerInputValue());
    }
  }

  return {
    currentServer,
    serverIndex,
  };
}
