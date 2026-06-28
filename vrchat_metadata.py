"""Shared VRChat screenshot metadata extraction.

Goal: keep organizer + preview consistent.

Metadata sources (observed patterns in VRChat images):
- PNG/JPEG `image.info` keys (e.g. Description/Comment) storing JSON.
- EXIF tag 270 (ImageDescription) storing JSON with a `world` object.

The JSON shape is expected to include:
{
  "world": { "name": "<world name>", ... },
  ...
}

This module intentionally avoids any runtime `pip install` logic.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

from PIL import Image


@dataclass(frozen=True)
class ImageMeta:
    dimensions: Optional[Tuple[int, int]]
    world_name: Optional[str]
    software: Optional[str]
    width: int
    height: int


_INVALID_CHARS = '<>:"|?*/\\'


def sanitize_name(name: str) -> str:
    """Sanitize world name for use as a folder name."""
    for char in _INVALID_CHARS:
        name = name.replace(char, "_")
    while "__" in name:
        name = name.replace("__", "_")
    return name.strip(". ")


def _parse_json_metadata(value: Any) -> Optional[Dict[str, Any]]:
    """Parse JSON from metadata values (str or bytes)."""
    if not isinstance(value, (str, bytes)):
        return None
    try:
        value_str = value.decode("utf-8", errors="ignore") if isinstance(value, bytes) else str(value)
        if value_str.startswith("{"):
            return json.loads(value_str)
    except (json.JSONDecodeError, UnicodeDecodeError):
        pass
    return None


def _decode_info_value(val: Any) -> Optional[str]:
    if isinstance(val, bytes):
        return val.decode("utf-8", errors="ignore")
    if val is None:
        return None
    return str(val)


def extract_image_meta(image_path: Path) -> ImageMeta:
    """Extract dimensions, world name, and software from a single image pass."""
    dimensions: Optional[Tuple[int, int]] = None
    world_name: Optional[str] = None
    software: Optional[str] = None
    width = 0
    height = 0

    try:
        with Image.open(image_path) as image:
            width, height = image.size
            dimensions = (width, height)

            # PNG/JPEG info keys (fast path)
            if getattr(image, "info", None):
                info = image.info or {}

                # Software
                for key in ("Software", "software", "CreatorTool", "creator_tool"):
                    if key in info:
                        software = _decode_info_value(info.get(key))
                        if software:
                            break

                # World JSON
                if world_name is None:
                    for key in ("Description", "Comment", "comment", "description"):
                        vrcx_data = _parse_json_metadata(info.get(key))
                        if vrcx_data and "world" in vrcx_data:
                            world_name = sanitize_name(vrcx_data["world"].get("name", "")) or None
                            if world_name:
                                break

            # EXIF
            exif_data = None
            try:
                exif_data = image.getexif()
            except Exception:
                exif_data = None

            if exif_data:
                # Software
                if software is None and 305 in exif_data:
                    software = _decode_info_value(exif_data.get(305))

                # World JSON (tag 270)
                if world_name is None:
                    vrcx_data = _parse_json_metadata(exif_data.get(270))
                    if vrcx_data and "world" in vrcx_data:
                        world_name = sanitize_name(vrcx_data["world"].get("name", "")) or None

                # Fallback: search IFD 0th for tag 270
                if world_name is None and hasattr(exif_data, "get_ifd"):
                    try:
                        ifd = exif_data.get_ifd(0)
                        vrcx_data = _parse_json_metadata(ifd.get(270))
                        if vrcx_data and "world" in vrcx_data:
                            world_name = sanitize_name(vrcx_data["world"].get("name", "")) or None
                    except Exception:
                        pass

    except (IOError, PermissionError):
        # File locked by another process or inaccessible.
        pass

    return ImageMeta(dimensions=dimensions, world_name=world_name, software=software, width=width, height=height)


def extract_date_from_filename(filename: str) -> Optional[datetime]:
    """Extract date from standard VRChat filename: VRChat_YYYY-MM-DD_..."""
    import re

    match = re.search(r"VRChat_(\d{4})-(\d{2})-(\d{2})", filename)
    if match:
        try:
            return datetime(int(match.group(1)), int(match.group(2)), int(match.group(3)))
        except ValueError:
            return None
    return None

