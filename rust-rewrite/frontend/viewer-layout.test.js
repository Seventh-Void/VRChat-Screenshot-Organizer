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
        core: { invoke: async cmd => (cmd === 'get_library_root' || cmd === 'get_default_path' ? '/library' : null), convertFileSrc: value => value },
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
    assert.deepEqual(pageErrors, [], 'page errors');
    console.log(`viewer layout: ${cases} cases passed, 0 failed`);
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
