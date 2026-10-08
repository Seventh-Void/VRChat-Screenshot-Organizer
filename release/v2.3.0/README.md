# VRChat Organizer 2.3.0

Introduces the **Lite edition**: the same organizing engine in a small window
with a tray icon, for people who just want their screenshots sorted.

## Lite edition

- **Watcher mode** organizes new screenshots automatically. Switch it from the
  window or the tray menu.
- **Organize now** and **Undo last run**, with quick settings for including
  older months and starting in the tray.
- Closing the window keeps Lite watching from the tray. It restarts itself as a
  small tray-only process (about 60 MB, no browser view, no idle CPU) once
  any move in progress has finished.
- Same engine and options as the full app, so both produce identical folders.
- Opening Lite again shows the running one instead of starting a second watcher.

Linux: `VRChatOrganizer-2.3.0-lite-x86_64.AppImage`. Windows:
`VRChatOrganizer-2.3.0-windows-x86_64-lite-portable.exe`. Any build also
starts as Lite with `--lite`.

## Fixes

- **Safer moves:** with two organizers watching the same folder on a drive
  without hard links (FAT/exFAT, some network shares), a photo could be
  deleted. A move now never removes its copy when the original is already gone.
- Changing "Include older months" in the full app now restarts a running
  watcher with the new setting.

## Release artifacts

| File | Edition / platform | SHA-256 |
|---|---|---|
| `VRChatOrganizer-2.3.0-x86_64.AppImage` | Linux x86_64 — Full | `7ec8c269d77df217b48ff49a327ddcb19edb9283f14410119e267069b618ed54` |
| `VRChatOrganizer-2.3.0-lite-x86_64.AppImage` | Linux x86_64 — Lite | `f23c2e32cbf820a1771a5a9e6f212f340a52204a37c41ee6d37aa594ab7e85a1` |
| `VRChatOrganizer-2.3.0-windows-x86_64-portable.exe` | Windows x86_64 — Full, portable (no install) | `fecf733c14d4b14902ce1af02812b075ecb6e4280e7f3172e6f90d5159b30899` |
| `VRChatOrganizer-2.3.0-windows-x86_64-lite-portable.exe` | Windows x86_64 — Lite, portable (no install) | `fecf733c14d4b14902ce1af02812b075ecb6e4280e7f3172e6f90d5159b30899` |
| `VRChat.Organizer_2.3.0_x64-setup.exe` | Windows x86_64 — Full installer | `07bf253edafd63957109fc3eacc5d10de3b8aeb59d3e77c17ff935c96de39019` |
| `VRChat.Organizer_2.3.0_x64_en-US.msi` | Windows x86_64 — Full installer (MSI) | `bc8ce17818d50ed9a86e13811b608bb53192088819b09748ff347de8c2bedcbe` |
| `VRChatOrganizer-2.3.0-windows-x86_64-cli.exe` | Windows x86_64 — command-line tool | `3d323cee0c97fe2b1b00ab3e11a6188621fd10364d28b592e615ebdb67451a46` |

On Windows, most people want the installer or a portable exe; the
`-cli.exe` is the command-line organizer only.

Verify downloads with:

```bash
sha256sum -c SHA256SUMS
```
