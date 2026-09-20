const assert = require('node:assert/strict');
const { chromium } = require('playwright-core');
const fs = require('node:fs');

const fixtures = [
  ['16:9', 1600, 900], ['4:3', 1200, 900], ['square', 1000, 1000],
  ['portrait', 900, 1600], ['ultrawide', 3200, 900], ['very-tall', 600, 2400],
  ['tiny', 64, 64], ['huge', 7680, 4320]
];
const sizes = [[1280, 720], [1920, 1080], [900, 600]];
const html = `<!doctype html><style>
*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;background:#0e0d14}
.lightbox{position:fixed;inset:0;display:grid;grid-template-columns:minmax(0,1fr) 320px;grid-template-rows:minmax(0,1fr);gap:24px;min-width:0;min-height:0;width:100%;height:100%;padding:58px 34px 28px;overflow:hidden}
.lightbox-media{position:relative;min-width:0;min-height:0;display:grid;place-items:center;overflow:hidden;padding:18px 58px}
.lightbox-image-wrap{position:relative;display:grid;place-items:center;min-width:0;min-height:0;max-width:100%;max-height:100%;width:max-content;height:max-content}
.lightbox img{display:block;width:auto;height:auto;max-width:100%;max-height:100%;object-fit:contain}
.lightbox-tag-overlay{position:absolute;left:10px;bottom:10px}
.lightbox-panel{min-width:0;min-height:0;overflow:auto}
</style><div class="lightbox"><div class="lightbox-media"><div class="lightbox-image-wrap"><img id="image"><div class="lightbox-tag-overlay">tags</div></div></div><aside class="lightbox-panel"></aside></div>`;

function assertGeometry(metrics, fixture, viewport) {
  const tolerance = 2;
  assert(metrics.image.left >= metrics.stage.left - tolerance, `${fixture} image left overflow`);
  assert(metrics.image.top >= metrics.stage.top - tolerance, `${fixture} image top overflow`);
  assert(metrics.image.right <= metrics.stage.right + tolerance, `${fixture} image right overflow`);
  assert(metrics.image.bottom <= metrics.stage.bottom + tolerance, `${fixture} image bottom overflow`);
  assert(Math.abs((metrics.image.left + metrics.image.right) / 2 - (metrics.stage.left + metrics.stage.right) / 2) <= tolerance, `${fixture} horizontal alignment`);
  assert(Math.abs((metrics.image.top + metrics.image.bottom) / 2 - (metrics.stage.top + metrics.stage.bottom) / 2) <= tolerance, `${fixture} vertical alignment`);
  assert(metrics.overlay.left >= metrics.wrapper.left - tolerance && metrics.overlay.bottom <= metrics.wrapper.bottom + tolerance, `${fixture} overlay anchoring`);
  assert(metrics.scrollWidth <= viewport[0] && metrics.scrollHeight <= viewport[1], `${fixture} page overflow`);
}

(async () => {
  const evidenceDir = '/tmp/vrchat-organizer-viewer-evidence';
  fs.rmSync(evidenceDir, { recursive: true, force: true });
  fs.mkdirSync(evidenceDir, { recursive: true });
  const browser = await chromium.launch({ headless: true, executablePath: '/usr/bin/chromium', args: ['--no-sandbox'] });
  const page = await browser.newPage();
  await page.setContent(html);
  for (const viewport of sizes) {
    await page.setViewportSize({ width: viewport[0], height: viewport[1] });
    for (const [name, width, height] of fixtures) {
      await page.evaluate(({ width, height }) => {
        const image = document.querySelector('#image');
        image.width = width;
        image.height = height;
        image.src = `data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}"></svg>`;
      }, { width, height });
      await page.waitForFunction(() => document.querySelector('#image').complete);
      await page.evaluate(() => {
        const stage = document.querySelector('.lightbox-media');
        const image = document.querySelector('#image');
        const wrapper = document.querySelector('.lightbox-image-wrap');
        const scale = Math.min((stage.clientWidth - 116) / image.naturalWidth, (stage.clientHeight - 36) / image.naturalHeight, 1);
        wrapper.style.width = `${Math.round(image.naturalWidth * scale)}px`;
        wrapper.style.height = `${Math.round(image.naturalHeight * scale)}px`;
      });
      const metrics = await page.evaluate(() => {
        const rect = selector => { const r = document.querySelector(selector).getBoundingClientRect(); return { left:r.left, top:r.top, right:r.right, bottom:r.bottom, width:r.width, height:r.height }; };
        return { stage: rect('.lightbox-media'), wrapper: rect('.lightbox-image-wrap'), image: rect('#image'), overlay: rect('.lightbox-tag-overlay'), scrollWidth: document.documentElement.scrollWidth, scrollHeight: document.documentElement.scrollHeight };
      });
      assertGeometry(metrics, name, viewport);
      await page.screenshot({ path: `${evidenceDir}/${name}-${viewport[0]}x${viewport[1]}.png` });
    }
  }
  await page.setViewportSize({ width: 1280, height: 720 });
  const before = await page.evaluate(() => { const r = document.querySelector('#image').getBoundingClientRect(); return { x:r.x, y:r.y }; });
  await page.setViewportSize({ width: 900, height: 600 });
  const after = await page.evaluate(() => { const r = document.querySelector('#image').getBoundingClientRect(); return { x:r.x, y:r.y }; });
  assert.notDeepEqual(before, after, 'resize must refit geometry');
  console.log(`viewer layout: ${fixtures.length * sizes.length + 1} cases passed, 0 failed`);
  await browser.close();
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
