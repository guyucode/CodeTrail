"""Construct an explicit reviewed check plan outside the indexed repository."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BASE = 'd9efbc04bdfe71c2561310891db34f71d921ab8f'
REASONS = {'Cargo.lock': 'Lock the two official C/C++ grammar packages; generated dependency edges are not '
               'business code.',
 'crates/reader-core/Cargo.toml': 'Declare C/C++ grammar dependencies; actual extraction and '
                                  'boundaries are associated with the syntax-index feature.',
 'crates/reader-core/src/lib.rs': 'Move the existing traversal policy into source_walker; its '
                                  'implementation is associated. Module registration is shared '
                                  'plumbing; deleted lines were moved, not removed behavior.',
 'crates/reader-core/src/parse.rs': 'Imports, grammar dispatch, bounded error counting and shared '
                                    'symbol-recording refactor support associated C/C++ '
                                    'extraction. Deleted side moves the existing record logic '
                                    'without retiring old language identities.',
 'crates/reader-core/src/search.rs': 'DTO fields, serialization derives and imports support the '
                                     'associated bounded read-only search implementation.',
 'crates/reader-core/tests/c_family.rs': 'Shared test imports support five associated grammar, '
                                         'scope and partial-index regressions.',
 'crates/reader-core/tests/search.rs': 'Shared test imports; both meaningful text and boundary '
                                       'regressions are associated.',
 'docs/USER_GUIDE.md': 'Document C/C++ source-development support, supported extensions, literal '
                       'search and semantic/call-graph limits; existing full guide evidence covers '
                       'the instructions.',
 'src-tauri/Cargo.lock': 'Synchronize the desktop lockfile with the same C/C++ parser packages '
                         'used by reader-cli.',
 'src-tauri/src/lib.rs': 'Register the session-bound search command; the command implementation is '
                         'associated.',
 'src/App.tsx': 'Import the search component and replace input wording. Existing App evidence '
                'includes result navigation and review isolation.',
 'src/api.ts': 'Result type import and desktop/demo dispatch; existing api evidence includes '
               'literal demo search and native invocation.',
 'src/components/CodeSearch.tsx': 'Component imports and prop declarations; query cancellation and '
                                  'result actions are associated with the component.',
 'src/styles.css': 'Narrow sidebar layout and long-result ellipsis, without data or access '
                   'semantics.',
 'src/types.ts': 'Frontend DTO declarations mirror native search results, without separate '
                 'business behavior.',
 'README.md': 'Update both platform downloads and matching CLI links to 0.3.1; preserve '
              'compatibility and signing boundaries.',
 'docs/MAC_RELEASE.md': 'Document Mac 0.3.1 installers and tools without overstating platform '
                        'validation.',
 'docs/WINDOWS_RELEASE.md': 'Document Windows 0.3.1 installer and tool names with the actual '
                            'validation boundary.',
 'docs/AGENT_DELIVERY.md': 'Specify matching 0.3.1 C/C++ CLI and desktop support; preserve schema '
                           'and human review isolation.',
 'scripts/windows-check-plan.py': 'Update the fixed release baseline and reviewed exceptions for '
                                  'search, C/C++ and version metadata; reject unknown uncovered '
                                  'paths.',
 '.github/workflows/windows-preview.yml': 'Run the final release CLI against an isolated C/C++ '
                                          'delivery fixture before packaging.',
 'scripts/test-windows-install.ps1': 'Require the new search entry in the installed native '
                                     'frontend in addition to the existing exact-binary, resource '
                                     'and rendering checks.',
 'scripts/check-release-cli.py': 'Synthetic final-binary acceptance fixture: inspect C/C++ symbols '
                                 'and deliver/check valid associations without real user records.',
 'package.json': 'Synchronize release package version to 0.3.1.',
 'package-lock.json': 'Synchronize root npm package version without changing npm dependency '
                      'resolution.',
 'crates/reader-cli/Cargo.toml': 'Synchronize CLI version with the 0.3.1 desktop and core.',
 'crates/reader-collab/Cargo.toml': 'Synchronize local workspace package version; no collaboration '
                                    'protocol changes.',
 'src-tauri/Cargo.toml': 'Synchronize the desktop package release version.',
 'src-tauri/tauri.conf.json': 'Set bundle version 0.3.1 while preserving identifier, data paths '
                              'and permissions.',
 'scripts/package-windows.ps1': 'Select only the current-version x64 installer so cached older '
                                'installers cannot be shipped or block packaging.'}

def main():
    cli = ROOT / 'target/debug' / ('reader-cli.exe' if os.name == 'nt' else 'reader-cli')
    report = json.loads(subprocess.check_output([str(cli), 'agent', 'coverage', str(ROOT), BASE], encoding='utf-8'))
    exceptions = []
    for file in report['files']:
        if file['needsExplanation']:
            if file['path'] not in REASONS:
                raise SystemExit('Unreviewed coverage exception: ' + file['path'])
            exceptions.append({'path': file['path'], 'changeDigest': file['changeDigest'], 'reason': REASONS[file['path']]})
    npm = 'npm.cmd' if os.name == 'nt' else 'npm'
    checks = [
        ('cargo', ['test', '--workspace', '--locked']),
        ('cargo', ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']),
        (npm, ['test']), (npm, ['run', 'build']),
        ('cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--locked']),
        (sys.executable, ['-B', '-m', 'unittest', 'discover', '-s', 'sdk/python', '-p', 'test_*.py']),
        ('git', ['diff', '--check', BASE]),
    ]
    plan = {'schemaVersion': 1, 'base': BASE, 'unassociated': exceptions,
            'checks': [{'program': program, 'args': args} for program, args in checks]}
    output = Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir())) / 'codetrail-windows-check-plan.json'
    output.write_text(json.dumps(plan, ensure_ascii=False), encoding='utf-8')
    print(output)

if __name__ == '__main__':
    main()
