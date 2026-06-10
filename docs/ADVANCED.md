# Advanced Usage Guide

## ⚠️ Important Safety Reminder

Before using any of the advanced features below:
- **Always backup your VRChat pictures folder first**
- **Always run with `--dry-run` first** to see what will happen
- **Review the output carefully** before proceeding without `--dry-run`
- **Only run without `--dry-run`** if you're comfortable with the changes shown
- **Use at your own risk** - this tool moves real files on your hard drive

If you don't wish to proceed after seeing the `--dry-run` output, simply don't run the command without the flag - your files will remain unchanged.

## Command-Line Arguments Reference

### Basic Usage

```bash
python3 organize_vrchat.py [PATH] [OPTIONS]
```

### Arguments

#### `PATH` (Optional)
- **Description**: Path to organize
- **Default**: `~/Pictures/VRChat/VRChat`
- **Examples**:
  - `/home/user/Pictures/VRChat/VRChat`
  - `./local_vrchat_folder`
  - `~/Pictures/VRChat/VRChat`

### Options

#### `--dry-run`
- **Description**: Show what would be done without making changes
- **Usage**: `python3 organize_vrchat.py /path --dry-run`
- **⚠️ CRITICAL - ALWAYS USE THIS FIRST:**
  - **No files are moved or changed** - it's completely safe
  - See exactly what will happen before any action
  - Review the output carefully before proceeding
  - If you don't like the results, **do not run without `--dry-run`**
  - Your files remain untouched until you explicitly run without `--dry-run`
- **Recommended Workflow:**
  1. Run with `--dry-run`
  2. Review output carefully
  3. Create a backup of your pictures folder
  4. Only then run without `--dry-run` if you're satisfied
- **Tips**: This is your safety net - use it every time!

#### `--watch`
- **Description**: Keep monitoring the folder and organize automatically
- **Usage**: `python3 organize_vrchat.py /path --watch`
- **⚠️ Important**: VRCX must be running in the background for watch mode to work!
  - New screenshots won't have metadata unless VRCX is running
  - Verify "Screenshot Metadata" is enabled in VRCX Settings
- **Tips**: 
  - Press Ctrl+C to stop
  - Useful for continuous organization during gameplay
  - Keep VRCX running while playing for automatic organization

#### `--interval SECONDS`
- **Description**: Watch interval in seconds (default: 30)
- **Usage**: `python3 organize_vrchat.py /path --watch --interval 60`
- **Valid values**: 1-3600 seconds
- **Tips**:
  - Smaller values = more frequent checks (more CPU usage)
  - Larger values = less frequent checks (files might wait longer)

#### `--scan-all-months`
- **Description**: Scan all YYYY-MM folders instead of just the latest one
- **Usage**: `python3 organize_vrchat.py /path --scan-all-months`
- **When to use**:
  - First-time setup to organize your entire history
  - After disabling/enabling metadata to catch missed files

#### `--single-folder`
- **Description**: Treat path as a single folder to organize
- **Usage**: `python3 organize_vrchat.py /path/to/folder --single-folder`
- **When to use**:
  - Organizing a folder without year/month subfolders
  - Organizing a specific month folder
  - Custom folder structures

#### `--template TEMPLATE_STRING`
- **Description**: Define a custom naming structure for the subfolders where screenshots will be moved.
- **Usage**: `python3 organize_vrchat.py /path --template "{world} ({width}x{height})"`
- **Available Variables**:
  - `{world}`: The name of the VRChat world (e.g., "Black Cat").
  - `{year}`: The year the screenshot was taken (e.g., "2023").
  - `{month}`: The month the screenshot was taken (e.g., "01" for January).
  - `{day}`: The day the screenshot was taken (e.g., "15").
  - `{width}`: The width of the screenshot in pixels (e.g., "1920").
  - `{height}`: The height of the screenshot in pixels (e.g., "1080").
