"""Migration/ownership tests in a temporary tree; no host units or privileges."""
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('install-socket-activation.py').resolve()
NativePath = type(Path())

class InstallTests(unittest.TestCase):
    def invoke(self, root, source):
        repo = root/'opt/cangling-keeper'
        class FixturePath(NativePath):
            def __new__(cls, *args):
                return object.__new__(cls)
            def __init__(self, *args):
                if args and isinstance(args[0], str):
                    value = args[0]
                    for prefix in ['/opt/', '/etc/', '/var/backups/', '/usr/local/']:
                        if value.startswith(prefix):
                            args = (str(root/value.lstrip('/')), *args[1:])
                            break
                super().__init__(*args)
            def stat(self, *args, **kwargs):
                result = super().stat(*args, **kwargs)
                if str(self) == str(repo):
                    values = list(result); values[4] = 0
                    return os.stat_result(values)
                return result
        with patch('pathlib.Path', FixturePath), patch('sys.argv', ['install', str(source)]), \
             patch.dict(os.environ, {'SUDO_USER': __import__('pwd').getpwuid(os.getuid()).pw_name}, clear=True), \
             patch('os.geteuid', return_value=0), patch('os.chown') as ownership, \
             patch('shutil.which', return_value=None), patch('subprocess.run') as commands:
            commands.return_value = subprocess.CompletedProcess([], 0)
            runpy.run_path(str(SCRIPT), run_name='__main__')
            return ownership.call_args_list, commands.call_args_list

    def fixture(self, root):
        repo = root/'opt/cangling-keeper'; repo.mkdir(parents=True, mode=0o755)
        (root/'var/backups').mkdir(parents=True)
        (root/'usr/local/bin').mkdir(parents=True)
        source = root/'new-binary'; source.write_bytes(b'new build')
        (repo/'cangling-keeper').write_bytes(b'old build')
        (root/'usr/local/bin/cangling-keeper').symlink_to(repo/'cangling-keeper')
        return repo, source

    def test_migrates_legacy_binary_keeps_root_units_and_user_runtime(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); repo, source = self.fixture(root)
            ownership, commands = self.invoke(root, source)
            app = repo/'app'; exe = app/'cangling-keeper'
            self.assertEqual(exe.read_bytes(), b'new build')
            self.assertEqual((repo/'cangling-keeper').resolve(), exe)
            self.assertEqual((root/'usr/local/bin/cangling-keeper').resolve(), exe)
            self.assertEqual(app.stat().st_mode & 0o777, 0o755)
            self.assertEqual(exe.stat().st_mode & 0o777, 0o755)
            self.assertTrue(any(c.args == (app, os.getuid(), os.getgid()) for c in ownership))
            self.assertTrue(any('/app/.keeper-' in str(c.args[0]) and c.args[1:] == (os.getuid(), os.getgid()) for c in ownership))
            unit = (root/'etc/systemd/system/cangling-keeper-local.service').read_text()
            self.assertIn('User='+__import__('pwd').getpwuid(os.getuid()).pw_name, unit)
            self.assertIn(str(exe), unit)
            self.assertIn('Sockets=cangling-keeper-http.socket cangling-keeper-https.socket', unit)
            rule = (root/'etc/polkit-1/rules.d/49-cangling-keeper-local.rules').read_text()
            self.assertIn('reset-failed', rule)
            self.assertNotIn('manage-unit-files', rule)
            verbs = [call.args[0][1] for call in commands if call.args[0][0] == 'systemctl']
            self.assertEqual(verbs, ['daemon-reload'])
            backups = list((root/'var/backups').glob('*/cangling-keeper'))
            self.assertEqual(backups[0].read_bytes(), b'old build')

    def test_rejects_symlink_application_directory(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); repo, source = self.fixture(root)
            outside = root/'outside'; outside.mkdir(); (repo/'app').symlink_to(outside)
            with self.assertRaises(SystemExit): self.invoke(root, source)
            self.assertEqual((repo/'cangling-keeper').read_bytes(), b'old build')
            self.assertEqual(list(outside.iterdir()), [])

    def test_refuses_unrelated_command_entry(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); repo, source = self.fixture(root)
            command = root/'usr/local/bin/cangling-keeper'; command.unlink(); command.write_text('unrelated')
            with self.assertRaises(SystemExit): self.invoke(root, source)
            self.assertEqual(command.read_text(), 'unrelated')
            self.assertFalse((repo/'app').exists())

unittest.main()
