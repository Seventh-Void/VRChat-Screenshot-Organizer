// Loads the real index.html in Chromium and checks the photo viewer fits every aspect ratio.
const assert = require('node:assert/strict');
const { chromium } = require('playwright-core');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const fixtures = [
  ['16:9', 1600, 900], ['4:3', 1200, 900], ['square', 1000, 1000],
  ['portrait', 900, 1600], ['ultrawide', 3200, 900], ['very-tall', 600, 2400],
  ['tiny', 64, 64], ['huge', 7680, 4320]
];
// 860x620 is the minimum window size and uses the stacked (<=900px) viewer layout.
const sizes = [[1280, 720], [1920, 1080], [900, 600], [860, 620]];
const svg = (width, height) => `data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}"></svg>`;

async function measure(page) {
  return page.evaluate(() => {
    const rect = selector => { const r = document.querySelector(selector).getBoundingClientRect(); return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height }; };
    const stage = document.querySelector('#lightboxMedia');
    const style = getComputedStyle(stage);
    const image = document.querySelector('#lightboxImage');
    return {
      stage: rect('#lightboxMedia'), wrapper: rect('#lightboxImageWrap'), image: rect('#lightboxImage'), overlay: rect('#lightboxTagOverlay'),
      contentWidth: stage.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight),
      contentHeight: stage.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom),
      naturalWidth: image.naturalWidth, naturalHeight: image.naturalHeight,
      scrollWidth: document.documentElement.scrollWidth, scrollHeight: document.documentElement.scrollHeight
    };
  });
}

function assertGeometry(m, label, viewport) {
  const tolerance = 2;
  assert(m.image.left >= m.stage.left - tolerance && m.image.right <= m.stage.right + tolerance, `${label} horizontal overflow`);
  assert(m.image.top >= m.stage.top - tolerance && m.image.bottom <= m.stage.bottom + tolerance, `${label} vertical overflow`);
  assert(Math.abs((m.image.left + m.image.right) / 2 - (m.stage.left + m.stage.right) / 2) <= tolerance, `${label} horizontal alignment`);
  assert(Math.abs((m.image.top + m.image.bottom) / 2 - (m.stage.top + m.stage.bottom) / 2) <= tolerance, `${label} vertical alignment`);
  // Image is as large as the stage's content box allows (never upscaled).
  const scale = Math.min(m.contentWidth / m.naturalWidth, m.contentHeight / m.naturalHeight, 1);
  assert(Math.abs(m.image.width - m.naturalWidth * scale) <= tolerance && Math.abs(m.image.height - m.naturalHeight * scale) <= tolerance, `${label} fit scale`);
  // The side panel must not squeeze the stage when the layout stacks.
  assert(m.stage.height >= viewport[1] * 0.3, `${label} stage too short (${Math.round(m.stage.height)}px)`);
  assert(m.overlay.left >= m.wrapper.left - tolerance && m.overlay.bottom <= m.wrapper.bottom + tolerance, `${label} overlay anchoring`);
  assert(m.scrollWidth <= viewport[0] && m.scrollHeight <= viewport[1], `${label} page overflow`);
}

async function showFixture(page, width, height) {
  await page.evaluate(async src => {
    openViewer({ name: 'Fixture', photos: [{ path: src }] }, 0);
    await new Promise(resolve => setTimeout(resolve, 0)); // let loadPositionalTags re-render first
  }, svg(width, height));
  await page.waitForFunction(() => {
    const image = document.querySelector('#lightboxImage');
    return image.complete && image.naturalWidth > 0 && document.querySelector('#lightboxImageWrap').style.width.endsWith('px');
  });
}

