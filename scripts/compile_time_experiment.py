#!/usr/bin/env python3
"""Stage an isolated CLI experiment; never edit framework/CLI sources in place.

The staged workspace references the current libraries by absolute paths and
inherits this checkout's CLI build profiles. Build its `proa` binary, then pass
that binary to compile_time_probe.py or dev_latency_probe.py.
"""

import argparse
from pathlib import Path
import re
import shutil
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-identical-cache-writes", action="store_true")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    root = Path(tempfile.mkdtemp(prefix="proa-cli-compile-experiment-"))
    shutil.copytree(repo / "proa_cli", root / "proa_cli")
    manifest = (repo / "Cargo.toml").read_text()
    manifest = re.sub(r'^members = \[.*?\]', 'members = ["proa_cli"]', manifest, flags=re.M | re.S)
    manifest = re.sub(r'^default-members = \[.*?\]', 'default-members = ["proa_cli"]', manifest, flags=re.M | re.S)
    manifest = re.sub(r'path = "([^"]+)"', lambda match: 'path = "' + str(repo / match[1]) + '"', manifest)
    manifest = manifest.replace('license-file = "LICENSE"', f'license-file = "{repo / "LICENSE"}"')
    (root / "Cargo.toml").write_text(manifest)
    for name in ("Cargo.lock", "rust-toolchain.toml"):
        shutil.copyfile(repo / name, root / name)
    if args.skip_identical_cache_writes:
        cache = root / "proa_cli/src/rsjs/cache.rs"
        source = cache.read_text()
        # Preserve file-kind checks, atomic replacement, and durability for all
        # changed records. Existing durable equal records need no replacement.
        anchor = 'fn atomic_write_replaceable(path: &Path, bytes: &[u8]) -> Result<(), DescriptorCacheError> {'
        guard = '''
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file()) {
        if let Ok(existing) = fs::read(path) {
            if existing.len() == bytes.len() + 1
                && existing.last() == Some(&b'\\n')
                && &existing[..bytes.len()] == bytes
            {
                return Ok(());
            }
        }
    }
'''
        start = source.index(anchor)
        section = source[start:]
        section = section.replace("    ensure_plain_directory(parent)?;", "    ensure_plain_directory(parent)?;" + guard, 1)
        source = source[:start] + section
        cache.write_text(source)
    print(root)
    print(f"cargo build --manifest-path {root / 'Cargo.toml'} -p proa-cli --bin proa --release")


if __name__ == "__main__":
    main()
