#!/usr/bin/env python3
"""VRChat Screenshot Organizer.

Organizes VRChat screenshots by world based on EXIF metadata.
Maintains year/month folder structure.

Notes:
- Runtime dependency auto-install has been removed.
- Metadata extraction is centralized in `vrchat_metadata.py`.
"""

import os
import re
import shutil
import sys
import threading
import time
from datetime import datetime
from pathlib import Path
import logging
from typing import Optional, Dict

try:
    # Ensure Pillow exists (no runtime installation)
    from PIL import Image  # noqa: F401
except ImportError as e:
    raise SystemExit("Missing dependency 'Pillow'. Install with: pip install Pillow") from e

from vrchat_metadata import ImageMeta, extract_image_meta, extract_date_from_filename, sanitize_name

logger = logging.getLogger(__name__)


class VRChatOrganizer:
    def __init__(self, base_path: str):
        """Initialize the organizer with the base VRChat pictures path."""
        self.base_path = Path(base_path)
        self.image_extensions = {".png", ".jpg", ".jpeg"}

        self.watch_mode = False
        self._seen_files = set()
        self._retry_counts = {}  # Track attempts per file in watch mode
        self._last_month_folder = None
        self._stop_event = threading.Event()

        self.stats = {
            "processed": 0,
            "organized": 0,
            "no_metadata": 0,
            "errors": 0,
            "total": 0,
        }

    def _sanitize_name(self, name: str) -> str:
        """Backwards-compatible alias."""
        return sanitize_name(name)

    def _get_date_from_filename(self, filename: str) -> Optional[datetime]:
        """Backwards-compatible alias."""
        return extract_date_from_filename(filename)

    def _apply_template(self, template: str, world: str, date: datetime, width: int, height: int) -> str:
        """Apply template variables and sanitize path components."""
        vars_map = {
            "{world}": self._sanitize_name(world),
            "{year}": date.strftime("%Y"),
            "{month}": date.strftime("%m"),
            "{day}": date.strftime("%d"),
            "{width}": str(width),
            "{height}": str(height),
        }

        result = template
        for placeholder, value in vars_map.items():
            result = result.replace(placeholder, value)
        return result

    def _get_image_data(self, image_path: Path) -> Dict:
        """Extract all relevant metadata in a single pass.

        Uses shared logic so organizer + preview behave consistently.
        """
        try:
            meta: ImageMeta = extract_image_meta(image_path)
            return {
                "dimensions": meta.dimensions,
                "world_name": meta.world_name,
                "software": meta.software,
                "width": meta.width,
                "height": meta.height,
            }
        except Exception as e:
            logger.error(f"Error reading metadata from {image_path}: {e}")
            self.stats["errors"] += 1
            return {
                "dimensions": None,
                "world_name": None,
                "software": None,
                "width": 0,
                "height": 0,
            }

    def extract_vrcx_metadata(self, image_path: Path) -> Optional[Dict]:
        """Legacy method maintained for compatibility.

        Shared helper returns only derived fields; this reconstructs a minimal shape.
        """
        meta = extract_image_meta(image_path)
        if meta.world_name:
            return {"world": {"name": meta.world_name}}
        return None

    def get_world_name(self, image_path: Path) -> Optional[str]:
        """Return extracted world name."""
        return self._get_image_data(image_path).get("world_name")

    def get_image_dimensions(self, image_path: Path) -> Optional[tuple]:
        """Return extracted dimensions."""
        return self._get_image_data(image_path).get("dimensions")

    def _process_folder(self, folder: Path, dry_run: bool = False, template: str = "{world}") -> bool:
        """Unified logic to process images in a folder."""
        try:
            image_files = [
                f
                for f in folder.iterdir()
                if f.is_file() and f.suffix.lower() in self.image_extensions
            ]
        except Exception as e:
            if not self.watch_mode:
                logger.error(f"Error accessing folder {folder}: {e}")
            return False

        if self.watch_mode:
            valid_files = []
            for f in image_files:
                res = f.resolve()
                if res in self._seen_files:
                    continue
                # Leave watch retry policy as-is for now (next TODO step).
                if self._retry_counts.get(res, 0) >= 10:
                    self._seen_files.add(res)
                    self.stats["no_metadata"] += 1
                    continue
                valid_files.append(f)
            image_files = valid_files

        if not image_files:
            return False

        self.stats["total"] += len(image_files)

        if not self.watch_mode:
            logger.info(f"Processing folder: {folder.name}")

        for image_file in image_files:
            if self.watch_mode and self._stop_event.is_set():
                break

            self.stats["processed"] += 1
            resolved_path = image_file.resolve()

            img_data = self._get_image_data(image_file)
            target_sub = None

            if img_data["dimensions"] == (2048, 1440):
                target_sub = "Prints"
            elif img_data["world_name"]:
                current_template = template if template else "{world}"

                file_date = self._get_date_from_filename(image_file.name)
                if not file_date:
                    try:
                        file_date = datetime.fromtimestamp(image_file.stat().st_mtime)
                    except Exception:
                        file_date = datetime.now()

                target_sub = self._apply_template(
                    current_template,
                    img_data["world_name"],
                    file_date,
                    img_data.get("width", 0),
                    img_data.get("height", 0),
                )

            if not target_sub:
                if self.watch_mode:
                    self._retry_counts[resolved_path] = self._retry_counts.get(resolved_path, 0) + 1
                else:
                    self.stats["no_metadata"] += 1
                continue

            dest_folder = folder / target_sub
            if not dry_run:
                dest_folder.mkdir(parents=True, exist_ok=True)

            dest_path = dest_folder / image_file.name
            counter = 1
            base_stem = image_file.stem
            while dest_path.exists():
                dest_path = dest_folder / f"{base_stem}_{counter}{image_file.suffix}"
                counter += 1

            try:
                if dry_run:
                    logger.info(f"Dry run: would move {image_file.name} -> {target_sub}/")
                else:
                    shutil.move(str(image_file), str(dest_path))
                    logger.info(f"Moved {image_file.name} -> {target_sub}/")

                self.stats["organized"] += 1
                if self.watch_mode:
                    self._seen_files.add(resolved_path)
            except Exception as e:
                logger.error(f"Failed to move {image_file.name}: {e}")
                self.stats["errors"] += 1
                if self.watch_mode:
                    self._seen_files.add(resolved_path)

        return True

    def organize_month_folder(self, month_folder: Path, dry_run: bool = False) -> bool:
        """Organize month folder using unified logic."""
        return self._process_folder(month_folder, dry_run=dry_run, template="{world}")

    def organize_single_folder(self, folder: Path, dry_run: bool = False, template: str = "{world}") -> bool:
        """Organize single folder using unified logic."""
        return self._process_folder(folder, dry_run=dry_run, template=template)

    def run(
        self,
        single_folder: Optional[Path] = None,
        dry_run: bool = False,
        scan_all_months: bool = False,
        watch: bool = False,
        interval: int = 5,
        template: str = "{world}",
    ) -> None:
        """Run the organization process."""
        # Reset stats for a fresh run
        self.stats["processed"] = 0
        self.stats["organized"] = 0
        self.stats["no_metadata"] = 0
        self.stats["errors"] = 0
        self.stats["total"] = 0

        self.watch_mode = watch
        if watch:
            self._seen_files = set()
            self._retry_counts = {}
            self._last_month_folder = None
            self._stop_event.clear()

        def run_once() -> bool:
            if single_folder:
                if not single_folder.exists():
                    logger.error(f"Path does not exist: {single_folder}")
                    return False
                return self._process_folder(single_folder, dry_run=dry_run, template=template)

            if not self.base_path.exists():
                logger.error(f"Path does not exist: {self.base_path}")
                return False

            month_pattern = re.compile(r"^\d{4}-\d{2}$")
            month_folders = sorted(
                [d for d in self.base_path.iterdir() if d.is_dir() and month_pattern.match(d.name)]
            )
            if not month_folders:
                logger.warning(f"No month folders (YYYY-MM format) found in {self.base_path}")
                return False

            any_changed = False
            if scan_all_months:
                logger.info(f"Starting scan of all {len(month_folders)} month folders...")
                for month_folder in month_folders:
                    if self._stop_event.is_set():
                        logger.info("Stop requested; halting remaining month folders")
                        break
                    if self._process_folder(month_folder, dry_run=dry_run, template=template):
                        any_changed = True
            else:
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
                        if self._stop_event.is_set():
                            break
                        time.sleep(1)
            except KeyboardInterrupt:
                logger.info("Watch mode interrupted by user")
        else:
            changed = run_once()
            if changed:
                logger.info("\n" + "=" * 50)
                logger.info("Organization Summary:")
                logger.info(f"Total images processed: {self.stats['processed']}")
                logger.info(f"Images organized: {self.stats['organized']}")
                logger.info(f"Images without metadata: {self.stats['no_metadata']}")
                logger.info(f"Errors: {self.stats['errors']}")
                logger.info("=" * 50)

    def stop(self) -> None:
        """Request that a running watch loop stop."""
        logger.info("Stop requested for organizer")
        self._stop_event.set()


