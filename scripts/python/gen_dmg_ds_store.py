#!/usr/bin/env python3
"""Generate the Finder .DS_Store for a Solidway dmg volume.

Usage: gen_dmg_ds_store.py <staging-or-mounted-volume-root>

The root must already contain:
  - Solidway.app
  - Applications -> /Applications (symlink)

macOS 26 Finder ignores custom dmg backgrounds (both backgroundImageAlias and
backgroundColor). The layout therefore only writes the window frame
(`bwsp`), arrange mode (`icvp.arrangeBy`) and icon positions (`Iloc`).
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
# Resolve the vendored libraries regardless of the caller's cwd/PYTHONPATH.
sys.path.insert(0, HERE)

from ds_store import DSStore  # noqa: E402

APP_NAME = "Solidway"
APP_BUNDLE = APP_NAME + ".app"

WINDOW_ORIGIN = (100, 100)
WINDOW_SIZE = (660, 420)

# Icon positions are Finder view coordinates (center points).
APP_ILOC = (165, 210)
APPLICATIONS_ILOC = (495, 210)


def fail(message):
    print(message, file=sys.stderr)
    sys.exit(1)


def main():
    if len(sys.argv) != 2:
        fail("用法: gen_dmg_ds_store.py <staging-or-mounted-volume-root>")

    volume = os.path.abspath(sys.argv[1])
    app = os.path.join(volume, APP_BUNDLE)
    applications = os.path.join(volume, "Applications")

    if not os.path.isdir(app):
        fail("缺少 app bundle: " + app)
    if not os.path.islink(applications):
        fail("缺少 Applications 符号链接: " + applications)

    bounds_string = "{{%d, %d}, {%d, %d}}" % (
        WINDOW_ORIGIN[0],
        WINDOW_ORIGIN[1],
        WINDOW_SIZE[0],
        WINDOW_SIZE[1],
    )
    bwsp = {
        "ShowStatusBar": False,
        "WindowBounds": bounds_string,
        "ContainerShowSidebar": False,
        "PreviewPaneVisibility": False,
        "SidebarWidth": 180,
        "ShowTabView": False,
        "ShowToolbar": False,
        "ShowPathbar": False,
        "ShowSidebar": False,
    }
    # Only arrangeBy: macOS 26 ignores background keys; keep icon view defaults
    # for size/text so Finder does not reflow the manually placed icons.
    icvp = {"arrangeBy": "none"}

    ds_store_path = os.path.join(volume, ".DS_Store")
    with DSStore.open(ds_store_path, "w+") as store:
        store["."]["bwsp"] = bwsp
        store["."]["icvp"] = icvp
        store[APP_BUNDLE]["Iloc"] = APP_ILOC
        store["Applications"]["Iloc"] = APPLICATIONS_ILOC

    print("wrote %s (window %dx%d)" % (ds_store_path, WINDOW_SIZE[0], WINDOW_SIZE[1]))


if __name__ == "__main__":
    main()
