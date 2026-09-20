const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const context = { globalThis: {} };
vm.runInNewContext(
  fs.readFileSync(`${__dirname}/tagging-state.js`, 'utf8'),
  context
);
const tagging = context.globalThis.VRChatTagging;

const taggedPhoto = { path: 'tagged.png', participants: ['SeventhVoid'], taggedParticipants: [] };
const metadataPhoto = { path: 'metadata.png', participants: ['Ćóâl'], taggedParticipants: [] };
tagging.applyTags(taggedPhoto, ['Floki-AutumnFox']);
const indexes = tagging.buildPeopleIndex([taggedPhoto, metadataPhoto]);

assert.equal(indexes.known.get('floki-autumnfox').sources.has('Tagged image'), true);
assert.equal(indexes.known.get('ćóâl').sources.has('World metadata'), true);
assert.equal(indexes.tagged.get('floki-autumnfox').photos[0].path, taggedPhoto.path);
assert.equal(indexes.tagged.has('ćóâl'), false);
assert.equal(tagging.tagSummary(taggedPhoto.taggedParticipants), 'Tagged: Floki-AutumnFox');
assert.notEqual(tagging.tagSummary(taggedPhoto.taggedParticipants), 'No player data');
assert.equal(tagging.tagSummary([]), 'No people tagged — right-click to add');
assert.equal(tagging.normalizeName('C\u0301o\u0301a\u0302l'), tagging.normalizeName('Ćóâl'));
console.log('frontend tagging state: 8 assertions passed, 0 failed');
