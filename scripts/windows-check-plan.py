"""Construct an explicit reviewed check plan outside the indexed repository."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BASE = '7db14e3eea166d6996968e5ee9e9a9ce5c358b68'
REASONS = {'crates/reader-cli/src/workflow.rs': 'Replace post-check short-circuit errors with explicit failure reports and a trace lifecycle; prior validation branches are retained in stricter form.', 'docs/AGENT_DELIVERY.md': 'Replace the old execution description with the explicit reviewed-command boundary and document optional trace recording.', 'docs/RUNTIME_RECORDS.md': 'Replace the old step-first default with the five-field overview; retain old producer and storage compatibility.', 'docs/USER_GUIDE.md': 'Replace the prior step-first reading instructions with the summary-first workflow and clear legacy-data behavior.', 'sdk/python/codetrail_trace.py': 'Extend finish with an optional verification payload; omitting it still sends the unchanged legacy shape.', 'sdk/python/test_codetrail_trace.py': 'Extend the actual-function recorder integration check to verify expected/actual summary and observed values.', 'src/components/RuntimePane.tsx': 'Replace the always-expanded run summary and step list with the five-field overview plus closed process disclosure; legacy details and navigation remain available.', 'package.json': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'package-lock.json': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'crates/reader-core/Cargo.toml': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'crates/reader-cli/Cargo.toml': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'crates/reader-collab/Cargo.toml': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'Cargo.lock': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'src-tauri/Cargo.toml': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'src-tauri/Cargo.lock': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'src-tauri/tauri.conf.json': 'Synchronize 0.3.2 package and generated lock metadata; no dependency resolution change.', 'README.md': 'Document matching 0.3.2 downloads, five-field runtime summaries, upgrade compatibility and actual platform validation boundaries.', 'docs/MAC_RELEASE.md': 'Document matching 0.3.2 downloads, five-field runtime summaries, upgrade compatibility and actual platform validation boundaries.', 'docs/WINDOWS_RELEASE.md': 'Document matching 0.3.2 downloads, five-field runtime summaries, upgrade compatibility and actual platform validation boundaries.', 'scripts/windows-check-plan.py': 'Set the fixed runtime-summary release baseline, reviewed delta exceptions and explicit real validation recording.', 'scripts/check-release-cli.py': 'Exercise the actual final CLI on isolated C/C++ associations and real command-generated verification records, without user data.'}

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
            'checks': [{'program': program, 'args': args} for program, args in checks],
            'record': {'goal': 'Validate runtime summaries and release compatibility on Windows',
                       'path': 'src/components/RuntimePane.tsx', 'startLine': 56, 'endLine': 66}}
    output = Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir())) / 'codetrail-windows-check-plan.json'
    output.write_text(json.dumps(plan, ensure_ascii=False), encoding='utf-8')
    print(output)

if __name__ == '__main__':
    main()
