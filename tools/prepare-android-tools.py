#!/usr/bin/env python3
"""Fetch a project-local Android compile toolchain; never redistribute SDK binaries."""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import zipfile

ROOT=Path(__file__).resolve().parents[1]
DEST=ROOT/'.cache/android-tools'
ARTIFACTS=[
 ('jdk.tar.gz','https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.13%2B9/OpenJDK21U-jdk_aarch64_mac_hotspot_21.0.13_9.tar.gz','sha256','bf036b35b071bfb2f54f2c0af8ee8d88afa95ee6eb3dce02c0f94cfc2524d04a'),
 ('platform.zip','https://dl.google.com/android/repository/platform-36_r02.zip','sha1','2c1a80dd4d9f7d0e6dd336ec603d9b5c55a6f576'),
 ('build-tools.zip','https://dl.google.com/android/repository/build-tools_r36_macosx.zip','sha1','199ae0047ee61e842f8ee0c6d3918e44fb9a1f83'),
 ('android-ndk-r30-darwin.zip','https://dl.google.com/android/repository/android-ndk-r30-darwin.zip','sha1','c060be96767eefbb8e0a27796d6f43115fc1a0c4'),
 ('vulkan-validation.zip','https://github.com/KhronosGroup/Vulkan-ValidationLayers/releases/download/vulkan-sdk-1.4.363.0/android-binaries-1.4.363.0.zip','sha256','9a3b8e32cb3330406dd4f3651594201635baee4528da8e2a5300e4f55e1a9cc7'),
]
def fetch(item):
 name,url,algorithm,expected=item
 path=DEST/name
 if not path.exists(): subprocess.run(['curl','-fL','--retry','3','--silent','--show-error',url,'-o',str(path)],check=True)
 with path.open('rb') as f: assert hashlib.file_digest(f,algorithm).hexdigest()==expected, name
 with path.open('rb') as f: sha=hashlib.file_digest(f,'sha256').hexdigest()
 return {'file':name,'url':url,'upstream_digest_algorithm':algorithm,'upstream_digest':expected,'sha256':sha,'bytes':path.stat().st_size}
def main():
 DEST.mkdir(parents=True,exist_ok=True)
 with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool: results=list(pool.map(fetch,ARTIFACTS))
 for name in ['platform','build-tools']:
  target=DEST/name
  if not target.exists():
   with zipfile.ZipFile(DEST/f'{name}.zip') as z:
    assert all(not Path(x.filename).is_absolute() and '..' not in Path(x.filename).parts for x in z.infolist())
    z.extractall(target)
    for entry in z.infolist():
     if not entry.is_dir(): os.chmod(target/entry.filename,(entry.external_attr>>16)&0o777 or 0o644)
 if not (DEST/'jdk').exists():
  with tarfile.open(DEST/'jdk.tar.gz') as t: t.extractall(DEST/'jdk',filter='data')
 if not (DEST/'ndk/android-ndk-r30').exists():
  subprocess.run(['ditto','-x','-k',str(DEST/'android-ndk-r30-darwin.zip'),str(DEST/'ndk')],check=True)
 if not (DEST/'vulkan-validation/android-binaries-1.4.363.0').exists():
  with zipfile.ZipFile(DEST/'vulkan-validation.zip') as z:
   assert all(not Path(x.filename).is_absolute() and '..' not in Path(x.filename).parts for x in z.infolist())
   z.extractall(DEST/'vulkan-validation')
 (DEST/'provenance.json').write_text(json.dumps(results,indent=2)+'\n')
 print(DEST)
if __name__=='__main__': main()
