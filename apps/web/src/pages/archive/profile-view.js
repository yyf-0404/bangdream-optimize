export function createProfileView({
  profileSelect,
  profileNameInput,
  deleteButton,
  getProfiles,
  getActiveId,
}) {
  function renderPlayerProfileControls() {
    const profiles = getProfiles();
    const activeId = getActiveId();
    profileSelect.textContent = '';
    for (const profile of profiles) {
      const option = document.createElement('option');
      option.value = profile.id;
      option.textContent = profile.name;
      option.selected = profile.id === activeId;
      profileSelect.append(option);
    }
    const active = activeProfile();
    profileSelect.disabled = profiles.length === 0;
    profileNameInput.disabled = !active;
    profileNameInput.value = active?.name ?? '';
    deleteButton.disabled = profiles.length <= 1;
  }

  function activeProfile() {
    return getProfiles().find((profile) => profile.id === getActiveId());
  }

  function activeProfileName() {
    return activeProfile()?.name ?? '当前配置';
  }

  function nextProfileName(baseName) {
    const existing = new Set(getProfiles().map((profile) => profile.name));
    if (!existing.has(baseName)) {
      return baseName;
    }
    for (let index = 2; ; index += 1) {
      const name = `${baseName} ${index}`;
      if (!existing.has(name)) {
        return name;
      }
    }
  }

  return {
    activeProfile,
    activeProfileName,
    nextProfileName,
    renderPlayerProfileControls,
  };
}
