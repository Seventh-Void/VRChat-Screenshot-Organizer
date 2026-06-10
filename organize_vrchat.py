#!/usr/bin/env python3
"""
VRChat Screenshot Organizer
Organizes VRChat screenshots by world based on EXIF metadata.
Maintains year/month folder structure.
"""

import json
import os
import shutil
import re
import sys
import subprocess
from datetime import datetime # Added for template date variables
import time
import threading
from pathlib import Path
import logging
from typing import Optional, Dict

# Auto-install dependencies
def install_dependencies():
    """Automatically install required dependencies."""
    required_packages = {
        'PIL': 'Pillow'
    }
    
    missing_packages = []
    
    for import_name, pip_name in required_packages.items():
        try:
            __import__(import_name)
        except ImportError:
            missing_packages.append(pip_name)
    
    if missing_packages:
        print("📦 Installing required packages...\n")
        for package in missing_packages:
            print(f"   Installing {package}...")
            try:
                subprocess.check_call([
                    sys.executable, "-m", "pip", "install", package
                ])
                print(f"   ✓ {package} installed successfully\n")
            except subprocess.CalledProcessError as e:
                print(f"   ✗ Failed to install {package}")
                print(f"   Error: {e}")
                print(f"\n   Try installing manually: pip3 install {package}")
                sys.exit(1)
        print("✓ All dependencies installed!\n")

try:
    from PIL import Image
    from PIL.ExifTags import TAGS
except ImportError:
    install_dependencies()
    from PIL import Image
    from PIL.ExifTags import TAGS

logger = logging.getLogger(__name__)

