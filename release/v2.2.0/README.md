# VRChat Organizer 2.2.0

A safer, much smaller and faster release. Avatar AI is gone: people are tagged
by drawing a box and picking a name from the screenshot's player list.

## Highlights

- **Reads VRChat's own metadata** — screenshots without VRCX data are now
  organized by the world name VRChat writes itself, into the same folder
  VRCX-tagged photos of that world use.
- **Fail-safe organizing** — moves never overwrite a file, anything that can't
  be organized is left untouched, and a move whose undo entry can't be written
  is rolled back. Undo never overwrites.
- **Removed avatar AI** — recognition was unreliable (avatars change
  constantly). Drawn boxes and their pictures are kept; old AI data is cleaned
  up on first launch. The AppImage shrinks from 522 MB to about 7 MB.
- **Faster and lighter** — library scans ~20× faster; organizing 5,000 photos
  takes 0.2 s instead of 4.7 s with ~1000× less disk writing; lower idle CPU.
- **Security** — the screenshot folder can only be chosen from the native
  picker or auto-detect and the app can only read that folder; content
  security policy enabled; crafted images can't exhaust memory; symlinks in
  shared folders can't redirect writes.
- **UI** — consistent fonts, spacing and colours; themes apply everywhere;
  keyboard focus and Escape work in all dialogs; in-app dialogs replace
  browser pop-ups; layout fixed at the minimum window size.

The folder layout is unchanged: `<base>/YYYY-MM/<World Name>/VRChat_….png`.

## Release artifacts

| File | Platform | SHA-256 |
|---|---|---|
| `VRChatOrganizer-2.2.0-x86_64.AppImage` | Linux x86_64 GUI | `bbaa1f05ec846e43be7f4835ab70f697d8a91d39605b4bd9edab9449899e4c2f` |
| `VRChatOrganizer-2.2.0-windows-x86_64-cli.exe` | Windows x86_64 command-line tool | `1300fdb72b5f0b33ca46138942aa24fe693a82513c0b3644f8771cd2b527aeb7` |

The Windows `-cli.exe` is the command-line organizer cross-compiled from Linux.
The Windows GUI (portable exe and installers) is built and attached by the
release workflow.

Verify downloads with:

```bash
sha256sum -c SHA256SUMS
```
