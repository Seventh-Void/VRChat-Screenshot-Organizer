# VRChat Organizer 2.3.1

Important fix for the Linux app. Please update.

- **Fixed a memory runaway that crashed the app.** After turning on "Scan all
  month folders" with a large library, the library page grew by hundreds of MB
  per second (20 GB+) until the app died. The cause was a blur effect on the
  favourite button of every world card, which WebKitGTK handles badly. With
  every month loaded the app now stays at about 400 MB.

Nothing else changed. Organizing, folders and undo work exactly as in 2.3.0.

## Release artifacts

@TABLE@

On Windows, most people want the installer or a portable exe; the
`-cli.exe` is the command-line organizer only.

Verify downloads with:

```bash
sha256sum -c SHA256SUMS
```
