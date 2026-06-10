#!/usr/bin/env python3
"""
VRChat Screenshot Organizer
Organizes VRChat screenshots by world based on EXIF metadata.
Maintains year/month folder structure.
"""

import json
import os
import shutil
import sys
import subprocess
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

# Install dependencies before importing
install_dependencies()

# Now import the packages
from PIL import Image
from PIL.ExifTags import TAGS

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)

class VRChatOrganizer:
    def __init__(self, base_path: str):
        """Initialize the organizer with the base VRChat pictures path."""
        self.base_path = Path(base_path)
        self.image_extensions = {'.png', '.jpg', '.jpeg'}
        self.watch_mode = False
        self._seen_files = set()
        self._stop_event = threading.Event()
        self.stats = {
            'processed': 0,
            'organized': 0,
            'no_metadata': 0,
            'errors': 0
        }
    
    def extract_vrcx_metadata(self, image_path: Path) -> Optional[Dict]:
        """
        Extract VRCX metadata from image (PNG info or JPEG EXIF).
        
        Args:
            image_path: Path to the image file
            
        Returns:
            Dictionary containing VRCX data, or None if not found
        """
        try:
            image = Image.open(image_path)
            
            # Check image.info dictionary (PNG chunks, etc.)
            if hasattr(image, 'info') and image.info:
                # Look for Description or Comment fields
                for key in ['Description', 'Comment', 'comment', 'description']:
                    if key in image.info:
                        value = image.info[key]
                        if isinstance(value, (str, bytes)):
                            try:
                                # Convert bytes to string if needed
                                if isinstance(value, bytes):
                                    value_str = value.decode('utf-8', errors='ignore')
                                else:
                                    value_str = str(value)
                                
                                # Try to parse as JSON
                                if value_str.startswith('{'):
                                    data = json.loads(value_str)
                                    if 'world' in data and 'name' in data.get('world', {}):
                                        logger.debug(f"Found metadata in {key}")
                                        return data
                            except (json.JSONDecodeError, UnicodeDecodeError):
                                continue
            
            # Check EXIF data for JPEG files
            exif_data = image.getexif()
            
            if exif_data:
                # Check tag 270 directly (ImageDescription - most common for VRCX data)
                if 270 in exif_data:
                    value = exif_data[270]
                    if isinstance(value, (str, bytes)):
                        try:
                            if isinstance(value, bytes):
                                value_str = value.decode('utf-8', errors='ignore')
                            else:
                                value_str = str(value)
                            
                            if value_str.startswith('{'):
                                data = json.loads(value_str)
                                if 'world' in data and 'name' in data.get('world', {}):
                                    logger.debug(f"Found metadata in EXIF tag 270")
                                    return data
                        except (json.JSONDecodeError, UnicodeDecodeError):
                            pass
                
                # Also check IFD sections for completeness
                if hasattr(exif_data, 'get_ifd'):
                    try:
                        ifd = exif_data.get_ifd(0)
                        if 270 in ifd:
                            value = ifd[270]
                            if isinstance(value, (str, bytes)):
                                try:
                                    if isinstance(value, bytes):
                                        value_str = value.decode('utf-8', errors='ignore')
                                    else:
                                        value_str = str(value)
                                    
                                    if value_str.startswith('{'):
                                        data = json.loads(value_str)
                                        if 'world' in data and 'name' in data.get('world', {}):
                                            logger.debug(f"Found metadata in EXIF 0th IFD tag 270")
                                            return data
                                except (json.JSONDecodeError, UnicodeDecodeError):
                                    pass
                    except Exception:
                        pass
            
            return None
            
        except Exception as e:
            logger.error(f"Error reading metadata from {image_path}: {e}")
            self.stats['errors'] += 1
            return None
    
    def get_software_metadata(self, image_path: Path) -> Optional[str]:
        """
        Extract software metadata from image.
        
        Args:
            image_path: Path to the image file
            
        Returns:
            Software string, or None if not found
        """
        try:
            image = Image.open(image_path)
            
            # Check image.info for software info
            if hasattr(image, 'info') and image.info:
                for key in ['Software', 'software', 'CreatorTool', 'creator_tool']:
                    if key in image.info:
                        value = image.info[key]
                        if isinstance(value, (str, bytes)):
                            if isinstance(value, bytes):
                                return value.decode('utf-8', errors='ignore')
                            return str(value)
            
            # Check EXIF for software tag (305) directly first
            exif_data = image.getexif()
            if exif_data:
                if 305 in exif_data:  # Software tag
                    value = exif_data[305]
                    if isinstance(value, bytes):
                        return value.decode('utf-8', errors='ignore')
                    return str(value)
                
                # Also check IFD sections
                if hasattr(exif_data, 'get_ifd'):
                    try:
                        ifd = exif_data.get_ifd(0)
                        if 305 in ifd:
                            value = ifd[305]
                            if isinstance(value, bytes):
                                return value.decode('utf-8', errors='ignore')
                            return str(value)
                    except Exception:
                        pass
            
            return None
        except Exception:
            return None
    
    def get_world_name(self, image_path: Path) -> Optional[str]:
        """
        Extract world name from image metadata.
        
        Args:
            image_path: Path to the image file
            
        Returns:
            World name string, or None if not found
        """
        metadata = self.extract_vrcx_metadata(image_path)
        if metadata and 'world' in metadata:
            world_name = metadata['world'].get('name')
            if world_name:
                # Sanitize world name for use as folder name
                # Remove invalid filesystem characters
                invalid_chars = '<>:"|?*/'
                for char in invalid_chars:
                    world_name = world_name.replace(char, '_')
                # Clean up multiple underscores
                while '__' in world_name:
                    world_name = world_name.replace('__', '_')
                return world_name
        return None
    
    def get_image_dimensions(self, image_path: Path) -> Optional[tuple]:
        """
        Get image dimensions.
        
        Args:
            image_path: Path to the image file
            
        Returns:
            Tuple of (width, height) or None if error
        """
        try:
            image = Image.open(image_path)
            return image.size
        except Exception as e:
            logger.error(f"Error getting dimensions for {image_path}: {e}")
            return None
    
    def organize_month_folder(self, month_folder: Path, dry_run: bool = False) -> bool:
        """
        Organize all images in a month folder by world or into Prints folder.
        2048x1440 images go to Prints folder, others organized by world.
        
        Args:
            month_folder: Path to the month folder (e.g., 2025-03)
            dry_run: If True, log actions without moving files
        """
        if self.watch_mode and not hasattr(self, '_seen_files'):
            self._seen_files = set()

        # Find all candidate image files in this folder
        image_files = [
            f for f in month_folder.iterdir()
            if f.is_file() and f.suffix.lower() in self.image_extensions
        ]

        if self.watch_mode:
            image_files = [
                f for f in image_files
                if f.resolve() not in self._seen_files
            ]
            if not image_files:
                return False

        logger.info(f"Processing {month_folder.name}...")
        
        # Create Prints folder if it will be needed
        prints_folder = month_folder / "Prints"
        
        for image_file in image_files:
            if self.watch_mode and self._stop_event.is_set():
                logger.info('Stop requested; exiting current month folder early')
                return True
            
            resolved_path = image_file.resolve()
            if self.watch_mode:
                self._seen_files.add(resolved_path)
            
            self.stats['processed'] += 1
            
            # Check if image is 2048x1440 (Prints)
            dimensions = self.get_image_dimensions(image_file)
            if dimensions == (2048, 1440):
                # Move to Prints folder
                prints_folder.mkdir(exist_ok=True)
                dest_path = prints_folder / image_file.name
                
                # Handle duplicate names
                counter = 1
                base_stem = image_file.stem
                while dest_path.exists():
                    new_name = f"{base_stem}_{counter}{image_file.suffix}"
                    dest_path = prints_folder / new_name
                    counter += 1
                
                try:
                    if dry_run:
                        logger.info(f"Dry run: would move {image_file.name} -> Prints/")
                    else:
                        shutil.move(str(image_file), str(dest_path))
                        logger.info(f"Moved {image_file.name} -> Prints/")
                    self.stats['organized'] += 1
                except Exception as e:
                    logger.error(f"Failed to move {image_file.name} to Prints: {e}")
                    self.stats['errors'] += 1
                continue
            
            # Extract world name for non-Print images
            world_name = self.get_world_name(image_file)
            
            if not world_name:
                logger.debug(f"No world metadata found for {image_file.name}")
                self.stats['no_metadata'] += 1
                continue
            
            # Create world subfolder
            world_folder = month_folder / world_name
            world_folder.mkdir(exist_ok=True)
            
            # Move image to world folder
            dest_path = world_folder / image_file.name
            
            # Handle duplicate names
            counter = 1
            base_stem = image_file.stem
            while dest_path.exists():
                new_name = f"{base_stem}_{counter}{image_file.suffix}"
                dest_path = world_folder / new_name
                counter += 1
            
            try:
                if dry_run:
                    logger.info(f"Dry run: would move {image_file.name} -> {world_name}/")
                else:
                    shutil.move(str(image_file), str(dest_path))
                    logger.info(f"Moved {image_file.name} -> {world_name}/")
                self.stats['organized'] += 1
            except Exception as e:
                logger.error(f"Failed to move {image_file.name}: {e}")
                self.stats['errors'] += 1
        return True
    
    def organize_single_folder(self, folder: Path, software_filter: Optional[str] = None, dry_run: bool = False) -> bool:
        """
        Organize all images in a single folder by world.
        Optionally filters by software metadata.
        
        Args:
            folder: Path to the folder to organize
            software_filter: Optional software string to filter by (e.g., "Adobe Photoshop Lightroom Classic 15.3 (Windows)")
            dry_run: If True, log actions without moving files
        """
        if self.watch_mode and not hasattr(self, '_seen_files'):
            self._seen_files = set()

        # Find all candidate image files in this folder
        image_files = [
            f for f in folder.iterdir()
            if f.is_file() and f.suffix.lower() in self.image_extensions
        ]

        if self.watch_mode:
            image_files = [
                f for f in image_files
                if f.resolve() not in self._seen_files
            ]
            if not image_files:
                return False

        logger.info(f"Processing folder: {folder.name}")
        
        if software_filter:
            logger.info(f"Filtering by software: {software_filter}")
        
        # Create Prints folder reference
        prints_folder = folder / "Prints"
        
        for image_file in image_files:
            if self.watch_mode and self._stop_event.is_set():
                logger.info('Stop requested; exiting current folder early')
                return True
            
            resolved_path = image_file.resolve()
            if self.watch_mode:
                self._seen_files.add(resolved_path)
            
            self.stats['processed'] += 1
            
            # Check software filter if specified
            if software_filter:
                software = self.get_software_metadata(image_file)
                if not software or software_filter not in software:
                    logger.debug(f"Skipping {image_file.name}: software mismatch (found: {software})")
                    self.stats['no_metadata'] += 1
                    continue
            
            # Check if image is 2048x1440 (Prints)
            dimensions = self.get_image_dimensions(image_file)
            if dimensions == (2048, 1440):
                # Move to Prints folder
                prints_folder.mkdir(exist_ok=True)
                dest_path = prints_folder / image_file.name
                
                # Handle duplicate names
                counter = 1
                base_stem = image_file.stem
                while dest_path.exists():
                    new_name = f"{base_stem}_{counter}{image_file.suffix}"
                    dest_path = prints_folder / new_name
                    counter += 1
                
                try:
                    if dry_run:
                        logger.info(f"Dry run: would move {image_file.name} -> Prints/")
                    else:
                        shutil.move(str(image_file), str(dest_path))
                        logger.info(f"Moved {image_file.name} -> Prints/")
                    self.stats['organized'] += 1
                except Exception as e:
                    logger.error(f"Failed to move {image_file.name} to Prints: {e}")
                    self.stats['errors'] += 1
                continue
            
            # Extract world name for non-Print images
            world_name = self.get_world_name(image_file)
            
            if not world_name:
                logger.debug(f"No world metadata found for {image_file.name}")
                self.stats['no_metadata'] += 1
                continue
            
            # Create world subfolder
            world_folder = folder / world_name
            world_folder.mkdir(exist_ok=True)
            
            # Move image to world folder
            dest_path = world_folder / image_file.name
            
            # Handle duplicate names
            counter = 1
            base_stem = image_file.stem
            while dest_path.exists():
                new_name = f"{base_stem}_{counter}{image_file.suffix}"
                dest_path = world_folder / new_name
                counter += 1
            
            try:
                if dry_run:
                    logger.info(f"Dry run: would move {image_file.name} -> {world_name}/")
                else:
                    shutil.move(str(image_file), str(dest_path))
                    logger.info(f"Moved {image_file.name} -> {world_name}/")
                self.stats['organized'] += 1
            except Exception as e:
                logger.error(f"Failed to move {image_file.name}: {e}")
                self.stats['errors'] += 1
        return True
    
    def run(
        self,
        single_folder: Optional[Path] = None,
        software_filter: Optional[str] = None,
        dry_run: bool = False,
        watch: bool = False,
        interval: int = 30,
    ) -> None:
        """Run the organization process."""
        self.watch_mode = watch
        if watch:
            self._seen_files = set()
            self._stop_event.clear()
        
        def run_once() -> bool:
            # If single_folder is specified, organize just that folder
            if single_folder:
                if not single_folder.exists():
                    logger.error(f"Path does not exist: {single_folder}")
                    return False

                return self.organize_single_folder(single_folder, software_filter, dry_run=dry_run)
            else:
                if not self.base_path.exists():
                    logger.error(f"Path does not exist: {self.base_path}")
                    return False

                # Find all month folders (YYYY-MM pattern)
                month_folders = sorted([
                    d for d in self.base_path.iterdir()
                    if d.is_dir() and len(d.name) == 7 and d.name[4] == '-'
                ])

                if not month_folders:
                    logger.warning(f"No month folders (YYYY-MM format) found in {self.base_path}")
                    return False

                any_changed = False
                for month_folder in month_folders:
                    if self._stop_event.is_set():
                        logger.info('Stop requested; halting remaining month folders')
                        break
                    if self.organize_month_folder(month_folder, dry_run=dry_run):
                        any_changed = True

                return any_changed

        if watch:
            logger.info(f"Watch mode enabled, scanning every {interval} seconds")
            try:
                while True:
                    if self._stop_event.is_set():
                        logger.info("Stop requested; exiting watch loop")
                        break

                    stats_before = self.stats.copy()
                    changed = run_once()

                    if changed:
                        logger.info("\n" + "="*50)
                        logger.info("Watch scan summary:")
                        logger.info(f"New images processed: {self.stats['processed'] - stats_before['processed']}")
                        logger.info(f"Images organized: {self.stats['organized'] - stats_before['organized']}")
                        logger.info(f"Images without metadata: {self.stats['no_metadata'] - stats_before['no_metadata']}")
                        logger.info(f"Errors: {self.stats['errors'] - stats_before['errors']}")
                        logger.info("="*50)
                        logger.info(f"Sleeping for {interval} seconds before next scan...")

                    # Sleep in small increments to be responsive to stop requests
                    slept = 0
                    while slept < interval and not self._stop_event.is_set():
                        time.sleep(1)
                        slept += 1
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
        default=30,
        help='Watch interval in seconds when --watch is enabled'
    )
    parser.add_argument(
        '--single-folder',
        action='store_true',
        help='Treat path as a single folder to organize (not as a root with YYYY-MM folders)'
    )
    parser.add_argument(
        '--software-filter',
        type=str,
        default=None,
        help='Only process images from a specific software (e.g., "Adobe Photoshop Lightroom Classic 15.3 (Windows)")'
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
        software_filter=args.software_filter,
        dry_run=args.dry_run,
        watch=args.watch,
        interval=args.interval,
    )


if __name__ == '__main__':
    main()