- **Examples**:
  - `--template "{world}"` (Default behavior)
  - `--template "{year}-{month}/{world}"` (Creates "2023-01/Black Cat" structure)
  - `--template "{world} ({width}x{height})"` (Creates "Black Cat (1920x1080)" folders)
  - `--template "{world} - {day}-{month}-{year}"` (Creates "Black Cat - 15-01-2023" folders)

## Example Use Cases

### Use Case 1: Organize One Month

```bash
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat/2025-01 --single-folder --dry-run
# Review the output
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat/2025-01 --single-folder
```

### Use Case 2: Watch Mode During Gameplay

⚠️ **Requirements**: VRCX must be running in background!

```bash
# Make sure VRCX is running first!
# Then in a terminal, run in background
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --watch --interval 60 &

# Play VRChat - screenshots get organized automatically as they're taken
# VRCX embeds the metadata, organizer watches and sorts them
# To stop: press Ctrl+C in the terminal
```

### Use Case 3: Organize Only Edited Screenshots

```bash
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat \
  --software-filter "Adobe Photoshop" \
  --dry-run

python3 organize_vrchat.py ~/Pictures/VRChat/VRChat \
  --software-filter "Adobe Photoshop"
```

### Use Case 4: Test Before Full Organization

```bash
# First, test with dry-run
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --dry-run

# Review the output carefully
# If satisfied, run for real
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat
```

## Debugging

### Using debug_metadata.py

Inspect metadata in a specific image:

```bash
python3 debug_metadata.py ~/Pictures/VRChat/VRChat/2025-01/screenshot.png
```

Output shows:
- Image dimensions
- EXIF data
- PNG metadata
- Detected world name
- Software used to capture

### Checking Logs

The organizer writes logs to console. To save logs to file:

```bash
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat > organization.log 2>&1
```

## Performance Tips

1. **First run is slowest**: Initial organization might take a while
2. **Watch mode**: Scales well with interval of 30-60 seconds
3. **Large libraries**: Consider organizing by month (use `--single-folder` with month folders)

## Advanced Tips

### Organizing Remote Folders

```bash
# Via SSH mount (Linux/macOS)
mkdir -p ~/mnt/remote_vrchat
sshfs user@remote:/path/to/VRChat ~/mnt/remote_vrchat
python3 organize_vrchat.py ~/mnt/remote_vrchat

# Via SMB (Windows shares)
# Mount the share first, then organize
```

### Integration with Scripts

```bash
#!/bin/bash
# Daily organization script
0 3 * * * python3 /path/to/organize_vrchat.py --watch --interval 300
```

### Extracting Metadata Programmatically

See the source code in `organize_vrchat.py` for the `VRChatOrganizer` class API.

## Output Explanation

### Summary Statistics

```
===================================================
Organization Summary:
Total images processed: 42
Images organized: 40
Images without metadata: 2
Errors: 0
===================================================
```

- **Processed**: Total images examined
- **Organized**: Successfully moved to world folders
- **Without metadata**: Images that couldn't be identified
- **Errors**: Failed operations (usually permission issues)

## Troubleshooting Advanced Issues

### Issue: "Permission denied" errors

```bash
# Check permissions
ls -l ~/Pictures/VRChat/VRChat

# Fix permissions if needed
chmod -R u+rwX ~/Pictures/VRChat/VRChat
```

### Issue: Metadata not being read

1. Run `debug_metadata.py` on a problematic image
2. Check if VRCX embedded the metadata
3. Use `--software-filter` to narrow down which images to process

### Issue: Watch mode not detecting new files

- Increase the interval: `--interval 60` (slower but more reliable)
- Check file system permissions
- Ensure the path is correct

## Getting Help

For more complex issues:
1. Run with `--dry-run` and paste the output
2. Run `debug_metadata.py` on an example image
3. Open an issue on GitHub with:
   - Your command
   - The output/error
   - Your OS and Python version
   - (Optional) A sample image for metadata inspection
