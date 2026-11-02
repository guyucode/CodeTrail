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
    print(json.dumps({'ok': True, 'scope': 'Final CLI binary, isolated C/C++ inspect/deliver/check',
                      'symbols': ['add', 'app::Engine::run']}))


if __name__ == '__main__':
    main()
