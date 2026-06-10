# Frequently Asked Questions (FAQ)

## ⚠️ Safety & Backups (READ THIS!)

### Q: Is this tool safe? Will it delete my screenshots?
**A:** It doesn't delete files - it only **moves** them into subfolders. However, **this tool modifies your file system** and you should treat it with care:
- It **moves real files** on your hard drive
- While designed to be safe, **no tool is 100% guaranteed** to work perfectly
- **Always use `--dry-run` first** to see what will happen
- **Always backup before running**
- **Use at your own risk** - the author assumes no responsibility for data loss

### Q: Should I backup my screenshots?
**A:** **YES!** Before running this tool:
1. Back up your entire `~/Pictures/VRChat/VRChat` folder (or wherever your screenshots are)
2. Or at least backup the month folders you're organizing
3. You can restore from backup if anything goes wrong

### Q: What's the safest way to use this tool?
**A:** Follow these steps:
1. **Create a backup** of your VRChat pictures folder
2. **Run with `--dry-run`:**
   ```bash
   python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --dry-run
   ```
3. **Review the output carefully** - does it match what you expected?
4. **Only then run without `--dry-run`** if you're comfortable
5. If you don't wish to proceed, **don't run the command** - your files stay unchanged

### Q: Can I undo the organization?
**A:** The best way is to restore from your backup. Alternatively, you can manually move folders back to the root. That's why backups are important!

## Installation & Setup

### Q: Do I need VRCX? What is it?
**A:** Yes! [VRCX](https://github.com/vrcx-team/VRCX) is a VRChat companion app that embeds world metadata into your screenshots. Without it, the organizer won't be able to identify which world each screenshot is from.

### Q: How do I install VRCX?
**A:** 
1. Download from: https://github.com/vrcx-team/VRCX/releases
2. Run the installer
3. Launch VRCX and go to Settings
4. Enable "Screenshot Metadata" checkbox
5. Keep VRCX running in the background while taking screenshots

### Q: Can I use this tool without VRCX?
**A:** Unfortunately no. VRCX is required because it embeds the world name metadata into each screenshot. Without VRCX running and enabled, your screenshots won't have this metadata, so the organizer can't identify which world they're from.

### Q: Do I need to keep VRCX running in watch mode?
**A:** **YES!** If you're using watch mode (`--watch`), VRCX must be running in the background. Otherwise, new screenshots won't have metadata embedded, and watch mode won't be able to organize them.

### Q: What if I see "No world metadata found" errors?
**A:** This means VRCX metadata wasn't embedded. Check:
1. Is VRCX installed? Download from https://github.com/vrcx-team/VRCX
2. Is "Screenshot Metadata" enabled in VRCX Settings?
3. Was VRCX running when you took the screenshot?
4. Run `debug_metadata.py screenshot.png` to inspect the image

### Q: Do I need to install anything manually?
**A:** No! The script automatically installs Pillow on first run. You just need Python 3.7+.

### Q: How do I know if Python 3 is installed?
**A:** Run: `python3 --version`

If it shows version 3.7 or higher, you're good. If not, install Python 3.

### Q: Can I run this on Windows?
**A:** Yes! Python 3 works on Windows. The GUI (`gui_vrchat_organizer.py`) works best on Windows.

### Q: Does this work on macOS?
**A:** Yes! Both CLI and GUI work on macOS.

## Usage Questions

### Q: Where are my VRChat screenshots located?
**A:** By default: `~/Pictures/VRChat/VRChat`

To check, run:
```bash
ls ~/Pictures/VRChat/VRChat
```

Or use the GUI to browse to your folder.

### Q: What does `--dry-run` do?
**A:** It shows you exactly what would happen WITHOUT actually moving any files. Always test this first!

### Q: How do I use watch mode?
**A:** Run: `python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --watch`

This will organize new screenshots automatically every 30 seconds.

### Q: What if I'm not ready to organize everything yet?
**A:** Use `--dry-run` first, then decide. Or organize one month at a time using `--single-folder`.

## Metadata & Organization

### Q: Why are some screenshots not being organized?
**A:** Screenshots without world metadata won't be organized. They stay in the month folder.

Use `debug_metadata.py` to check if a specific image has metadata.

### Q: What about my "Prints" folder?
**A:** Images with 2048x1440 resolution are automatically moved to a "Prints" folder.

### Q: Can I organize screenshots by software (Lightroom, Photoshop, etc.)?
**A:** Yes! Use: `python3 organize_vrchat.py /path --software-filter "Adobe Photoshop"`

### Q: What if two screenshots have the same name?
**A:** The organizer automatically renames them: `screenshot_1.png`, `screenshot_2.png`, etc.

## Troubleshooting

### Q: I got an error "Permission denied"
**A:** Your screenshots folder might be read-only. Try:

```bash
chmod -R u+rwX ~/Pictures/VRChat/VRChat
```

### Q: The GUI isn't starting
**A:** Install tkinter:
- **Ubuntu/Debian**: `sudo apt install python3-tk`
- **Fedora**: `sudo dnf install python3-tkinter`
- **macOS**: `brew install python-tk`

### Q: The script is taking a long time
**A:** This is normal for the first run with many screenshots. Subsequent runs are faster.

### Q: How do I stop watch mode?
**A:** Press `Ctrl+C` in the terminal.

## Features & Behavior

### Q: Will this delete any of my screenshots?
**A:** No. It only moves files into subfolders. Your original screenshot data is always preserved.

### Q: Can I undo the organization?
**A:** Yes! Use a backup or reverse the folder structure. Or use `--dry-run` first to ensure you understand what will happen.

### Q: Does this work with cloud storage (Google Drive, OneDrive, etc.)?
**A:** It should work with locally mounted folders. Avoid running on actively syncing folders to prevent conflicts.

### Q: Can I organize a custom folder structure?
**A:** Yes! Use `--single-folder` flag for folders that don't have YYYY-MM subfolders.

## Technical

### Q: What metadata does it read?
**A:** 
- PNG image metadata chunks
- JPEG EXIF data (tag 270 - ImageDescription)
- VRCX-embedded world information

### Q: Is my data safe?
**A:** Yes! Always:
1. Test with `--dry-run` first
2. Keep backups
3. Check the logs for any issues

### Q: How can I help develop this?
**A:** See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines!

### Q: Where are the logs saved?
**A:** Logs are printed to console. To save them:
```bash
python3 organize_vrchat.py /path > log.txt 2>&1
```

### Q: Can I use this in a script or automation?
**A:** Yes! It's designed to be scriptable. Use it in cron jobs or other automation tools.

Example cron job:
```
0 3 * * * python3 /path/to/organize_vrchat.py --watch --interval 300
```

## Still Have Questions?

1. Check the [README.md](../README.md) for overview
2. Check [ADVANCED.md](ADVANCED.md) for advanced usage
3. Use `debug_metadata.py` to inspect your specific images
4. Open an issue on GitHub

We're here to help!
