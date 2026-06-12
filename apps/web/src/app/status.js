export function createStatusProxy() {
  let view;

  function attach(nextView) {
    view = nextView;
  }

  function appendLog(message) {
    currentView().appendLog(message);
  }

  function setStatus(message) {
    currentView().setStatus(message);
  }

  function setError(error) {
    currentView().setError(error);
  }

  function currentView() {
    if (!view) {
      throw new Error('status view is not initialized');
    }
    return view;
  }

  return {
    appendLog,
    attach,
    setError,
    setStatus,
  };
}
