"""Exercise removal in a temporary tree with mocked systemctl; never touch host units."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('uninstall', Path(__file__).with_name('uninstall-socket-activation.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class UninstallTests(unittest.TestCase):
    def test_remove_repeat_and_keep_data(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            rule = root/'keeper.rules'
            data = root/'keeper.db'; data.write_text('retain')
            paths = [root/name for name in module.UNITS]+[rule]
            for path in paths: path.write_text('unit')
            with patch.object(module,'UNIT_DIR',root), patch.object(module,'RULE',rule), patch('sys.argv',['uninstall','binary']), patch.object(module.os,'geteuid',return_value=0), patch.object(module.subprocess,'run') as calls:
                calls.side_effect=lambda cmd,**kw: subprocess.CompletedProcess(cmd,0,stdout=('loaded' if (root/cmd[2]).exists() else 'not-found') if cmd[1]=='show' else '')
                module.main()
                self.assertTrue(all(not p.exists() for p in paths))
                self.assertEqual(data.read_text(),'retain')
                self.assertEqual([c.args[0][1] for c in calls.call_args_list],['show','show','show','stop','disable','daemon-reload'])
                calls.reset_mock(); module.main()
                self.assertNotIn('stop',[c.args[0][1] for c in calls.call_args_list])
    def test_stop_failure_keeps_configuration(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); rule=root/'keeper.rules';rule.write_text('retain')
            def run(cmd,**kw):
                if cmd[1]=='stop': raise subprocess.CalledProcessError(1,cmd)
                return subprocess.CompletedProcess(cmd,0,stdout='loaded')
            with patch.object(module,'UNIT_DIR',root), patch.object(module,'RULE',rule), patch('sys.argv',['uninstall','binary']), patch.object(module.os,'geteuid',return_value=0), patch.object(module.subprocess,'run',side_effect=run):
                with self.assertRaises(subprocess.CalledProcessError):module.main()
                self.assertTrue(rule.exists())

unittest.main()
