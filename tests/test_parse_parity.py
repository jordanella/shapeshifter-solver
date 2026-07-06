"""Verify the userscript's JS parser matches the Python parser exactly.

Usage: python tests/test_parse_parity.py   (requires node)
"""

import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'tools'))
import shapeshifter_page as page

HERE = os.path.dirname(__file__)
FIXTURES = os.path.join(HERE, 'fixtures')
JS = os.path.join(HERE, 'dom', 'test_userscript_parse.js')


def normalize(p):
    p = dict(p)
    p['shapes'] = [{'id': s['id'], 'points': sorted(s['points'])}
                   for s in p['shapes']]
    return p


if __name__ == '__main__':
    for name in sorted(os.listdir(FIXTURES)):
        fp = os.path.join(FIXTURES, name)
        html = open(fp, encoding='utf-8', errors='ignore').read()
        a = normalize(page.to_payload(page.parse_html(html)))
        out = subprocess.run(['node', JS, fp], capture_output=True,
                             text=True, check=True).stdout
        b = normalize(json.loads(out))
        assert a == b, f'{name}: MISMATCH\npython={a}\njs={b}'
        print(f'{name}: MATCH')
    print('Parser parity confirmed on all fixtures.')
