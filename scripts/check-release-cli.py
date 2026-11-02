"""Exercise the actual packaged CLI on synthetic C/C++ source and Agent records."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    cli = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='codetrail-release-cli-') as directory:
        root = Path(directory)
        (root / 'sum.c').write_text('int add(int a, int b) { return a+b; }\n', encoding='utf-8')
        (root / 'engine.hpp').write_text('namespace app { class Engine { public: int run(int x) { return x; } }; }\n', encoding='utf-8')

        def call(command, payload=None):
            args = [str(cli), 'agent', command, str(root)]
            result = subprocess.run(args, input=json.dumps(payload) if payload else None,
                                    text=True, encoding='utf-8', capture_output=True, check=True)
            return json.loads(result.stdout)

        inventory = call('inspect')
        evidence = []
        for index, name in enumerate(['add', 'app::Engine::run']):
            symbol = next(s for s in inventory['project']['symbols'] if s['qualifiedName'] == name)
            path = symbol['range']['file']
            evidence.append({'id': f'entry-{index}', 'path': path, 'symbolId': symbol['id'],
                             'expectedFileHash': inventory['fileHashes'][path], 'role': 'entry',
                             'responsibility': 'Read the synthetic native computation entry.'})
        call('deliver', {'schemaVersion': 1, 'deliveryId': 'release-native-fixture',
                         'expectedRevision': inventory['revision'], 'features': [{
                             'id': 'native-compute', 'expectedDigest': None, 'name': 'Native compute',
                             'description': 'Synthetic C/C++ release acceptance fixture.',
                             'summary': 'Verify final CLI grammar and delivery.', 'evidence': evidence}]})
        if not call('check')['ok']:
            raise SystemExit('Packaged CLI rejected C/C++ evidence')
        # Execute a real successful command through the packaged CLI, then read its trace.
        subprocess.run(['git', 'init', '-q', str(root)], check=True)
        subprocess.run(['git', '-C', str(root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(root), '-c', 'user.name=Release QA',
                        '-c', 'user.email=qa@example.invalid', 'commit', '-qm', 'Isolated fixture'], check=True)
        base = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
        report = call('validate', {'schemaVersion': 1, 'base': base, 'unassociated': [],
                                  'checks': [{'program': str(cli), 'args': ['agent', 'check', str(root)]}],
                                  'record': {'goal': 'Verify packaged CLI association checks',
                                             'path': 'sum.c', 'startLine': 1, 'endLine': 1}})
        assert report['ok'], report
        run_id = report['verificationRunId']
        def trace(command, payload=None):
            result = subprocess.run([str(cli), 'trace', command, str(root)],
                                    input=json.dumps(payload) if payload else None,
                                    text=True, encoding='utf-8', capture_output=True, check=True)
            return json.loads(result.stdout)['value']
        recorded = trace('read', {'runId': run_id})
        assert recorded['run']['verification']['verdict'] == 'matched', recorded
        assert recorded['run']['verification']['goal'] == 'Verify packaged CLI association checks'
        trace('start', {'id': 'legacy', 'label': 'Legacy fixture', 'kind': 'isolatedTest',
                        'producer': 'release acceptance', 'summary': 'No extended summary'})
        trace('append', {'runId': 'legacy', 'step': {'id': 'check', 'title': 'Observed command',
              'status': 'ok', 'result': 'The actual association command exited successfully',
              'reason': 'The preceding validate call completed', 'inputs': [], 'outputs': [], 'elapsedMs': 0,
              'location': {'path': 'sum.c', 'startLine': 1, 'endLine': 1,
                           'fileHash': inventory['fileHashes']['sum.c']}}})
        trace('finish', {'runId': 'legacy', 'status': 'completed', 'summary': 'Legacy shape'})
        assert 'verification' not in trace('read', {'runId': 'legacy'})['run']
    print(json.dumps({'ok': True, 'scope': 'Final CLI binary: isolated C/C++ associations, actual validation execution and legacy trace compatibility',
                      'symbols': ['add', 'app::Engine::run']}))


if __name__ == '__main__':
    main()
