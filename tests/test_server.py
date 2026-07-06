"""End-to-end tests against a running solver server.

Start the server first:  solver-core serve
Then:                    python tests/test_server.py
"""

import json
import os
import sys
import urllib.request

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'tools'))
import shapeshifter_page as page

SERVER = 'http://127.0.0.1:8977'
FIXTURES = os.path.join(os.path.dirname(__file__), 'fixtures')


def post_json(path, payload):
    req = urllib.request.Request(
        SERVER + path, data=json.dumps(payload).encode(),
        headers={'Content-Type': 'application/json'}, method='POST')
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.loads(r.read())


def load(name):
    html = open(os.path.join(FIXTURES, name),
                encoding='utf-8', errors='ignore').read()
    return page.parse_html(html)


def check_level(name):
    d = load(name)
    resp = post_json('/solve', page.to_payload(d))
    assert resp['solved'], f'server returned no solution: {resp}'
    placements = {s['shapeId']: (s['row'], s['col']) for s in resp['steps']}
    cur = [row[:] for row in d['grid']]
    for i, shape in enumerate(d['shapes']):
        r_off, c_off = placements[i]
        cur = page.apply_shape(cur, shape, r_off, c_off, d['states'])
        assert cur is not None, f'shape {i} out of bounds'
    assert page.all_goal(cur, d['goal']), 'grid not at goal'
    print(f'{name}: solved in {resp["ms"]}ms over HTTP, VERIFIED')


def walk_level(name):
    """Simulate the full gameplay loop in the game's FORCED order: place the
    active shape where the server says, re-request with the remaining
    shapes, expect cache hits, and end exactly at the goal."""
    d = load(name)
    states = d['states']
    grid = [row[:] for row in d['grid']]
    shapes = list(d['shapes'])
    hits = misses = 0
    while shapes:
        payload = page.to_payload(
            {'grid': grid, 'states': states, 'goal': d['goal'], 'shapes': shapes})
        resp = post_json('/solve', payload)
        assert resp['solved'], f'no solution with {len(shapes)} shapes left'
        if resp.get('cached'):
            hits += 1
        else:
            misses += 1
        step = next(s for s in resp['steps'] if s['shapeId'] == 0)
        grid = page.apply_shape(grid, shapes[0], step['row'], step['col'],
                                states)
        assert grid is not None, 'active placement out of bounds'
        shapes = shapes[1:]
    assert page.all_goal(grid, d['goal']), 'walk did not end at goal'
    assert misses == 1, f'expected exactly 1 fresh solve, got {misses}'
    print(f'{name}: walked to goal in forced order'
          f' ({misses} solve, {hits} cache hits)')


if __name__ == '__main__':
    with urllib.request.urlopen(SERVER + '/health', timeout=5) as r:
        assert json.loads(r.read())['status'] == 'ok'
    print('health: ok')
    check_level('sample_6x6_board_12_pieces_level_31.html')
    check_level('sample_7x8_board_18_pieces_level_48.html')
    check_level('sample_level_96.html')
    walk_level('sample_6x6_board_12_pieces_level_31.html')
    walk_level('sample_level_96.html')
    print('All server tests passed.')
