// Loads the real lite.html in Chromium with a stubbed Tauri bridge and checks
// that Lite drives the shared engine with the Full edition's options.
const assert = require('node:assert/strict');
const { chromium } = require('playwright-core');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const ENGINE = { dryRun: false, singleFolder: false, template: '{world}' };

async function openLite(browser, { root = null, settings = {}, watching = false, seen = true, failStart = false } = {}) {
  const page = await browser.newPage({ viewport: { width: 360, height: 460 } });
  const errors = [];
  page.on('pageerror', error => errors.push(String(error)));
  await page.addInitScript(([root, settings, watching, seen, failStart]) => {
    // Same key the Full edition sets after its one-time "files will be moved" warning.
    if (seen) localStorage.setItem('vrchat-organizer-change-warning-seen', 'true');
    else localStorage.removeItem('vrchat-organizer-change-warning-seen');
    window.__calls = [];
    window.__listeners = {};
    const state = window.__state = { root, settings: { watch: false, startInTray: false, scanAllMonths: false, ...settings }, watching };
    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args = {}) => {
          window.__calls.push([cmd, args]);
          switch (cmd) {
            case 'get_lite_settings': return state.settings;
            case 'set_lite_settings': state.settings = args.settings; return null;
            case 'get_library_root': if (state.root === 'MISSING') throw new Error('saved folder no longer exists'); return state.root;
            case 'pick_library_folder': state.root = '/library'; return state.root;
            case 'use_detected_folder': state.root = '/detected'; return state.root;
            case 'is_watching': return state.watching;
            case 'start_watching': if (failStart) throw new Error('path does not exist'); state.watching = true; return 'watching';
            case 'stop_watching': state.watching = false; return 'stopped';
            case 'organize_folder': return { organized: 3, noMetadata: 1, errors: 0 };
            case 'undo_last_run': return { undone: 3, errors: 0 };
            default: throw new Error(`Lite must not call ${cmd}`);
          }
        }
      },
      event: { listen: async (name, handler) => { window.__listeners[name] = handler; return () => {}; } }
    };
  }, [root, settings, watching, seen, failStart]);
  await page.goto(pathToFileURL(path.join(__dirname, 'lite.html')).href);
  await page.waitForFunction(() => document.body.dataset.ready === 'true');
  return { page, errors };
}
const calls = (page, cmd) => page.evaluate(cmd => window.__calls.filter(([name]) => name === cmd).map(([, args]) => args), cmd);

