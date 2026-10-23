#!/usr/bin/env python3
"""Fetch the locked public AOSP experiment image; do not install it as a qualified runtime."""
import hashlib
import json
from pathlib import Path
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
# Public-build API identifier published by google/android-cuttlefish, not a user credential.
PUBLIC_BUILD_KEY = "AIzaSyBIelMvbjtNkpa5O96eqbm_IuSUA5WsO14"
API = "https://androidbuild-pa.googleapis.com/v4/"

def api(path):
    request = urllib.request.Request(API + path, headers={"X-goog-api-key": PUBLIC_BUILD_KEY})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)

def digest(path, algorithm):
    value = hashlib.new(algorithm)
    with path.open("rb") as file:
        for block in iter(lambda: file.read(4 << 20), b""):
            value.update(block)
    return value.hexdigest()

def main():
    lock = json.loads((ROOT / "engine/cuttlefish-lock.json").read_text())
    destination = ROOT / ".cache/cuttlefish" / lock["build_id"]
    destination.mkdir(parents=True, exist_ok=True)
    records = []
    for artifact in lock["artifacts"]:
        name = artifact["name"]
        if Path(name).name != name:
            raise SystemExit("Artifact must be a filename")
        path = destination / name
        if not path.exists() or path.stat().st_size != artifact["bytes"] or digest(path, "md5") != artifact["api_md5"]:
            url = api(f"builds/{lock['build_id']}/{lock['target']}/attempts/latest/artifacts/{urllib.parse.quote(name)}/url")["signedUrl"]
            partial = path.with_suffix(path.suffix + ".partial")
            print(f"Downloading {name}: {artifact['bytes']} bytes", flush=True)
            request = urllib.request.Request(url)
            count = 0
            with urllib.request.urlopen(request, timeout=120) as response, partial.open("wb") as file:
                while block := response.read(4 << 20):
                    count += len(block)
                    if count > artifact["bytes"]:
                        raise SystemExit("Download exceeded locked size")
                    file.write(block)
            if count != artifact["bytes"] or digest(partial, "md5") != artifact["api_md5"]:
                raise SystemExit("Artifact size or publisher API digest mismatch")
            partial.replace(path)
        records.append({**artifact, "sha256": digest(path, "sha256")})
        if records[-1]["sha256"] != artifact["sha256"]:
            raise SystemExit("Artifact differs from the locally pinned SHA-256")
        print(f"Verified {name}", flush=True)
    output = ROOT / "evidence/g0/android-image-provenance.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({**lock, "artifacts": records, "publisher_signature_verified": False, "locally_source_built": False}, indent=2) + "\n")
    print(destination)

if __name__ == "__main__":
    main()
