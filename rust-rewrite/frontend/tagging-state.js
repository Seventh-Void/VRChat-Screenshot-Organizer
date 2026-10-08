(function (root) {
  function normalizeName(name) {
    return String(name || '').trim().normalize('NFC').toLocaleLowerCase();
  }

  function applyTags(photo, tags) {
    photo.taggedParticipants = [...new Set(tags.map(name => String(name).trim()).filter(Boolean))];
    return photo.taggedParticipants;
  }

  function buildPeopleIndex(photos) {
    const tagged = new Map();
    const known = new Map();
    photos.forEach(photo => {
      const metadata = Array.isArray(photo.participants) ? photo.participants : [];
      const explicit = Array.isArray(photo.taggedParticipants) ? photo.taggedParticipants : [];
      metadata.forEach(name => addKnown(known, name, photo, 'World metadata'));
      explicit.forEach(name => {
        addKnown(known, name, photo, 'Tagged image');
        const key = normalizeName(name);
        if (!tagged.has(key)) tagged.set(key, { name: String(name).trim(), photos: [] });
        tagged.get(key).photos.push(photo);
      });
    });
    return { tagged, known };
  }

  function addKnown(index, name, photo, source) {
    const display = String(name || '').trim();
    if (!display) return;
    const key = normalizeName(display);
    if (!index.has(key)) index.set(key, { name: display, photos: [], sources: new Set() });
    const person = index.get(key);
    person.sources.add(source);
    if (source === 'Tagged image') person.photos.push(photo);
  }

  function tagSummary(tags) {
    return tags.length ? `Tagged: ${tags.join(', ')}` : 'No people tagged — right-click to add';
  }

  function createTagDialogController(names = []) {
    let selected = [];
    let highlighted = -1;
    let visible = names.slice();
    const candidates = () => visible;
    // The dialog shows a filtered list; keep the highlight on the same person (normalized) or clear it.
    const setCandidates = list => {
      const current = highlighted >= 0 ? visible[highlighted] : null;
      visible = list.slice();
      highlighted = current === null ? -1 : visible.findIndex(name => normalizeName(name) === normalizeName(current));
      return visible.slice();
    };
    const select = name => {
      const display = String(name || '').trim();
      if (display && !selected.some(existing => normalizeName(existing) === normalizeName(display))) {
        selected.push(display);
      }
      highlighted = -1;
      return selected.slice();
    };
    const remove = name => {
      selected = selected.filter(existing => normalizeName(existing) !== normalizeName(name));
      return selected.slice();
    };
    const move = direction => {
      if (!candidates().length) return null;
      highlighted = (highlighted + direction + candidates().length) % candidates().length;
      return candidates()[highlighted];
    };
    const acceptHighlighted = () => highlighted >= 0 ? select(candidates()[highlighted]) : selected.slice();
    return {
      get selected() { return selected.slice(); },
      get highlighted() { return highlighted; },
      select,
      setCandidates,
      remove,
      move,
      acceptHighlighted
    };
  }

  root.VRChatTagging = { applyTags, buildPeopleIndex, createTagDialogController, normalizeName, tagSummary };
})(globalThis);