class VRChatOrganizer:
    def __init__(self, base_path: str):
        """Initialize the organizer with the base VRChat pictures path."""
        self.base_path = Path(base_path)
        self.image_extensions = {'.png', '.jpg', '.jpeg'}
        self.watch_mode = False
        self._seen_files = set()
        self._retry_counts = {} # Track attempts per file in watch mode
        self._last_month_folder = None
        self._stop_event = threading.Event()
        self.stats = {
            'processed': 0,
            'organized': 0,
            'no_metadata': 0,
            'errors': 0,
            'total': 0
        }

    def _parse_json_metadata(self, value) -> Optional[Dict]:
        """Helper to parse JSON from metadata values (str or bytes)."""
        if not isinstance(value, (str, bytes)):
            return None
        try:
            value_str = value.decode('utf-8', errors='ignore') if isinstance(value, bytes) else str(value)
            if value_str.startswith('{'):
                return json.loads(value_str)
        except (json.JSONDecodeError, UnicodeDecodeError):
            pass
        return None

    def _sanitize_name(self, name: str) -> str:
        """Sanitize world name for use as folder name."""
        invalid_chars = '<>:"|?*/\\'
        for char in invalid_chars:
            name = name.replace(char, '_')
        while '__' in name:
            name = name.replace('__', '_')
        return name.strip('. ')

    def _get_date_from_filename(self, filename: str) -> Optional[datetime]:
        """Try to extract date from standard VRChat filename: VRChat_YYYY-MM-DD_..."""
        match = re.search(r'VRChat_(\d{4})-(\d{2})-(\d{2})', filename)
        if match:
            try:
                return datetime(int(match.group(1)), int(match.group(2)), int(match.group(3)))
            except ValueError:
                pass
        return None

    def _apply_template(self, template: str, world: str, date: datetime, width: int, height: int) -> str:
        """Apply template variables and sanitize path components."""
        # Map of variables
        vars = {
            "{world}": self._sanitize_name(world),
            "{year}": date.strftime("%Y"),
            "{month}": date.strftime("%m"),
            "{day}": date.strftime("%d"),
            "{width}": str(width),
            "{height}": str(height)
        }
        
        result = template
        for placeholder, value in vars.items():
            result = result.replace(placeholder, value)
        
        return result

    def _get_image_data(self, image_path: Path) -> Dict:
        """Extract all relevant metadata in a single pass."""
        data = {'dimensions': None, 'world_name': None, 'software': None, 'width': 0, 'height': 0}
        try:
            with Image.open(image_path) as image:
                data['dimensions'] = image.size
                data['width'], data['height'] = image.size
                if hasattr(image, 'info') and image.info:
                    for key in ['Software', 'software', 'CreatorTool', 'creator_tool']:
                        if key in image.info:
                            val = image.info[key]
                            data['software'] = val.decode('utf-8', errors='ignore') if isinstance(val, bytes) else str(val)
                            break
                    for key in ['Description', 'Comment', 'comment', 'description']:
                        vrcx_data = self._parse_json_metadata(image.info.get(key))
                        if vrcx_data and 'world' in vrcx_data:
                            data['world_name'] = self._sanitize_name(vrcx_data['world'].get('name', ''))
                            break
                
                exif_data = image.getexif()
                if exif_data:
                    if not data['software'] and 305 in exif_data:
                        val = exif_data[305]
                        data['software'] = val.decode('utf-8', errors='ignore') if isinstance(val, bytes) else str(val)
                    if not data['world_name']:
                        vrcx_data = self._parse_json_metadata(exif_data.get(270))
                        if not vrcx_data and hasattr(exif_data, 'get_ifd'):
                            try:
                                ifd = exif_data.get_ifd(0)
                                vrcx_data = self._parse_json_metadata(ifd.get(270))
                            except Exception: pass
                        if vrcx_data and 'world' in vrcx_data:
                            data['world_name'] = self._sanitize_name(vrcx_data['world'].get('name', ''))
        except (IOError, PermissionError):
            pass # File likely locked by another process
        except Exception as e:
            logger.error(f"Error reading metadata from {image_path}: {e}")
            self.stats['errors'] += 1
        return data

    def extract_vrcx_metadata(self, image_path: Path) -> Optional[Dict]:
        """Legacy method maintained for compatibility."""
        try:
            with Image.open(image_path) as image:
                if hasattr(image, 'info') and image.info:
                    for key in ['Description', 'Comment', 'comment', 'description']:
                        vrcx_data = self._parse_json_metadata(image.info.get(key))
                        if vrcx_data and 'world' in vrcx_data: return vrcx_data
                exif_data = image.getexif()
                if exif_data:
                    vrcx_data = self._parse_json_metadata(exif_data.get(270))
                    if vrcx_data and 'world' in vrcx_data: return vrcx_data
                    if hasattr(exif_data, 'get_ifd'):
                        try:
                            ifd = exif_data.get_ifd(0)
                            vrcx_data = self._parse_json_metadata(ifd.get(270))
                            if vrcx_data and 'world' in vrcx_data: return vrcx_data
                        except Exception: pass
            return None
        except Exception: return None

    def get_world_name(self, image_path: Path) -> Optional[str]:
        """Legacy method maintained for compatibility."""
        return self._get_image_data(image_path).get('world_name')

    def get_image_dimensions(self, image_path: Path) -> Optional[tuple]:
        """Legacy method maintained for compatibility."""
        return self._get_image_data(image_path).get('dimensions')

    def _process_folder(self, folder: Path, dry_run: bool = False, template: str = "{world}") -> bool:
        """Unified logic to process images in a folder."""
        try:
            image_files = [
                f for f in folder.iterdir()
                if f.is_file() and f.suffix.lower() in self.image_extensions
            ]
        except Exception as e:
            if not self.watch_mode: logger.error(f"Error accessing folder {folder}: {e}")
            return False

        if self.watch_mode:
            valid_files = []
            for f in image_files:
                res = f.resolve()
                if res in self._seen_files: continue
                if self._retry_counts.get(res, 0) >= 10:
                    self._seen_files.add(res)
                    self.stats['no_metadata'] += 1
                    continue
                valid_files.append(f)
            image_files = valid_files

        if not image_files: return False

        # Update total count for progress tracking
        self.stats['total'] += len(image_files)

        if not self.watch_mode:
            logger.info(f"Processing folder: {folder.name}")
        
        for image_file in image_files:
            if self.watch_mode and self._stop_event.is_set():
                break
            
            self.stats['processed'] += 1
            resolved_path = image_file.resolve()
            img_data = self._get_image_data(image_file)

            target_sub = None
            if img_data['dimensions'] == (2048, 1440):
                target_sub = "Prints"
            elif img_data['world_name']:
                # Start with the template, or default to "{world}" if template is empty
                current_template = template if template else "{world}"

                # Get date components from file modification time
                file_date = self._get_date_from_filename(image_file.name)
                if not file_date:
                    try:
                        file_date = datetime.fromtimestamp(image_file.stat().st_mtime)
                    except Exception:
                        file_date = datetime.now()

                target_sub = self._apply_template(
                    current_template, 
                    img_data['world_name'],
                    file_date,
                    img_data.get('width', 0),
                    img_data.get('height', 0)
                )

            if not target_sub:
                if self.watch_mode:
                    self._retry_counts[resolved_path] = self._retry_counts.get(resolved_path, 0) + 1
                else:
                    self.stats['no_metadata'] += 1
                continue

            dest_folder = folder / target_sub
            if not dry_run: dest_folder.mkdir(parents=True, exist_ok=True)

            dest_path = dest_folder / image_file.name
            counter = 1
            base_stem = image_file.stem
            while dest_path.exists():
                new_name = f"{base_stem}_{counter}{image_file.suffix}"
                dest_path = dest_folder / new_name
                counter += 1

            try:
                if dry_run:
                    logger.info(f"Dry run: would move {image_file.name} -> {target_sub}/")
                else:
                    shutil.move(str(image_file), str(dest_path))
                    logger.info(f"Moved {image_file.name} -> {target_sub}/")
                self.stats['organized'] += 1
                if self.watch_mode: self._seen_files.add(resolved_path)
            except Exception as e:
                logger.error(f"Failed to move {image_file.name}: {e}")
                self.stats['errors'] += 1
                if self.watch_mode: self._seen_files.add(resolved_path)
        return True

    def organize_month_folder(self, month_folder: Path, dry_run: bool = False) -> bool:
        """Organize month folder using unified logic."""
        return self._process_folder(month_folder, dry_run=dry_run, template="{world}")

    def organize_single_folder(self, folder: Path, dry_run: bool = False, template: str = "{world}") -> bool:
        """Organize single folder using unified logic."""
        return self._process_folder(folder, dry_run, template)

    def run(
        self,
        single_folder: Optional[Path] = None,
        dry_run: bool = False,
        scan_all_months: bool = False, # New parameter
        watch: bool = False,
        interval: int = 5,
        template: str = "{world}"
    ) -> None:
        """Run the organization process."""
        # Reset stats for a fresh run
        self.stats['processed'] = 0
        self.stats['organized'] = 0
        self.stats['no_metadata'] = 0
        self.stats['errors'] = 0
        self.stats['total'] = 0

        self.watch_mode = watch
        if watch:
            self._seen_files = set()
            self._retry_counts = {}
            self._last_month_folder = None
            self._stop_event.clear()

        def run_once() -> bool:
            # If single_folder is specified, organize just that folder
            if single_folder:
                if not single_folder.exists():
                    logger.error(f"Path does not exist: {single_folder}")
                    return False
                return self._process_folder(single_folder, dry_run=dry_run, template=template)
            else:
                if not self.base_path.exists():
                    logger.error(f"Path does not exist: {self.base_path}")
                    return False

                # Find all month folders (YYYY-MM pattern)
                month_pattern = re.compile(r'^\d{4}-\d{2}$')
                month_folders = sorted([
                    d for d in self.base_path.iterdir()
                    if d.is_dir() and month_pattern.match(d.name)
                ])
                if not month_folders:
                    logger.warning(f"No month folders (YYYY-MM format) found in {self.base_path}")
                    return False

                any_changed = False
                if scan_all_months:
                    logger.info(f"Starting scan of all {len(month_folders)} month folders...")
                    for month_folder in month_folders:
                        if self._stop_event.is_set():
                            logger.info('Stop requested; halting remaining month folders')
                            break
                        if self._process_folder(month_folder, dry_run=dry_run, template=template):
                            any_changed = True
                else:
                    # Pick the most recent folder (last one in sorted list)
                    target_folder = month_folders[-1]
                    if not self.watch_mode or self._last_month_folder != target_folder:
                        logger.info(f"Automatically targeting most recent month: {target_folder.name}")
                        self._last_month_folder = target_folder
                    if self._process_folder(target_folder, dry_run=dry_run, template=template):
                        any_changed = True
                return any_changed

        if watch:
            logger.info(f"Watch mode enabled (interval: {interval}s)")
            try:
                while not self._stop_event.is_set():
                    run_once()
                    for _ in range(interval):
                        if self._stop_event.is_set(): break
                        time.sleep(1)
            except KeyboardInterrupt:
                logger.info("Watch mode interrupted by user")
        else:
            changed = run_once()
            if changed:
                logger.info("\n" + "="*50)
                logger.info("Organization Summary:")
                logger.info(f"Total images processed: {self.stats['processed']}")
                logger.info(f"Images organized: {self.stats['organized']}")
                logger.info(f"Images without metadata: {self.stats['no_metadata']}")
                logger.info(f"Errors: {self.stats['errors']}")
                logger.info("="*50)

    def stop(self) -> None:
        """Request that a running watch loop stop."""
        logger.info("Stop requested for organizer")
        self._stop_event.set()


