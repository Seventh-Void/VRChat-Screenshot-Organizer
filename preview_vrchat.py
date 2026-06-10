#!/usr/bin/env python3
"""
VRChat Screenshot Organization Preview
Shows what would be organized without making changes.
"""

import json
import sys
import subprocess
from pathlib import Path
from collections import defaultdict

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

def extract_vrcx_metadata(image_path):
    """Extract VRCX metadata from image EXIF data."""
    try:
        image = Image.open(image_path)
        exif_data = image.getexif()
        
        if not exif_data:
            return None
        
        for tag_id, value in exif_data.items():
            tag_name = TAGS.get(tag_id, tag_id)
            
            if tag_name in ['ImageDescription', 'UserComment', '0th', '1st']:
                if isinstance(value, (str, bytes)):
                    try:
                        if isinstance(value, bytes):
                            value_str = value.decode('utf-8', errors='ignore')
                        else:
                            value_str = value
                        
                        if value_str.startswith('{'):
                            data = json.loads(value_str)
                            if 'world' in data and 'name' in data.get('world', {}):
                                return data
                    except (json.JSONDecodeError, UnicodeDecodeError):
                        continue
        
        if hasattr(exif_data, 'get_ifd'):
            for ifd_name in ['0th', '1st', 'Exif', 'GPS']:
                try:
                    ifd = exif_data.get_ifd(ifd_name)
                    for tag_id, value in ifd.items():
                        tag_name = TAGS.get(tag_id, tag_id)
                        if tag_name in ['ImageDescription', 'UserComment']:
                            try:
                                if isinstance(value, bytes):
                                    value_str = value.decode('utf-8', errors='ignore')
                                else:
                                    value_str = str(value)
                                
                                if value_str.startswith('{'):
                                    data = json.loads(value_str)
                                    if 'world' in data and 'name' in data.get('world', {}):
                                        return data
                            except (json.JSONDecodeError, UnicodeDecodeError):
                                continue
                except Exception:
                    continue
        
        return None
    except Exception as e:
        print(f"Error reading {image_path}: {e}", file=sys.stderr)
        return None

def get_image_info(image_path):
    """Get dimensions and metadata from image."""
    try:
        with Image.open(image_path) as image:
            return image.size, extract_vrcx_metadata(image_path)
    except Exception:
        return None, None

def get_world_name(image_path):
    """Extract world name from image metadata."""
    metadata = extract_vrcx_metadata(image_path)
    if metadata and 'world' in metadata:
        world_name = metadata['world'].get('name')
        if world_name:
            invalid_chars = '<>:"|?*'
            for char in invalid_chars:
                world_name = world_name.replace(char, '_')
            return world_name
    return None

def preview(base_path):
    """Preview organization without making changes."""
    base_path = Path(base_path).expanduser()
    
    if not base_path.exists():
        print(f"Error: Path does not exist: {base_path}")
        sys.exit(1)
    
    image_extensions = {'.png', '.jpg', '.jpeg'}
    
    # Find month folders
    month_folders = sorted([
        d for d in base_path.iterdir()
        if d.is_dir() and len(d.name) == 7 and d.name[4] == '-'
    ])
    
    if not month_folders:
        print(f"No month folders found in {base_path}")
        sys.exit(1)
    
    print(f"Preview: Organization of {len(month_folders)} month folders\n")
    print("=" * 70)
    
    total_processed = 0
    total_organized = 0
    total_no_metadata = 0
    
    for month_folder in month_folders:
        print(f"\n📁 {month_folder.name}/")
        
        worlds = defaultdict(list)
        no_metadata = []
        
        # Scan images
        for image_file in sorted(month_folder.iterdir()):
            if not image_file.is_file():
                continue
            if image_file.suffix.lower() not in image_extensions:
                continue
            
            total_processed += 1
            dimensions, metadata = get_image_info(image_file)
            
            target = None
            # Check for prints first, matching organizer logic
            if dimensions == (2048, 1440):
                target = "Prints"
            elif metadata and 'world' in metadata:
                world_name = metadata['world'].get('name')
                if world_name:
                    invalid_chars = '<>:"|?*'
                    for char in invalid_chars:
                        world_name = world_name.replace(char, '_')
                    target = world_name
            
            if target:
                worlds[target].append(image_file.name)
                total_organized += 1
            else:
                no_metadata.append(image_file.name)
                total_no_metadata += 1
        
        # Display results
        for world_name in sorted(worlds.keys()):
            images = worlds[world_name]
            print(f"   📂 {world_name}/ ({len(images)} image{'s' if len(images) != 1 else ''})")
            for img in images[:3]:  # Show first 3 images
                print(f"      ├─ {img}")
            if len(images) > 3:
                print(f"      └─ ... and {len(images) - 3} more")
        
        if no_metadata:
            print(f"   ⚠️  No metadata ({len(no_metadata)} image{'s' if len(no_metadata) != 1 else ''})")
            for img in no_metadata[:3]:
                print(f"      ├─ {img}")
            if len(no_metadata) > 3:
                print(f"      └─ ... and {len(no_metadata) - 3} more")
    
    print("\n" + "=" * 70)
    print(f"\n📊 Summary:")
    print(f"   Total images scanned: {total_processed}")
    print(f"   To be organized: {total_organized}")
    print(f"   Without metadata (will stay in month root): {total_no_metadata}")
    print("\n✨ Run 'python organize_vrchat.py' to apply organization\n")

if __name__ == '__main__':
    path = sys.argv[1] if len(sys.argv) > 1 else '~/Pictures/VRChat/VRChat'
    preview(path)
