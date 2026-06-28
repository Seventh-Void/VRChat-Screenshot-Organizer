# TODO

## Step 1 — Unify metadata extraction + sanitization
- [x] Refactor `organize_vrchat.py` metadata parsing into reusable helper(s).
- [x] Update `preview_vrchat.py` to use the same helper(s).
- [x] Ensure invalid-character sanitization matches in both paths.

## Step 2 — Remove runtime dependency auto-install
- [x] Delete/replace `install_dependencies()` auto-pip-install blocks in `organize_vrchat.py` and `preview_vrchat.py`.
- [x] Replace with a clear import error message if Pillow is missing.

## Step 3 — Watch-mode correctness
- [ ] Adjust watch retry/no-metadata policy so files aren’t permanently misclassified.

## Step 4 — Thread-safety for GUI stats
- [ ] Add a `threading.Lock` in `VRChatOrganizer` to protect `stats` updates.
- [ ] Update GUI (`gui_vrchat_organizer.py`) to read stats under the lock.

## Step 5 — Performance micro-optimizations
- [ ] Reduce unnecessary sorting in preview.
- [ ] Add early-exit once world name is found in metadata extraction.

## Step 6 — Run smoke tests
- [ ] `python organize_vrchat.py --dry-run --scan-all-months`
- [ ] `python preview_vrchat.py "<base_path>"`
- [ ] Start GUI and run “Run Once”

