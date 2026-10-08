# VRChat Organizer 2.2.1

Bug-fix release.

- **Fixed drawing person boxes:** dragging on the photo dragged the image
  itself, so no box was drawn or saved. The box now follows the pointer while
  you draw, and existing boxes can be moved and resized in draw mode too.

See the [2.2.0 notes](https://github.com/Seventh-Void/VRChat-Screenshot-Organizer/releases/tag/v2.2.0)
for everything new in 2.2.

## Release artifacts

@TABLE@

On Windows, most people want the installer or the portable exe; the
`-cli.exe` is the command-line organizer only.

Verify downloads with:

```bash
sha256sum -c SHA256SUMS
```
