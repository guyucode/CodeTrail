"""Construct an explicit reviewed check plan outside the indexed repository."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BASE = '6b95aaa0a2e59f9ced63765fb1afa686c653e4dc'
REASONS = {
    'crates/reader-core/src/git.rs': 'Honor Git index executable modes on Windows/core.filemode=false, preserving staged mode changes without misreporting unchanged scripts. New helper, comparison and regression are associated with Git diff; deletion side replaces the old immutable comparison binding.',
    '.github/workflows/windows-preview.yml': 'Windows-only manual CI orchestration; source validation, target-license collection, installer build and native smoke are associated with the delivery workflow.',
    'AGENTS.md': 'Replace the Mac-only release constraint with explicit Windows preview gates; other Agent, human-review and data-preservation rules remain in force.',
    'README.md': 'Replace the old no-Windows statement and add the Windows installation link; complete release instructions are associated with delivery workflow. Deletion side is the replaced platform statement.',
    'docs/WINDOWS_RELEASE.md': 'Documents Windows preview installation and the precise automated-versus-manual validation boundary, associated with delivery workflow.',
    'scripts/windows-check-plan.py': 'Explicit platform check command list and path-specific reviewed coverage reasons; generated plans stay outside project source.',
    'scripts/collect-release-licenses.py': 'Target-aware license inventory and original license/source redistribution used by Windows bundling; associated with delivery workflow.',
    'scripts/package-windows.ps1': 'Packages built Windows installer, matching CLI/SDK, legal material and hashes; associated with delivery workflow.',
    'scripts/test-windows-install.ps1': 'Isolated Windows installer/native-render smoke; associated with delivery workflow. It does not certify physical Windows 10/11 workflows.',
    'sdk/python/test_codetrail_trace.py': 'Choose reader-cli.exe on Windows in the existing recorder integration tests; import and executable-path setup are shared test plumbing, with full tests associated to runtime records.',
    'src-tauri/tauri.windows.conf.json': 'Windows-only NSIS overlay; existing Mac bundle configuration remains untouched.',
}

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
