const assert = require('node:assert/strict');
const { JSDOM } = require('jsdom');
const fs = require('node:fs');
const vm = require('node:vm');

const stateSource = fs.readFileSync(__dirname + '/tagging-state.js', 'utf8');
const dom = new JSDOM('<div id="tagMenu"><input id="search"><div id="suggestions"><button data-tag-name="Ćóâl">Ćóâl</button></div></div>', {
  url: 'http://localhost/'
});
vm.runInNewContext(stateSource, { console, globalThis: dom.window });
const controller = dom.window.VRChatTagging.createTagDialogController(['Ćóâl', 'Floki-AutumnFox']);
const menu = dom.window.document.querySelector('#tagMenu');
const input = dom.window.document.querySelector('#search');
const suggestion = dom.window.document.querySelector('[data-tag-name]');

let closed = false;
dom.window.document.addEventListener('click', event => {
  if (!event.composedPath().includes(menu)) closed = true;
});
suggestion.addEventListener('pointerdown', event => {
  event.preventDefault();
  event.stopPropagation();
  controller.select(event.currentTarget.dataset.tagName);
});

for (const type of ['pointerdown', 'mousedown']) {
  suggestion.dispatchEvent(new dom.window.MouseEvent(type, { bubbles: true, cancelable: true }));
}
input.dispatchEvent(new dom.window.FocusEvent('blur', { bubbles: true }));
input.dispatchEvent(new dom.window.FocusEvent('focusout', { bubbles: true }));
suggestion.dispatchEvent(new dom.window.MouseEvent('mouseup', { bubbles: true }));
suggestion.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true }));

assert.equal(closed, false, 'suggestion click must not close the dialog');
assert.equal(JSON.stringify(Array.from(controller.selected)), JSON.stringify(['Ćóâl']), 'accented suggestion must enter selected state');
assert.equal(menu.isConnected, true, 'dialog root must remain mounted');
console.log('tag dialog DOM regression: 3 assertions passed, 0 failed');
