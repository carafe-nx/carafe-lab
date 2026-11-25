import json
import os
from pathlib import Path
import sys


ASCII_LOWER = str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")


def find_named(directory, name, root):
    folders = []
    try:
        with os.scandir(directory) as entries:
            for entry in entries:
                if entry.is_dir(follow_symlinks=False):
                    folders.append(Path(entry.path))
                elif entry.name.translate(ASCII_LOWER) == name.translate(ASCII_LOWER):
                    path = Path(entry.path)
                    if path.resolve().is_relative_to(root):
                        return path
    except (OSError, RuntimeError):
        return None
    for folder in folders:
        found = find_named(folder, name, root)
        if found is not None:
            return found
    return None


def read_first(name, roots):
    for directory in roots:
        root = Path(directory).absolute()
        found = find_named(root, name, root)
        if found is not None:
            try:
                return found.read_bytes().decode("utf-8", errors="replace")
            except OSError:
                return ""
    return None


if __name__ == "__main__":
    print(json.dumps(read_first(sys.argv[1], sys.argv[2:])))
