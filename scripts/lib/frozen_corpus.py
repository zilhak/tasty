"""Validate the fixed aggregate corpus independently of per-file exceptions."""
from pathlib import Path
import sys

def entries(path):
    return [line.strip() for line in path.read_text().splitlines()
            if line.strip() and not line.lstrip().startswith("#")]

def validate(root):
    frozen = entries(root / ".complexity-frozen-files")
    allowed = entries(root / ".complexity-file-allowlist")
    if not frozen or len(set(frozen)) != len(frozen):
        raise ValueError("frozen corpus is empty or contains duplicate paths")
    for path in frozen:
        rel = Path(path)
        if rel.is_absolute() or ".." in rel.parts or rel.suffix != ".rs" or not (root / rel).is_file():
            raise ValueError("frozen corpus path is invalid or missing: " + path)
    absent = set(allowed) - set(frozen)
    if absent:
        raise ValueError("file exemptions missing from frozen corpus: " + ", ".join(sorted(absent)))
    return frozen

if __name__ == "__main__":
    try:
        print("\n".join(validate(Path(sys.argv[1]))))
    except (OSError, ValueError, IndexError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(2)
