# Bug Fix: Watch mode organizes files into wrong location

## Steps

- [x] Analyze the bug and gather information
- [x] Get user approval on the plan
- [ ] **Step 1**: Add `organize_single_file` function in `organizer-core/src/lib.rs` that:
  - Checks if the file's parent directory is a YYYY-MM folder
  - If so, uses that parent as the base for destination
  - If not, extracts date from filename and creates the YYYY-MM path
  - Computes destination as `YYYY-MM/worldname/filename`
- [ ] **Step 2**: Update the watch loop in `src-tauri/src/lib.rs` to call `organize_single_file` instead of `organize_path` for per-file events and polling
- [ ] **Step 3**: Build the project to verify compilation

