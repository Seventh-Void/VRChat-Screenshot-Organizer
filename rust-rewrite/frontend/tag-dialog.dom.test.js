// Drives the real tag-dialog handlers from index.html inside jsdom.
const assert = require('node:assert/strict');
const { JSDOM } = require('jsdom');
const fs = require('node:fs');

const stateSource = fs.readFileSync(`${__dirname}/tagging-state.js`, 'utf8');
const html = fs.readFileSync(`${__dirname}/index.html`, 'utf8')
  .replace('<script src="tagging-state.js"></script>', () => `<script>${stateSource}</script>`);

const dom = new JSDOM(html, {
  url: 'http://localhost/',
  runScripts: 'dangerously',
  pretendToBeVisual: true,
  beforeParse(window) {
    window.IntersectionObserver = class { observe() {} unobserve() {} };
    // jsdom has <dialog> but no modal API.
    window.HTMLDialogElement.prototype.showModal = function () { this.open = true; };
    window.HTMLDialogElement.prototype.close = function () { this.open = false; };
    window.localStorage.setItem('vrchat-organizer-onboarding-complete', 'true');
    window.__TAURI__ = {
      core: { invoke: async cmd => (cmd === 'get_library_root' || cmd === 'get_default_path' ? '/library' : null), convertFileSrc: path => path },
      event: { listen: async () => () => {} }
    };
  }
});
const { window } = dom;
const { document } = window;
const $ = selector => document.querySelector(selector);
const selected = () => JSON.stringify(Array.from(window.eval('tagDialogState.selected')));
const visible = () => [...document.querySelectorAll('#tagSuggestions [data-tag-name]')].map(button => button.dataset.tagName);
const type = value => { $('#tagPersonSearch').value = value; $('#tagPersonSearch').dispatchEvent(new window.Event('input', { bubbles: true })); };
const key = name => $('#tagPersonSearch').dispatchEvent(new window.KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }));
const openMenu = () => window.openTagMenu({ preventDefault() {}, clientX: 20, clientY: 20 });

(async () => {
  let passed = 0;
  const check = (fn, ...args) => { fn(...args); passed += 1; };
  await new Promise(resolve => setTimeout(resolve, 20));
  // 'Ćóâl' stored decomposed (NFD) on purpose: search must still match the composed query.
  window.openViewer({ name: 'World', photos: [{ path: '/library/a.png', participants: ['Beta', 'Alpha', 'Ćóâl'], taggedParticipants: [] }] }, 0);
  await new Promise(resolve => setTimeout(resolve, 0));

  // Suggestion pointerdown selects without the outside-click handler closing the dialog.
  openMenu();
  const accented = document.querySelector('#tagSuggestions [data-tag-name^="C"]');
  for (const eventType of ['pointerdown', 'mousedown']) accented.dispatchEvent(new window.MouseEvent(eventType, { bubbles: true, cancelable: true }));
  $('#tagSuggestions').dispatchEvent(new window.MouseEvent('click', { bubbles: true }));
  check(assert.equal, $('#tagMenu').hidden, false, 'suggestion click must not close the dialog');
  check(assert.equal, selected(), JSON.stringify(['Ćóâl']), 'accented suggestion must enter selected state once');

  // Highlight indexes the visible (filtered) list: "be" → only Beta is shown, so ArrowDown+Enter must pick Beta.
  openMenu();
  type('be');
  check(assert.deepEqual, visible(), ['Beta'], 'filter shows only Beta');
  key('ArrowDown');
  check(assert.equal, $('#tagSuggestions .active')?.dataset.tagName, 'Beta', 'highlight is on the visible row');
  key('Enter');
  check(assert.equal, selected(), JSON.stringify(['Beta']), 'Enter tags the highlighted visible person');

  // Normalized compare: composed lowercase query finds the decomposed participant.
  type('ćó');
  check(assert.equal, visible().length, 1, 'normalized search matches decomposed name');

  window.close();
  console.log(`tag dialog DOM regression: ${passed} assertions passed, 0 failed`);
})().catch(error => { console.error(error.stack); window.close(); process.exitCode = 1; });
