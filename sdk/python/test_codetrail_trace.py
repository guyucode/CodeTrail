"""Standard-library recorder integration checks, using an isolated temporary root."""
import importlib.util
import os
import json
from pathlib import Path
import tempfile
import unittest
from codetrail_trace import Recorder, location, selected_values, verification
CLI=Path(__file__).resolve().parents[2]/"target/debug"/("reader-cli.exe" if os.name == "nt" else "reader-cli")

class RecorderChecks(unittest.TestCase):
    def test_selected_values_redact_known_credentials_and_label_truncation(self):
        values=selected_values({'api_key':'do-not-show','count':3,'text':'a'*3000})
        self.assertNotIn('do-not-show',str(values))
        self.assertEqual(values[1]['value'],'3')
        self.assertIn('截断',values[2]['value'])

    def test_record_actual_result_with_source_hash_and_readable_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/'logic.py';path.write_text('def run(value):\n    return value.strip()\n')
            spec=importlib.util.spec_from_file_location('logic',path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
            recorder=Recorder(root,CLI,'actual strip',kind='isolatedTest',producer='unittest',summary='Execute a real function',strict=True)
            source=location(root,module.run);actual=module.run('  hello  ')
            recorder.step('strip',f'actual result: {actual}','The function returned.',source=source,inputs={'value':'  hello  '},outputs={'actual':actual})
            recorder.finish('completed','The isolated function returned.', verification=verification('Strip whitespace', inputs={'value':'  hello  '}, expected='hello', actual=actual, key_values={'length':len(actual)}, verdict='matched'))
            data=json.loads((root/'.reader/runs.json').read_text())['runs'][0]
            self.assertEqual(data['steps'][0]['observation']['outputs'][0]['value'],'"hello"')
            self.assertIn('return value.strip()',data['steps'][0]['sourceExcerpt'])
            self.assertEqual(data['status'],'completed')
            self.assertEqual(data['verification']['actual'],'hello')
            self.assertEqual(data['verification']['keyValues'][0]['value'],'5')

    def test_unavailable_recorder_is_visible_and_nonfatal_by_default(self):
        with tempfile.TemporaryDirectory() as root:
            recorder=Recorder(root,Path(root)/'missing-cli','test',summary='Recorder not installed')
            self.assertFalse(recorder.active);self.assertEqual(len(recorder.errors),1)
            self.assertIsNone(recorder.step('one','result','reason',source={}))
            self.assertFalse((Path(root)/'.reader').exists())

if __name__=='__main__':unittest.main()
