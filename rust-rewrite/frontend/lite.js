// VRChat Organizer Lite: a few controls over the same Rust engine as the Full app.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
// The exact options the Full app passes, so both editions organize identically.
const ENGINE = { dryRun: false, singleFolder: false, template: '{world}' };
const $ = id => document.getElementById(id);
const TOGGLES = [['watchToggle', 'watch'], ['monthsToggle', 'scanAllMonths'], ['trayToggle', 'startInTray']];
let settings = { watch: false, startInTray: false, scanAllMonths: false };
let root = null;

const note = text => { $('activity').textContent = text; };
// Shared with the Full edition, which sets it after its one-time "files will be moved" warning.
const SEEN_KEY = 'vrchat-organizer-change-warning-seen';
const seenWarning = () => { try { return localStorage.getItem(SEEN_KEY) === 'true'; } catch { return false; } };
const rememberWarning = () => { try { localStorage.setItem(SEEN_KEY, 'true'); } catch { /* private storage: ask again next time */ } };

// The webview has no confirm(); a risky control explains itself and runs on a second click.
let pending = null;
function needsConfirm(id, message) {
  if (pending === id) { pending = null; return false; }
  pending = id;
  note(message);
  setTimeout(() => { if (pending === id) pending = null; }, 6000);
  return true;
}
const FIRST_MOVE = 'Organizer moves screenshots into world folders inside each month folder. Every move can be undone. Click again to continue.';
function confirmFirstMove(id) {
  if (seenWarning()) return true;
  if (needsConfirm(id, FIRST_MOVE)) return false;
  rememberWarning();
  return true;
}
const fileName = file => String(file || '').split(/[\\/]/).pop();
const summary = stats => `Organized ${stats?.organized ?? 0} · ${stats?.noMetadata ?? 0} without world info`;

async function render() {
  $('folder').textContent = root || 'No folder chosen';
  $('folder').title = root || '';
  $('chooseFolder').textContent = root ? 'Change' : 'Choose';
  $('detectFolder').hidden = Boolean(root);
  for (const id of ['organize', 'undo', 'watchToggle']) $(id).disabled = !root;
  for (const [id, key] of TOGGLES) $(id).setAttribute('aria-checked', String(settings[key]));
  const watching = root ? await invoke('is_watching') : false;
  $('status').textContent = !root ? 'Choose your VRChat screenshot folder' : watching ? 'Watcher on: new screenshots are organized automatically' : 'Watcher off';
  $('dot').dataset.state = watching ? 'on' : 'off';
}

async function save(patch) {
  settings = { ...settings, ...patch };
  await invoke('set_lite_settings', { settings });
}

/** Resolves true when the watcher runs afterwards. */
async function startWatching() {
  try {
    await invoke('start_watching', { ...ENGINE, scanAllMonths: settings.scanAllMonths });
    return true;
  } catch (error) {
    if (String(error).includes('already watching')) return true;
    note(`Could not start watching: ${error}`);
    return false;
  }
}

async function useRoot(next) {
  if (!next) return;
  root = next;
  if (settings.watch) {
    await invoke('stop_watching');
    if (!(await startWatching())) await save({ watch: false });
  }
  await render();
}

$('chooseFolder').addEventListener('click', async () => {
  try { await useRoot(await invoke('pick_library_folder')); } catch (error) { note(`Folder not changed: ${error}`); }
});
$('detectFolder').addEventListener('click', async () => {
  try { await useRoot(await invoke('use_detected_folder')); } catch (error) { note(`No VRChat folder found: ${error}`); }
});
$('organize').addEventListener('click', async () => {
  if (!confirmFirstMove('organize')) return;
  if (settings.scanAllMonths && needsConfirm('organize-all', 'This scans every month folder and may take a while. Click Organize now again to continue.')) return;
  $('organize').disabled = true;
  note('Organizing…');
  try { note(summary(await invoke('organize_folder', { ...ENGINE, scanAllMonths: settings.scanAllMonths }))); }
  catch (error) { note(`Could not organize: ${error}`); }
  finally { $('organize').disabled = !root; }
});
$('undo').addEventListener('click', async () => {
  if (needsConfirm('undo', 'Undo moves every photo from the last run back where it was. Click Undo again to confirm.')) return;
  try { note(`Undid ${(await invoke('undo_last_run'))?.undone ?? 0} moves`); } catch (error) { note(`Nothing undone: ${error}`); }
});
$('watchToggle').addEventListener('click', async () => {
  const next = !settings.watch;
  if (next && !confirmFirstMove('watch')) return;
  await save({ watch: next });
  if (!next) await invoke('stop_watching');
  else if (!(await startWatching())) await save({ watch: false });
  await render();
});
$('monthsToggle').addEventListener('click', async () => {
  await save({ scanAllMonths: !settings.scanAllMonths });
  // The watcher keeps the options it started with; restart so the editions never diverge.
  if (settings.watch && root) {
    await invoke('stop_watching');
    if (!(await startWatching())) await save({ watch: false });
  }
  await render();
});
$('trayToggle').addEventListener('click', async () => {
  await save({ startInTray: !settings.startInTray });
  await render();
});

// The tray can flip watcher mode while the window is open; re-read the saved setting.
listen('watch-status', async () => {
  settings = { ...settings, ...(await invoke('get_lite_settings')) };
  await render();
});
listen('watch-organized', event => {
  const name = fileName(event.payload?.file);
  note(event.payload?.stats?.organized ? `Organized ${name}` : `${name} has no world info, left in place`);
});
listen('watch-error', event => note(`Watcher: ${event.payload?.error}`));
listen('watch-initial-scan', event => note(event.payload?.error ? `Startup scan failed: ${event.payload.error}` : summary(event.payload?.stats)));
listen('lite-organize', event => note(event.payload?.error ? `Could not organize: ${event.payload.error}` : summary(event.payload?.stats)));

(async () => {
  settings = { ...settings, ...(await invoke('get_lite_settings')) };
  // A saved folder that was deleted or unmounted errors here; treat it as "not chosen".
  root = await invoke('get_library_root').catch(() => null);
  if (settings.watch && root && !(await invoke('is_watching'))) await startWatching();
  await render();
})()
  .catch(error => { $('status').textContent = `Something went wrong: ${error}`; $('dot').dataset.state = 'error'; })
  .finally(() => { document.body.dataset.ready = 'true'; });
