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
| `VRChatOrganizer-2.2.0-x86_64.AppImage` | Linux x86_64 GUI | `dea4ed49509c172c9d9f178bc870006c9800f871be5795539bd85fb0040e71f9` |
| `VRChatOrganizer-2.2.0-windows-x86_64-portable.exe` | Windows x86_64 GUI, portable (no install) | `fb4feaa7a1e646c30a1598c9c0eb34739242b8ed434023cee7f2bfa659eacb38` |
| `VRChat.Organizer_2.2.0_x64-setup.exe` | Windows x86_64 GUI installer | `3e911806e95a12763cf2146ea3cdb0c06818a71aa3ffcc6021a53ff78d451e9d` |
| `VRChat.Organizer_2.2.0_x64_en-US.msi` | Windows x86_64 GUI installer (MSI) | `bd239f494c582a0642ff4b3d60d2d2ebd1ea3f1975953650b2fc8abbeef9c28b` |
| `VRChatOrganizer-2.2.0-windows-x86_64-cli.exe` | Windows x86_64 command-line tool | `b22a47e90ec9f40ebdafe1e3ababfbdf56d1188ff72c158feca7518a9f960c30` |

On Windows, most people want the installer or the portable exe; the
`-cli.exe` is the command-line organizer only.

Verify downloads with:

```bash
sha256sum -c SHA256SUMS
```
