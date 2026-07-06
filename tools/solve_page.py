"""Solve a saved Shapeshifter page from the command line — no server or
userscript needed.

Usage: python tools/solve_page.py <level.html> [path-to-solver-core-exe]

The solver binary is located from (in order): the second argument, the
SOLVER_CORE environment variable, target/release next to this repo, or
`solver-core` on PATH.
"""

import json
import os
import subprocess
import sys
import tempfile

import shapeshifter_page as page


def find_exe(arg):
    candidates = []
    if arg:
        candidates.append(arg)
    if os.environ.get('SOLVER_CORE'):
        candidates.append(os.environ['SOLVER_CORE'])
    here = os.path.dirname(os.path.abspath(__file__))
    for name in ('solver-core.exe', 'solver-core'):
        candidates.append(os.path.join(here, '..', 'solver-core', 'target',
                                       'release', name))
    candidates.append('solver-core')
    for c in candidates:
        if os.path.sep in c or os.path.exists(c):
            if os.path.exists(c):
                return c
        else:
            return c  # bare name: let PATH resolve it
    sys.exit('solver-core binary not found; build it or pass a path')


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    html = open(sys.argv[1], encoding='utf-8', errors='ignore').read()
    parsed = page.parse_html(html)
    payload = page.to_payload(parsed)
    exe = find_exe(sys.argv[2] if len(sys.argv) > 2 else None)

    fd, tmp = tempfile.mkstemp(suffix='.json')
    try:
        with os.fdopen(fd, 'w') as f:
            json.dump(payload, f)
        out = subprocess.run([exe, tmp], capture_output=True, text=True)
    finally:
        os.unlink(tmp)

    print(out.stdout.rstrip())
    if out.returncode != 0:
        sys.exit(out.stderr.rstrip() or out.returncode)


if __name__ == '__main__':
    main()
