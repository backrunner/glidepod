#!/usr/bin/env python3
"""Build original GlidePod Android helpers using the pinned local toolchain."""
import os
import pathlib
import subprocess
import zipfile
import shutil
import hashlib
import json

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = ROOT / '.cache/android-tools'
JDK = next((TOOLS / 'jdk').glob('*/Contents/Home'))
BUILD = next((TOOLS / 'build-tools').glob('android-*'))
ANDROID = next((TOOLS / 'platform').glob('*/android.jar'))
OUT = ROOT / 'target/android'
ENV = dict(os.environ, JAVA_HOME=str(JDK))

def run(*args):
    subprocess.run([*map(str, args)], check=True, env=ENV, cwd=ROOT)

def java(name):
    out = OUT / name
    classes = out / 'classes'
    dex = out / 'dex'
    if classes.exists(): shutil.rmtree(classes)
    if dex.exists(): shutil.rmtree(dex)
    classes.mkdir(parents=True, exist_ok=True)
    dex.mkdir(exist_ok=True)
    sources = sorted((ROOT / 'android' / name / 'src').rglob('*.java'))
    run(JDK / 'bin/javac', '-source', '8', '-target', '8', '-Xlint:-options', '-classpath', ANDROID,
        '-d', classes, *sources)
    run(BUILD / 'd8', '--lib', ANDROID, '--min-api', '29', '--output', dex, *sorted(classes.rglob('*.class')))
    return out, dex

def vulkan_native(out):
    ndk = TOOLS / 'ndk/android-ndk-r30'
    if not ndk.exists():
        raise RuntimeError('Install the pinned NDK r30; see .agents/implementation/08-native-vulkan.md')
    if '30.0.16248370' not in (ndk / 'source.properties').read_text():
        raise RuntimeError('Unexpected Android NDK revision')
    toolchain = ndk / 'toolchains/llvm/prebuilt/darwin-x86_64/bin'
    source = ROOT / 'android/vulkan/native'
    generated = out / 'generated'
    generated.mkdir(exist_ok=True)
    compiler = ndk / 'shader-tools/darwin-x86_64/glslc'
    header = ['#pragma once', '#include <stdint.h>']
    for shader in sorted((source / 'shaders').iterdir()):
        binary = generated / (shader.name + '.spv')
        run(compiler, '--target-env=vulkan1.1', '-O', shader, '-o', binary)
        import struct
        data = binary.read_bytes()
        words = struct.unpack('<' + 'I' * (len(data)//4), data)
        header.append('static const uint32_t ' + shader.name.replace('.', '_') + '[] = {' + ','.join(hex(w) for w in words) + '};')
    (generated / 'shaders.h').write_text('\n'.join(header)+'\n')
    library = out / 'libglidepod_vulkan.so'
    run(toolchain / 'aarch64-linux-android29-clang++', '-std=c++17', '-O2', '-Wall', '-Wextra', '-Werror',
        '-Wno-missing-field-initializers', '-fPIC', '-shared', '-static-libstdc++', '-DVK_USE_PLATFORM_ANDROID_KHR',
        '-I', generated, source / 'main.cpp', '-landroid', '-lvulkan', '-llog', '-Wl,-z,max-page-size=16384', '-o', library)
    (out / 'native-build.json').write_text(json.dumps({'ndk_revision':'30.0.16248370','abi':'arm64-v8a',
        'api':29,'spirv_target':'vulkan1.1','library_sha256':hashlib.sha256(library.read_bytes()).hexdigest(),
        'ndk_archive_sha1':'c060be96767eefbb8e0a27796d6f43115fc1a0c4'},indent=2)+'\n')
    return library

def apk(name, filename, package=None):
    out, dex = java(name)
    unsigned = out / 'unsigned.apk'
    res = ROOT / 'android' / name / 'res'
    compiled = []
    if res.exists():
        run(BUILD / 'aapt2', 'compile', '--dir', res, '-o', out / 'resources.zip')
        compiled = [out / 'resources.zip']
    manifest = ROOT / 'android' / name / 'AndroidManifest.xml'
    if package:
        original = manifest.read_text().replace('android:name=".MainActivity"', 'android:name="dev.glidepod.fixture.MainActivity"')
        manifest = out / 'AndroidManifest.xml'
        manifest.write_text(original.replace('package="dev.glidepod.fixture"', f'package="{package}"'))
    run(BUILD / 'aapt2', 'link', '-I', ANDROID, '--manifest', manifest, '-o', unsigned, *compiled)
    with zipfile.ZipFile(unsigned, 'a', zipfile.ZIP_DEFLATED) as archive:
        archive.write(dex / 'classes.dex', 'classes.dex')
        if name == 'vulkan':
            archive.write(vulkan_native(out), 'lib/arm64-v8a/libglidepod_vulkan.so')
            # Optional Khronos layer for diagnosis; never enabled in performance runs.
            layer = TOOLS / 'vulkan-validation/android-binaries-1.4.363.0/arm64-v8a/libVkLayer_khronos_validation.so'
            if layer.exists():
                if hashlib.sha256(layer.read_bytes()).hexdigest() != 'db7447027e28db95584e9fde8a57cc1f28c2dd09defcbe2f2b76ee2bbc65bf09':
                    raise RuntimeError('Khronos validation layer digest mismatch')
                archive.write(layer, 'lib/arm64-v8a/libVkLayer_khronos_validation.so')
                archive.write(ROOT / 'android/vulkan/licenses/Khronos-Validation-Layer.txt', 'assets/licenses/Khronos-Validation-Layer.txt')
            ndk = TOOLS / 'ndk/android-ndk-r30'
            archive.write(ndk / 'toolchains/llvm/prebuilt/darwin-x86_64/NOTICE', 'assets/licenses/LLVM-runtime.txt')
            archive.write(ndk / 'toolchains/llvm/prebuilt/darwin-x86_64/sysroot/NOTICE', 'assets/licenses/Android-runtime.txt')
    key = OUT / 'development.p12'
    if not key.exists():
        run(JDK / 'bin/keytool', '-genkeypair', '-keystore', key, '-storepass', 'android', '-alias', 'glidepod-dev', '-keyalg', 'RSA', '-keysize', '2048', '-validity', '3650', '-dname', 'CN=GlidePod Local Development')
    run(BUILD / 'zipalign', '-f', '-p', '4', unsigned, out / 'aligned.apk')
    run(BUILD / 'apksigner', 'sign', '--ks', key, '--ks-pass', 'pass:android', '--out', OUT / filename, out / 'aligned.apk')
    run(BUILD / 'apksigner', 'verify', OUT / filename)

if __name__ == '__main__':
    out, dex = java('bridge')
    with zipfile.ZipFile(OUT / 'glidepod-bridge.jar', 'w', zipfile.ZIP_DEFLATED) as archive:
        archive.write(dex / 'classes.dex', 'classes.dex')
    print(OUT / 'glidepod-bridge.jar')
    apk('fixtures', 'glidepod-fixture-secondary.apk', 'dev.glidepod.fixture.secondary')
    apk('fixtures', 'glidepod-fixture.apk')
    apk('widgets', 'glidepod-widgets.apk')
    apk('input', 'glidepod-input.apk')
    apk('notifications', 'glidepod-notifications.apk')
    apk('benchmark', 'glidepod-benchmark.apk')
    apk('vulkan', 'glidepod-vulkan.apk')
