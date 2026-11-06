#!/usr/bin/env python3
"""Prepare pinned GApps and test installation in a NEW disposable VM, never a user's store.

The recovery script in the downloaded archive is never executed. This is an engineering
candidate, not a production installer, Google certification or a redistribution grant.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import tarfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / 'engine/google-apps-lock.json'
CACHE = ROOT / '.cache/google-apps'
MAX_UNPACKED = 2 * 1024**3


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def payload_entries(archive, expected_sdk, expected_arch, variant='core'):
    infos = archive.infolist()
    if len(infos) > 4096 or sum(i.file_size for i in infos) > MAX_UNPACKED:
        raise ValueError('Package exceeds entry or expanded-size limit')
    seen = set()
    for info in infos:
        name = info.filename.rstrip('/')
        parts = name.split('/')
        if (not name or any(p in ('', '.', '..') for p in parts)
                or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-/' for c in name)
                or name.casefold() in seen or info.flag_bits & 1):
            raise ValueError('Unsafe, duplicate or encrypted package entry')
        mode = info.external_attr >> 16
        if stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR):
            raise ValueError('Links and special files are not supported')
        seen.add(name.casefold())
    prop = archive.getinfo('build.prop')
    if prop.file_size > 64 * 1024:
        raise ValueError('Oversized build.prop')
    properties = {}
    for line in archive.read(prop).decode('utf-8').splitlines():
        if line.strip() and not line.lstrip().startswith('#'):
            key, value = line.split('=', 1)
            if key.strip() in properties:
                raise ValueError('Duplicate package property')
            properties[key.strip()] = value.strip()
    if properties.get('arch') != expected_arch or properties.get('version') != str(expected_sdk):
        raise ValueError('GApps architecture or Android SDK mismatch')
    result = []
    for info in infos:
        if info.is_dir() or not info.filename.startswith('system/'):
            continue
        parts = PurePosixPath(info.filename).parts
        if parts[1] == 'addon.d':
            continue
        if len(parts) < 4 or parts[1] not in ('product', 'system_ext'):
            raise ValueError('Unsupported system payload layout')
        destination = '/'.join(parts[1:])
        if destination.startswith(('system_ext/priv-app/SetupWizard/', 'product/priv-app/VelvetTitan/')):
            continue  # Keep AOSP Provision; this candidate is the ARM64 phone, not tangorpro.
        if variant == 'core' and parts[2] in ('app', 'priv-app') and not destination.startswith((
                'product/priv-app/Phonesky/', 'product/priv-app/GooglePartnerSetup/',
                'system_ext/priv-app/GoogleServicesFramework/', 'system_ext/priv-app/GoogleFeedback/')):
            continue  # Search, speech, wellbeing and other optional apps are separate from GMS.
        if parts[2] not in ('app', 'priv-app', 'apex', 'etc', 'framework', 'lib', 'lib64', 'overlay'):
            raise ValueError('Unsupported partition payload')
        result.append((destination, info))
    names = {name for name, _ in result}
    for required in ('product/priv-app/Phonesky/Phonesky.apk',
                     'system_ext/priv-app/GoogleServicesFramework/GoogleServicesFramework.apk',
                     'product/etc/permissions/privapp-permissions-google-product.xml',
                     'system_ext/etc/permissions/privapp-permissions-google-system_ext.xml'):
        if required not in names:
            raise ValueError('Missing required GApps payload: ' + required)
    if not any('GmsCore' in name and name.endswith('.apk') for name in names) and \
            'product/apex/com.google.android.gmssystem.prodvic.apex' not in names:
        raise ValueError('Missing Google Play services payload')
    # Privileged allowlists must be in place before the APKs, or system_server can fail.
    return sorted(result, key=lambda row: ('/etc/' not in row[0], row[0]))


def prepare(lock, variant='core'):
    CACHE.mkdir(parents=True, exist_ok=True)
    package = CACHE / lock['filename']
    expected = lock['sha256']
    if not package.exists():
        partial = package.with_suffix('.zip.partial')
        try:
            total = 0
            with urllib.request.urlopen(lock['url'], timeout=30) as response, partial.open('wb') as target:
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    if total > lock['bytes']:
                        raise ValueError('Download exceeds pinned size')
                    target.write(chunk)
            if total != lock['bytes'] or digest(partial) != expected:
                raise ValueError('Download does not match pinned SHA-256')
            os.replace(partial, package)
        finally:
            partial.unlink(missing_ok=True)
    if package.stat().st_size != lock['bytes'] or digest(package) != expected:
        raise ValueError('Cached GApps package does not match pinned size/hash')
    payload = CACHE / (expected + '.' + variant + '.tar')
    partial = payload.with_suffix('.tar.partial')
    with zipfile.ZipFile(package) as archive:
        entries = payload_entries(archive, lock['sdk'], lock['arch'], variant)
        total = sum(info.file_size for _, info in entries)
        if shutil.disk_usage(CACHE).free < total + 1024**3:
            raise ValueError('Need space for payload plus 1 GiB free headroom')
        try:
            with tarfile.open(partial, 'w', format=tarfile.USTAR_FORMAT) as target:
                for name, info in entries:
                    item = tarfile.TarInfo(name)
                    item.size, item.mode, item.uid, item.gid = info.file_size, 0o644, 0, 0
                    with archive.open(info) as stream:
                        target.addfile(item, stream)
            os.replace(partial, payload)
        finally:
            partial.unlink(missing_ok=True)
    plan = {'source': lock['source'], 'variant': variant, 'package_sha256': expected, 'payload_sha256': digest(payload),
            'sdk': lock['sdk'], 'arch': lock['arch'], 'payload_bytes': total,
            'files': [name for name, _ in entries], 'recovery_scripts_executed': False,
            'certification_verified': False}
    manifest = payload.with_suffix('.json')
    manifest.write_text(json.dumps(plan, indent=2) + '\n')
    return payload, manifest, plan


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare-only', action='store_true')
    parser.add_argument('--variant', choices=('core', 'full'), default='core')
    parser.add_argument('--runtime-store', type=Path)
    parser.add_argument('--candidate', type=Path)
    args = parser.parse_args()
    lock = json.loads(LOCK.read_text())
    payload, manifest, plan = prepare(lock, args.variant)
    print(json.dumps(plan, indent=2), flush=True)
    if args.prepare_only:
        return
    if args.runtime_store is None or args.candidate is None:
        parser.error('Use --prepare-only, or provide --runtime-store and a NEW --candidate')
    source, candidate = args.runtime_store.resolve(), args.candidate.resolve()
    cache_root = (ROOT / '.cache').resolve()
    if not source.is_relative_to(cache_root) or not candidate.is_relative_to(cache_root):
        parser.error('Evaluation stores must be under the project .cache')
    if candidate == cache_root or candidate.exists():
        parser.error('Candidate must not exist; existing user data is never modified')
    config = json.loads((source / 'configuration.json').read_text())
    config['active_profile'] = 'developer'
    config['notifications_enabled'] = False
    config['recent_developer'] = []
    config['recent_personal'] = []
    candidate.mkdir(parents=True, mode=0o700)
    (candidate / 'configuration.json').write_text(json.dumps(config, indent=2) + '\n')
    for name in ('experimental-runtime.json', 'experimental-gpu.json'):
        if (source / name).exists():
            shutil.copy2(source / name, candidate / name)
    (candidate / 'google-apps-evaluation.json').write_text(json.dumps({
        'purpose': 'disposable_google_apps_candidate', 'package_sha256': plan['package_sha256'],
        'payload_sha256': plan['payload_sha256'], 'source_store': str(source)}, indent=2) + '\n')
    environment = dict(os.environ, GLIDEPOD_GAPPS_STORE=str(candidate),
                       GLIDEPOD_GAPPS_PAYLOAD=str(payload), GLIDEPOD_GAPPS_PLAN=str(manifest))
    subprocess.run(['cargo', 'test', '-p', 'glidepod-runtime', '--locked', '--lib',
                    'google_services::tests::isolated_google_apps_candidate', '--', '--ignored', '--nocapture'],
                   cwd=ROOT, env=environment, check=True)


if __name__ == '__main__':
    main()
