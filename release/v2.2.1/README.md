# VRChat Organizer 2.2.1

Bug-fix release.

- **Fixed drawing person boxes:** dragging on the photo dragged the image
  itself, so no box was drawn or saved. The box now follows the pointer while
  you draw, and existing boxes can be moved and resized in draw mode too.

See the [2.2.0 notes](https://github.com/Seventh-Void/VRChat-Screenshot-Organizer/releases/tag/v2.2.0)
for everything new in 2.2.

## Release artifacts

| File | Platform | SHA-256 |
|---|---|---|
| `VRChatOrganizer-2.2.1-x86_64.AppImage` | Linux x86_64 GUI | `e6006984cf0d96da19bbaa5d68d6d5d33fe7b0284066d514f7c7309a699fb8d5` |
| `VRChatOrganizer-2.2.1-windows-x86_64-portable.exe` | Windows x86_64 GUI, portable (no install) | `f7e606bbd37a427828efae85114c96743ac4c4aa207dfeb25f5309e3c895aecf` |
| `VRChat.Organizer_2.2.1_x64-setup.exe` | Windows x86_64 GUI installer | `ed004c63225df4e5b078d9465c72d9e4df2126e75e3fc3d2b86c463fb80a08e3` |
| `VRChat.Organizer_2.2.1_x64_en-US.msi` | Windows x86_64 GUI installer (MSI) | `867694cfd6b9aebfe7712e5961bd2a0e2b3be4f4ec6a86dfe8cefaf65f360dd8` |
| `VRChatOrganizer-2.2.1-windows-x86_64-cli.exe` | Windows x86_64 command-line tool | `206c195d30f67499420deed5696db5257700be95bf05df8e774f3cc4ae6e9e7f` |

On Windows, most people want the installer or the portable exe; the
`-cli.exe` is the command-line organizer only.

Verify downloads with:

```bash
sha256sum -c SHA256SUMS
```
