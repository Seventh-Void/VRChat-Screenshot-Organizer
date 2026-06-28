#!/usr/bin/env python3
"""VRChat Screenshot Organization Preview.

Shows what would be organized without making changes.
"""

import sys
from pathlib import Path
from collections import defaultdict

try:
    from PIL import Image  # noqa: F401
except ImportError as e:
    raise SystemExit("Missing dependency 'Pillow'. Install with: pip install Pillow") from e

from vrchat_metadata import extract_image_meta, sanitize_name



def _extract_target_sub(image_path: Path) -> str | None:
    """Return target subfolder name (or None if unknown)."""
    meta = extract_image_meta(image_path)
    if meta.dimensions == (2048, 1440):
        return "Prints"
    if meta.world_name:
        return sanitize_name(meta.world_name)
    return None


def preview(base_path) -> None:
    """Preview organization without making changes."""
    base_path = Path(base_path).expanduser()


    if not base_path.exists():
        print(f"Error: Path does not exist: {base_path}")
        sys.exit(1)

    image_extensions = {".png", ".jpg", ".jpeg"}

    month_folders = sorted(
        [
            d
            for d in base_path.iterdir()
            if d.is_dir() and len(d.name) == 7 and d.name[4] == "-"
        ]
    )

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

        worlds: dict[str, list[str]] = defaultdict(list)
        no_metadata: list[str] = []

        # Avoid sorting image files for speed.
        for image_file in month_folder.iterdir():
            if not image_file.is_file():
                continue
            if image_file.suffix.lower() not in image_extensions:
                continue

            total_processed += 1
            target = _extract_target_sub(image_file)

            if target:
                worlds[target].append(image_file.name)
                total_organized += 1
            else:
                no_metadata.append(image_file.name)
                total_no_metadata += 1

        for world_name in sorted(worlds.keys()):
            images = worlds[world_name]
            print(
                f"   📂 {world_name}/ ({len(images)} image{'s' if len(images) != 1 else ''})"
            )
            for img in images[:3]:
                print(f"      ├─ {img}")
            if len(images) > 3:
                print(f"      └─ ... and {len(images) - 3} more")

        if no_metadata:
            print(
                f"   ⚠️  No metadata ({len(no_metadata)} image{'s' if len(no_metadata) != 1 else ''})"
            )
            for img in no_metadata[:3]:
                print(f"      ├─ {img}")
            if len(no_metadata) > 3:
                print(f"      └─ ... and {len(no_metadata) - 3} more")

    print("\n" + "=" * 70)
    print("\n📊 Summary:")
    print(f"   Total images scanned: {total_processed}")
    print(f"   To be organized: {total_organized}")
    print(f"   Without metadata (will stay in month root): {total_no_metadata}")
    print("\n✨ Run 'python organize_vrchat.py' to apply organization\n")


if __name__ == "__main__":
    path = sys.argv[1] if len(sys.argv) > 1 else "~/Pictures/VRChat/VRChat"
    preview(path)

