#!/usr/bin/env python3
"""Debug script to inspect image metadata."""

import json
import sys
from pathlib import Path
from datetime import datetime
from PIL import Image
from PIL.ExifTags import TAGS

def preview_template(image_path, image, info):
    """Preview what the folder name would be with the default template."""
    world_name = "Unknown World"
    # Basic world name extraction for preview
    for key in ['Description', 'Comment', 'comment', 'description']:
        val = info.get(key)
        if val:
            try:
                val_str = val.decode('utf-8', errors='ignore') if isinstance(val, bytes) else str(val)
                data = json.loads(val_str)
                if 'world' in data:
                    world_name = data['world'].get('name', 'Unknown World')
                    break
            except: pass
    
    mod_time = datetime.fromtimestamp(image_path.stat().st_mtime)
    w, h = image.size
    
    print("\n📂 Template Variable Preview:")
    print(f"  {{world}}  -> {world_name}")
    print(f"  {{year}}   -> {mod_time.strftime('%Y')}")
    print(f"  {{month}}  -> {mod_time.strftime('%m')}")
    print(f"  {{day}}    -> {mod_time.strftime('%d')}")
    print(f"  {{width}}  -> {w}")
    print(f"  {{height}} -> {h}")

def inspect_image(image_path):
    """Inspect all metadata in an image file."""
    print(f"\n{'='*60}")
    print(f"Inspecting: {image_path.name}")
    print('='*60)
    
    try:
        image = Image.open(image_path)
        
        # Check image.info (PNG chunks, etc.)
        print("\n📋 Image Info (image.info):")
        if hasattr(image, 'info'):
            if image.info:
                for key, value in image.info.items():
                    if isinstance(value, bytes):
                        try:
                            value = value.decode('utf-8', errors='ignore')
                        except:
                            value = f"<bytes: {len(value)} chars>"
                    
                    # Truncate for display
                    if isinstance(value, str) and len(value) > 200:
                        print(f"  {key}: {value[:200]}...")
                    else:
                        print(f"  {key}: {value}")
            else:
                print("  (empty)")
        
        # Check EXIF data
        print("\n📸 EXIF Data (image.getexif()):")
        exif_data = image.getexif()
        if exif_data:
            print(f"  Found {len(exif_data)} EXIF tags")
            
            # Print main tags
            for tag_id, value in list(exif_data.items())[:10]:
                tag_name = TAGS.get(tag_id, tag_id)
                if isinstance(value, bytes):
                    try:
                        value_str = value.decode('utf-8', errors='ignore')
                        if len(value_str) > 200:
                            print(f"  {tag_name} ({tag_id}): {value_str[:200]}...")
                        else:
                            print(f"  {tag_name} ({tag_id}): {value_str}")
                    except:
                        print(f"  {tag_name} ({tag_id}): <bytes>")
                else:
                    print(f"  {tag_name} ({tag_id}): {value}")
            
            # Check IFD sections
            if hasattr(exif_data, 'get_ifd'):
                print("\n  📂 IFD Sections:")
                for ifd_name in ['0th', '1st', 'Exif', 'GPS']:
                    try:
                        ifd = exif_data.get_ifd(ifd_name)
                        print(f"    {ifd_name}: {len(ifd)} tags")
                        for tag_id, value in list(ifd.items())[:3]:
                            tag_name = TAGS.get(tag_id, tag_id)
                            if isinstance(value, bytes):
                                try:
                                    value_str = value.decode('utf-8', errors='ignore')
                                    if len(value_str) > 100:
                                        print(f"      {tag_name}: {value_str[:100]}...")
                                    else:
                                        print(f"      {tag_name}: {value_str}")
                                except:
                                    print(f"      {tag_name}: <bytes>")
                            else:
                                print(f"      {tag_name}: {value}")
                    except Exception as e:
                        print(f"    {ifd_name}: Error - {e}")
        else:
            print("  (no EXIF data)")
        
        # Show template preview
        preview_template(image_path, image, image.info)

        # Try to extract with exiftool if available
        print("\n🔧 Alternative: Using exiftool (if available):")
        import subprocess
        try:
            result = subprocess.run(
                ['exiftool', '-s', str(image_path)],
                capture_output=True,
                text=True,
                timeout=5
            )
            if result.returncode == 0:
                lines = result.stdout.strip().split('\n')
                for line in lines[:20]:
                    print(f"  {line}")
                if len(lines) > 20:
                    print(f"  ... and {len(lines) - 20} more lines")
            else:
                print("  (exiftool not found or error)")
        except:
            print("  (exiftool not available)")
            
    except Exception as e:
        print(f"Error: {e}")

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 debug_metadata.py <image_path> [image_path2] ...")
        print("\nExample: python3 debug_metadata.py ~/Pictures/VRChat/VRChat/2025-01/*.png")
        sys.exit(1)
    
    for arg in sys.argv[1:]:
        path = Path(arg).expanduser()
        if path.is_file():
            inspect_image(path)
        elif path.is_dir():
            # If directory, find first PNG
            pngs = list(path.glob('*.png'))
            if pngs:
                inspect_image(pngs[0])
            else:
                print(f"No PNG files in {path}")

if __name__ == '__main__':
    main()
