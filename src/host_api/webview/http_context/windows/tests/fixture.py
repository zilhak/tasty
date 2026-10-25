"""Build an isolated source-bound fixture with the root's exact native dependencies.

Usage: python fixture.py REPOSITORY OWN_NEW_FIXTURE_DIRECTORY
The caller supplies the build/native locks and an existing owned Cargo target.
"""
import json
from pathlib import Path
import shutil
import sys
import tomllib


def prepare(root, destination):
    root = root.resolve(strict=True)
    destination.mkdir(parents=True, exist_ok=False)
    source = root / "src/host_api/webview/http_context/windows.rs"
    blocks = (root / "Cargo.lock").read_text().split("[[package]]")[1:]
    packages = {}
    names = {}
    for block in blocks:
        package = tomllib.loads("[[package]]" + block)["package"][0]
        key = package["name"], package["version"]
        packages[key] = package, block
        names.setdefault(package["name"], []).append(key)
    pending = [("windows", "0.61.3"), ("webview2-com", "0.38.2")]
    selected = set()
    while pending:
        key = pending.pop()
        if key in selected:
            continue
        selected.add(key)
        package = packages[key][0]
        assert "source" in package and "checksum" in package
        for dependency in package.get("dependencies", []):
            words = dependency.split()
            if len(words) > 1:
                pending.append((words[0], words[1]))
            else:
                assert len(names[words[0]]) == 1, dependency
                pending.append(names[words[0]][0])
    manifest = '''[package]
name = "tasty-windows-http-context-fixture"
version = "0.0.0"
edition = "2024"
[workspace]
[target.'cfg(windows)'.dependencies]
webview2-com = "=0.38.2"
windows = { version = "=0.61.3", features = ["Win32_Foundation", "Win32_Graphics_Gdi", "Win32_System_Com", "Win32_System_LibraryLoader", "Win32_UI_WindowsAndMessaging"] }
[lints.rust]
unsafe_op_in_unsafe_fn = "deny"
[lints.clippy]
undocumented_unsafe_blocks = "deny"
multiple_unsafe_ops_per_block = "deny"
cognitive_complexity = "deny"
'''
    (destination / "Cargo.toml").write_text(manifest)
    lock = '''version = 4

[[package]]
name = "tasty-windows-http-context-fixture"
version = "0.0.0"
dependencies = ["webview2-com", "windows"]

'''
    lock += "".join("[[package]]" + packages[key][1] for key in sorted(selected))
    (destination / "Cargo.lock").write_text(lock)
    (destination / "src").mkdir()
    (destination / "src/lib.rs").write_text(
        f"#[path = {json.dumps(str(source))}]\nmod context;\n"
        "pub fn refuses_remote_http() -> bool { context::DataStore::new().is_err() }\n"
    )
    shutil.copyfile(root / "clippy.toml", destination / "clippy.toml")


if __name__ == "__main__":
    prepare(Path(sys.argv[1]), Path(sys.argv[2]))