(async () => {
  const evidenceDir = path.join(os.tmpdir(), 'vrchat-organizer-viewer-evidence');
  fs.rmSync(evidenceDir, { recursive: true, force: true });
  fs.mkdirSync(evidenceDir, { recursive: true });
  const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM || '/usr/bin/chromium', args: ['--no-sandbox'] });
  try {
    const page = await browser.newPage();
    const pageErrors = [];
    page.on('pageerror', error => pageErrors.push(String(error)));
    await page.addInitScript(() => {
      localStorage.setItem('vrchat-organizer-onboarding-complete', 'true');
      window.__TAURI__ = {
        core: { invoke: async (cmd, args) => window.__invokeOverride?.(cmd, args) ?? (cmd === 'get_library_root' || cmd === 'get_default_path' ? '/library' : null), convertFileSrc: value => value },
        event: { listen: async () => () => {} }
      };
    });
    await page.goto(pathToFileURL(path.join(__dirname, 'index.html')).href);
    let cases = 0;
    for (const viewport of sizes) {
      await page.setViewportSize({ width: viewport[0], height: viewport[1] });
      for (const [name, width, height] of fixtures) {
        await showFixture(page, width, height);
        assertGeometry(await measure(page), `${name} @ ${viewport.join('x')}`, viewport);
        await page.screenshot({ path: path.join(evidenceDir, `${name}-${viewport[0]}x${viewport[1]}.png`) });
        cases += 1;
      }
    }
    // Resizing an open viewer must refit without reloading the image.
    await page.setViewportSize({ width: 1280, height: 720 });
    await showFixture(page, 1600, 900);
    await page.setViewportSize({ width: 860, height: 620 });
    await page.waitForTimeout(100);
    assertGeometry(await measure(page), 'resize 1280x720 -> 860x620', [860, 620]);
    cases += 1;

    // Draw mode: dragging must draw (not drag the <img>), show a live box,
    // ask for a name on release, and still allow resizing existing boxes.
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.evaluate(() => {
      const tag = { id: 7, personName: 'Nova', x: 700, y: 300, width: 60, height: 60, imageWidth: 1600, imageHeight: 900 };
      window.__invokeOverride = (cmd, args) => cmd === 'get_positional_person_tags' ? [tag]
        : cmd === 'update_positional_person_tag_command' ? { ...tag, ...args, id: 7 } : undefined;
      document.addEventListener('dragstart', () => { window.__dragstarts = (window.__dragstarts || 0) + 1; }, true);
    });
    await showFixture(page, 1600, 900);
    await page.waitForSelector('[data-box-person]');
    await page.click('#drawPersonTag');
    const existing = await page.locator('[data-box-person]').boundingBox();
    await page.mouse.move(existing.x + existing.width - 2, existing.y + existing.height - 2);
    await page.mouse.down();
    await page.mouse.move(existing.x + existing.width + 60, existing.y + existing.height + 40, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(50);
    const resized = await page.locator('[data-box-person]').boundingBox();
    assert(resized.width > existing.width + 40, 'existing box resizes in draw mode');
    const image = await page.locator('#lightboxImage').boundingBox();
    await page.mouse.move(image.x + 100, image.y + 80);
    await page.mouse.down();
    await page.mouse.move(image.x + 300, image.y + 230, { steps: 5 });
    const preview = await page.locator('#drawPreview').boundingBox();
    assert(preview && Math.abs(preview.width - 200) <= 2, 'live box follows the pointer');
    await page.mouse.up();
    await page.waitForSelector('#askDialog[open]');
    assert.equal(await page.evaluate(() => window.__dragstarts || 0), 0, 'image must not start a native drag');
    assert.equal(await page.locator('#drawPreview').count(), 0, 'live box removed on release');
    cases += 1;

    // Full: changing "older months" while watching restarts the watcher with the
    // new option, so Full and Lite never organize with different settings.
    await page.evaluate(() => {
      window.__watchCalls = [];
      window.__invokeOverride = (cmd, args) => {
        if (cmd !== 'start_watching' && cmd !== 'stop_watching') return undefined;
        window.__watchCalls.push([cmd, args]);
        return 'ok';
      };
      document.querySelectorAll('dialog[open]').forEach(dialog => dialog.close());
      localStorage.setItem('vrchat-organizer-scan-all-months', 'false');
      setWatching(true);
      document.querySelector('#scanAllMonthsSwitch').click();
    });
    await page.waitForFunction(() => window.__watchCalls.length >= 2, null, { timeout: 2000 });
    const watchCalls = await page.evaluate(() => window.__watchCalls);
    assert.deepEqual(watchCalls.map(([cmd]) => cmd), ['stop_watching', 'start_watching']);
    assert.deepEqual(watchCalls[1][1], { dryRun: false, scanAllMonths: true, singleFolder: false, template: '{world}' });
    cases += 1;
    // WebKitGTK builds a live blurred backdrop for every element with
    // backdrop-filter; one per world card ran the Linux app past 20 GB after
    // an all-months scan. Nothing that repeats per card may use it.
    await page.evaluate(() => {
      document.querySelectorAll('dialog[open]').forEach(dialog => dialog.close());
      document.querySelector('[data-view="library"]').click();
      applyLibrarySnapshot({
        worldDetails: Array.from({ length: 40 }, (_, i) => ({ name: `World ${i}`, lastCapture: 1, photos: [{ path: `/library/2026-10/World ${i}/a.png`, capturedAt: 1 }] })),
        unorganizedPhotos: []
      });
      renderWorlds();
    });
    const blurred = await page.evaluate(() => [...document.querySelectorAll('#worldGrid *')]
      .filter(element => element.getClientRects().length && getComputedStyle(element).backdropFilter !== 'none').length);
    assert.equal(await page.locator('#worldGrid .world').count(), 40);
    assert.equal(blurred, 0, `${blurred} blurred elements in the world grid`);
    cases += 1;
    assert.deepEqual(pageErrors, [], 'page errors');
    console.log(`viewer layout: ${cases} cases passed, 0 failed`);
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
