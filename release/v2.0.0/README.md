# VRChat Organizer 2.0.0 Release Assets

This directory contains the v2.0.0 assets to upload to the GitHub release:

- `VRChatOrganizer-2.0.0-x86_64.AppImage` — Linux x86_64 Tauri GUI AppImage
- `VRChatOrganizer-2.0.0-windows-x86_64.exe` — Windows x86_64 CLI executable
- `SHA256SUMS` — SHA-256 checksums for the generated binaries
- `README.md` — this release asset guide

The native Windows Tauri GUI portable executable, NSIS installer, and MSI are
built separately by the Windows GitHub Actions runner. Upload those workflow
outputs to the same release when the workflow completes.
