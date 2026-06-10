#!/usr/bin/env python3
"""Setup configuration for VRChat Screenshot Organizer"""

from setuptools import setup, find_packages

with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

setup(
    name="vrchat-organizer",
    version="1.0.0",
    author="VRChat Organizer Contributors",
    description="Automatically organize VRChat screenshots by world",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/yourusername/VRChat-Organizer",
    packages=find_packages(),
    classifiers=[
        "Development Status :: 4 - Beta",
        "Intended Audience :: End Users/Desktop",
        "Topic :: Multimedia :: Graphics",
        "Topic :: System :: Archiving :: Mirroring",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.7",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Operating System :: POSIX :: Linux",
        "Operating System :: MacOS",
        "Operating System :: Microsoft :: Windows",
    ],
    python_requires=">=3.7",
    install_requires=[
        "Pillow>=9.0.0",
    ],
    entry_points={
        "console_scripts": [
            "vrchat-organizer=organize_vrchat:main",
            "vrchat-preview=preview_vrchat:main",
            "vrchat-debug=debug_metadata:main",
        ],
    },
    include_package_data=True,
    keywords="vrchat screenshots organizer metadata exif",
)
