# VRChat Screenshot Organizer — project notes for Claude

Read `gotcha.md` before non-trivial change; add to it when hit trap.

## What the app does
Move each VRChat screenshot into world-named folder **inside VRChat month folder it belongs to**:
`<base>/2026-10/VRChat_….png` → `<base>/2026-10/<World Name>/VRChat_….png`.
- World name from image-embedded metadata (PNG text chunk JSON / JPEG EXIF).
- Base-root photos routed by filename date into `<base>/YYYY-MM/<World>/`.
- 2048×1440 prints → `<base>/YYYY-MM/Prints/`. No world metadata → photo stays put.
- Every move undoable; watcher organizes live while VRChat runs.
- GUI = library (worlds, timeline, people, collections, storage, activity) + manual person tagging (draw box, pick name from photo's player metadata). Avatar AI removed 2026-10-07 at user request (unreliable; avatars change) — don't re-add without asking.

## Hard rules
- **Folder layout/routing/undo semantics never change.**
- **Fail-safe:** image can't be organised for any reason → leave exactly where/as was. No overwrite, no half-move, every move undoable (rollback if undo log write fails). Images only moved, never rewritten.
- **UI design changes:** send user before/after screenshots, ask how they like it before continuing visual work. Keep current look unless redesign asked. Use `:root` design tokens (font sizes, radii, accent tints, z-index) — no new hard-coded colours/sizes.
- **User-chosen values (2026-10-07):** secondary text `--muted: #827796`, `--faint: #706785` (dimmer than WCAG-ideal by request, still 4.6:1 on canvas); keyboard focus ring `1.5px solid var(--accent-a72)`, offset 2px. No change without asking.
- Screenshot harness for check-ins (recreate if scratchpad gone): Playwright + `/usr/bin/chromium` on `frontend/index.html` with stubbed `window.__TAURI__` fixture; shoot 1180×760 and 860×620.
- README pictures (`docs/screenshots/{library,world,viewer,people}.png`): same harness, demo data only (fake worlds/players, generated SVG scenery with explicit width/height), 1440×900 @1.5x. Never use user's real screenshots — they show other players.
- **Git:** no commit, branch, stash, reset unless user asks. Working tree often holds large uncommitted work.

## Commands (run in `rust-rewrite/`)
- `npm ci && npm test` (frontend tests; viewer test needs Chromium, path via `CHROMIUM`, default `/usr/bin/chromium`)
- `cargo tauri dev` — run GUI. `./build-appimage.sh`, `./build-windows.sh` — release builds.
- Never run organize/tag code against user's real `~/Pictures/VRChat` while testing — use temp copies.