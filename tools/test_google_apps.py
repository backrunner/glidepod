"""Package safety gates for the isolated Google apps evaluator; no downloads or VM required."""
import importlib.util
import io
from pathlib import Path
import stat
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('gapps', Path(__file__).with_name('evaluate-google-apps.py'))
gapps = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gapps)


class PackageTests(unittest.TestCase):
    def package(self, extras=(), sdk=37, omit=None):
        stream = io.BytesIO()
        entries = {
            'build.prop': f'arch=arm64\nversion={sdk}\nversion_nice=17.0.0\n',
            'system/product/priv-app/Phonesky/Phonesky.apk': 'apk',
            'system/system_ext/priv-app/GoogleServicesFramework/GoogleServicesFramework.apk': 'apk',
            'system/product/etc/permissions/privapp-permissions-google-product.xml': 'xml',
            'system/system_ext/etc/permissions/privapp-permissions-google-system_ext.xml': 'xml',
            'system/product/apex/com.google.android.gmssystem.prodvic.apex': 'apex',
            'system/system_ext/priv-app/SetupWizard/SetupWizard.apk': 'not installed',
            'system/product/priv-app/Velvet/Velvet.apk': 'optional search app',
            'META-INF/com/google/android/update-binary': 'never execute this script',
        }
        with zipfile.ZipFile(stream, 'w') as archive:
            for name, data in entries.items():
                if name != omit:
                    archive.writestr(name, data)
            for name, data in extras:
                archive.writestr(name, data)
        stream.seek(0)
        return zipfile.ZipFile(stream)

    def test_android_17_apex_and_allowlist_order(self):
        with self.package() as archive:
            entries = gapps.payload_entries(archive, 37, 'arm64')
        names = [name for name, _ in entries]
        self.assertTrue(all('/etc/' in name for name in names[:2]))
        self.assertTrue(any(name.endswith('.apex') for name in names))
        self.assertFalse(any('SetupWizard' in name or 'META-INF' in name for name in names))
        self.assertFalse(any('/Velvet/' in name for name in names))
        with self.package() as archive:
            full = gapps.payload_entries(archive, 37, 'arm64', 'full')
        self.assertTrue(any('/Velvet/' in name for name, _ in full))

    def test_mismatched_android_and_missing_privileged_allowlists_are_rejected(self):
        for archive in (self.package(sdk=36), self.package(omit='system/product/etc/permissions/privapp-permissions-google-product.xml')):
            with archive, self.assertRaises(ValueError):
                gapps.payload_entries(archive, 37, 'arm64')

    def test_path_traversal_and_shell_characters_are_rejected(self):
        for name in ('system/product/../escape', '/system/product/escape', 'system/product/a;id',
                     'system/product//escape', 'system/product/space name'):
            with self.package([(name, 'payload')]) as archive, self.assertRaises(ValueError):
                gapps.payload_entries(archive, 37, 'arm64')

    def test_symlinks_and_case_collisions_are_rejected(self):
        link = zipfile.ZipInfo('system/product/escape')
        link.create_system = 3
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        for extras in ([(link, '/data')], [('BUILD.PROP', 'conflict')]):
            with self.package(extras) as archive, self.assertRaises(ValueError):
                gapps.payload_entries(archive, 37, 'arm64')


if __name__ == '__main__':
    unittest.main()
