"""Round-trip parser test: JSON fixture -> synthetic page -> both parsers.

For each JSON fixture, tests/dom/render_page.js renders a page with the
same structural patterns as the real game, and then BOTH the Python parser
(tools/shapeshifter_page.py) and the userscript's JS parser must recover
the original payload exactly.

To additionally check the parsers against real saved game pages (not
shipped in this repo), point REAL_PAGES at a directory of captures:
    REAL_PAGES=path/to/captures python tests/test_parse_parity.py

Usage: python tests/test_parse_parity.py   (requires node)
"""

import json
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'tools'))
import shapeshifter_page as page

HERE = os.path.dirname(__file__)
FIXTURES = os.path.join(HERE, 'fixtures')
RENDER = os.path.join(HERE, 'dom', 'render_page.js')
JS_PARSE = os.path.join(HERE, 'dom', 'test_userscript_parse.js')


def normalize(p):
    p = dict(p)
    p['shapes'] = [{'id': s['id'], 'points': sorted(s['points'])}
                   for s in p['shapes']]
    return p


def js_parse(html_path):
    out = subprocess.run(['node', JS_PARSE, html_path], capture_output=True,
                         text=True, check=True).stdout
    return normalize(json.loads(out))


def round_trip(fixture_path):
    with open(fixture_path) as f:
        expected = normalize(json.load(f))
    html = subprocess.run(['node', RENDER, fixture_path], capture_output=True,
                          text=True, check=True).stdout

    got_py = normalize(page.to_payload(page.parse_html(html)))
    assert got_py == expected, f'{fixture_path}: python parser round-trip MISMATCH'

    fd, tmp = tempfile.mkstemp(suffix='.html')
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as f:
            f.write(html)
        got_js = js_parse(tmp)
    finally:
        os.unlink(tmp)
    assert got_js == expected, f'{fixture_path}: JS parser round-trip MISMATCH'


if __name__ == '__main__':
    for name in sorted(os.listdir(FIXTURES)):
        if name.endswith('.json'):
            round_trip(os.path.join(FIXTURES, name))
            print(f'{name}: round-trip MATCH (python + js)')

    real_dir = os.environ.get('REAL_PAGES')
    if real_dir:
        for name in sorted(os.listdir(real_dir)):
            if not name.endswith('.html'):
                continue
            fp = os.path.join(real_dir, name)
            html = open(fp, encoding='utf-8', errors='ignore').read()
            a = normalize(page.to_payload(page.parse_html(html)))
            b = js_parse(fp)
            assert a == b, f'{name}: real-page parser MISMATCH'
            print(f'{name}: real-page parity MATCH')

    print('Parser tests passed.')