def main():
    """Main entry point."""
    import argparse

    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s - %(levelname)s - %(message)s'
    )
    
    parser = argparse.ArgumentParser(
        description='Organize VRChat screenshots by world'
    )
    parser.add_argument(
        'path',
        nargs='?',
        default=None,
        help='Path to VRChat pictures directory or specific folder to organize'
    )
    parser.add_argument(
        '--dry-run',
        action='store_true',
        help='Show what would be done without making changes'
    )
    parser.add_argument(
        '--watch',
        action='store_true',
        help='Keep monitoring the folder and organize new screenshots automatically'
    )
    parser.add_argument(
        '--interval',
        type=int,
        default=5,
        help='Watch interval in seconds when --watch is enabled'
    )
    parser.add_argument(
        '--single-folder',
        action='store_true',
        help='Treat path as a single folder to organize (not as a root with YYYY-MM folders)'
    )
    parser.add_argument(
        '--scan-all-months',
        action='store_true',
        help='Scan all month folders instead of just the latest one'
    )
    parser.add_argument(
        '--template',
        type=str,
        default="{world}",
        help='Custom subfolder naming template (e.g., "{world}", "{year}-{month}/{world}"). Variables: {world}, {year}, {month}, {day}, {width}, {height}'
    )
    args = parser.parse_args()
    
    # Determine the path
    if args.path:
        base_path = args.path
    elif args.single_folder:
        print("Error: --single-folder requires a path argument")
        sys.exit(1)
    else:
        base_path = os.path.expanduser('~/Pictures/VRChat/VRChat')
    
    if args.dry_run:
        logger.info("DRY RUN MODE - No changes will be made")
    
    base_path = os.path.expanduser(base_path)
    
    organizer = VRChatOrganizer(base_path)
    organizer.run(
        single_folder=Path(base_path) if args.single_folder else None,
        scan_all_months=args.scan_all_months,
        dry_run=args.dry_run,
        watch=args.watch,
        interval=args.interval,
        template=args.template
    )


if __name__ == '__main__':
    main()
