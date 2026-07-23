# Bug Fix: Watch mode organizes files into wrong location

## Steps

- [x] Analyze the bug and gather information
- [x] Get user approval on the plan
- [x] **Step 1**: Add `organize_single_file` function in `organizer-core/src/lib.rs` that:
  - Checks if the file's parent directory is a YYYY-MM folder
  - If so, uses that parent as the base for destination
  - If not, extracts date from filename and creates the YYYY-MM path
  - Computes destination as `YYYY-MM/worldname/filename`
- [x] **Step 2**: Update the watch loop in `src-tauri/src/lib.rs` to call `organize_single_file` instead of `organize_path` for per-file events and polling
- [x] **Step 3**: Build the project to verify compilation — build succeeds, all 2 tests pass
- [x] **Step 4**: Delete old AppImages and build fresh AppImage — done

