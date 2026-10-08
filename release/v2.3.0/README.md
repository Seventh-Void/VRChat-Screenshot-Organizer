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

@TABLE@

On Windows, most people want the installer or a portable exe; the
`-cli.exe` is the command-line organizer only.

Verify downloads with:

```bash
sha256sum -c SHA256SUMS
```