(async () => {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM || '/usr/bin/chromium', args: ['--no-sandbox'] });
  let cases = 0;
  try {
    // First run: no folder. Nothing that moves files is possible yet.
    let { page, errors } = await openLite(browser);
    assert.match(await page.textContent('#status'), /Choose your VRChat screenshot folder/);
    assert.equal(await page.isDisabled('#organize'), true);
    assert.equal(await page.isDisabled('#watchToggle'), true);
    assert.deepEqual(await calls(page, 'start_watching'), []);
    await page.click('#chooseFolder');
    assert.equal(await page.textContent('#folder'), '/library');
    assert.equal(await page.isDisabled('#organize'), false);
    assert.deepEqual(errors, []);
    await page.close(); cases += 1;

    // Turning auto-organize on saves the setting and starts the shared watcher with Full's options.
    ({ page, errors } = await openLite(browser, { root: '/library' }));
    await page.click('#watchToggle');
    assert.deepEqual((await calls(page, 'set_lite_settings')).at(-1).settings.watch, true);
    assert.deepEqual(await calls(page, 'start_watching'), [{ ...ENGINE, scanAllMonths: false }]);
    assert.equal(await page.getAttribute('#watchToggle', 'aria-checked'), 'true');
    assert.match(await page.textContent('#status'), /Watcher on/);

    // Watcher mode switched off from the tray: the window follows.
    await page.evaluate(async () => {
      window.__state.settings.watch = false;
      window.__state.watching = false;
      await window.__listeners['watch-status']({ payload: { status: 'stopped' } });
    });
    assert.equal(await page.getAttribute('#watchToggle', 'aria-checked'), 'false');
    assert.match(await page.textContent('#status'), /Watcher off/);
    await page.click('#watchToggle');

    // Review Focus 5: changing the month option restarts the watcher with the new value.
    await page.click('#monthsToggle');
    assert.deepEqual((await calls(page, 'stop_watching')).length, 1);
    assert.deepEqual((await calls(page, 'start_watching')).at(-1), { ...ENGINE, scanAllMonths: true });

    // Organize now and undo use the same commands as Full.
    await page.click('#organize');
    assert.deepEqual(await calls(page, 'organize_folder'), [], 'all-months scan asks first');
    assert.match(await page.textContent('#activity'), /every month/);
    await page.click('#organize');
    assert.deepEqual(await calls(page, 'organize_folder'), [{ ...ENGINE, scanAllMonths: true }]);
    await page.waitForFunction(() => /Organized 3/.test(document.querySelector('#activity').textContent));
    await page.click('#undo');
    assert.deepEqual(await calls(page, 'undo_last_run'), [], 'undo asks first');
    await page.click('#undo');
    await page.waitForFunction(() => /Undid 3/.test(document.querySelector('#activity').textContent));

    // Watcher events show up as one short line.
    await page.evaluate(() => window.__listeners['watch-organized']({ payload: { stats: { organized: 1 }, file: '/library/2026-10/VRChat_x.png' } }));
    assert.match(await page.textContent('#activity'), /VRChat_x\.png/);

    // Fits the fixed 360x460 window: no scrolling.
    const overflow = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.scrollHeight]);
    assert(overflow[0] <= 360 && overflow[1] <= 460, `overflow ${overflow}`);
    assert.deepEqual(errors, []);
    await page.close(); cases += 1;

    // Saved "watch on" + folder: Lite resumes watching by itself.
    ({ page, errors } = await openLite(browser, { root: '/library', settings: { watch: true } }));
    assert.deepEqual(await calls(page, 'start_watching'), [{ ...ENGINE, scanAllMonths: false }]);
    assert.deepEqual(errors, []);
    await page.close(); cases += 1;

    // Review Focus 2: saved folder deleted. Lite asks for a folder instead of failing.
    ({ page, errors } = await openLite(browser, { root: 'MISSING', settings: { watch: true } }));
    assert.match(await page.textContent('#status'), /Choose your VRChat screenshot folder/);
    assert.deepEqual(await calls(page, 'start_watching'), []);
    assert.deepEqual(errors, []);
    await page.close(); cases += 1;

    // First move ever (Full's warning never accepted): explain, then a second click proceeds.
    ({ page, errors } = await openLite(browser, { root: '/library', seen: false }));
    await page.click('#watchToggle');
    assert.deepEqual(await calls(page, 'start_watching'), []);
    assert.equal(await page.getAttribute('#watchToggle', 'aria-checked'), 'false');
    assert.match(await page.textContent('#activity'), /moves screenshots into world folders/);
    await page.click('#watchToggle');
    assert.equal((await calls(page, 'start_watching')).length, 1);
    assert.equal(await page.evaluate(() => localStorage.getItem('vrchat-organizer-change-warning-seen')), 'true');
    assert.deepEqual(errors, []);
    await page.close(); cases += 1;

    // Watcher fails to start: the switch and the saved setting go back to off.
    ({ page, errors } = await openLite(browser, { root: '/library', failStart: true }));
    await page.click('#watchToggle');
    await page.waitForFunction(() => /Could not start watching/.test(document.querySelector('#activity').textContent));
    assert.equal(await page.getAttribute('#watchToggle', 'aria-checked'), 'false');
    assert.equal((await calls(page, 'set_lite_settings')).at(-1).settings.watch, false);
    assert.deepEqual(errors, []);
    await page.close(); cases += 1;

    console.log(`lite: ${cases} cases passed, 0 failed`);
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