def main():
    """Main entry point."""
    import argparse

    logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s")

    parser = argparse.ArgumentParser(description="Organize VRChat screenshots by world")
    parser.add_argument(
        "path",
        nargs="?",
        default=None,
        help="Path to VRChat pictures directory or specific folder to organize",
    )
    parser.add_argument("--dry-run", action="store_true", help="Show what would be done without making changes")
    parser.add_argument("--watch", action="store_true", help="Keep monitoring the folder and organize new screenshots")
    parser.add_argument(
        "--interval",
        type=int,
        default=5,
        help="Watch interval in seconds when --watch is enabled",
    )
    parser.add_argument(
        "--single-folder",
        action="store_true",
        help="Treat path as a single folder to organize (not as a root with YYYY-MM folders)",
    )
    parser.add_argument("--scan-all-months", action="store_true", help="Scan all month folders")
    parser.add_argument(
        "--template",
        type=str,
        default="{world}",
        help=(
            'Custom subfolder naming template (e.g., "{world}", "{year}-{month}/{world}"). '
            "Variables: {world}, {year}, {month}, {day}, {width}, {height}"
        ),
    )

    args = parser.parse_args()

    if args.path:
        base_path = args.path
    elif args.single_folder:
        print("Error: --single-folder requires a path argument")
        sys.exit(1)
    else:
        base_path = os.path.expanduser("~/Pictures/VRChat/VRChat")

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
        template=args.template,
    )


if __name__ == "__main__":
    main()

