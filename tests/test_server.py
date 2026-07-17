"""End-to-end tests against a running solver server.

Start the server first:  solver-core serve
Then:                    python tests/test_server.py

Fixtures are JSON puzzle extractions (see tools/export_level.py) — pure
game state, no page markup.
"""

import json
import os
import threading
import time
import urllib.request

SERVER = os.environ.get('SOLVER_URL', 'http://127.0.0.1:8977')
FIXTURES = os.path.join(os.path.dirname(__file__), 'fixtures')


def post_json(path, payload):
    req = urllib.request.Request(
        SERVER + path, data=json.dumps(payload).encode(),
        headers={'Content-Type': 'application/json'}, method='POST')
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.loads(r.read())


def load(name):
    with open(os.path.join(FIXTURES, name)) as f:
        return json.load(f)


def shape_cells(payload, shape):
    w = payload['width']
    return [(p // w, p % w) for p in shape['points']]


def apply_shape(grid, payload, shape, row, col):
    w, h, k = payload['width'], payload['height'], payload['numStates']
    g = list(grid)
    for dr, dc in shape_cells(payload, shape):
        r, c = row + dr, col + dc
        assert 0 <= r < h and 0 <= c < w, 'placement out of bounds'
        g[r * w + c] = (g[r * w + c] + 1) % k
    return g


def check_level(name):
    payload = load(name)
    resp = post_json('/solve', payload)
    assert resp['solved'], f'server returned no solution: {resp}'
    placements = {s['shapeId']: (s['row'], s['col']) for s in resp['steps']}
    grid = payload['grid']
    for shape in payload['shapes']:
        row, col = placements[shape['id']]
        grid = apply_shape(grid, payload, shape, row, col)
    assert all(v == payload['goal'] for v in grid), 'grid not at goal'
    print(f'{name}: solved in {resp["ms"]}ms over HTTP, VERIFIED')


def walk_level(name):
    """Simulate the full gameplay loop in the game's FORCED order: place the
    active shape where the server says, re-request with the remaining
    shapes, expect cache hits, and end exactly at the goal."""
    payload = load(name)
    grid = payload['grid']
    shapes = list(payload['shapes'])
    hits = misses = 0
    while shapes:
        req = {
            'width': payload['width'], 'height': payload['height'],
            'grid': grid, 'goal': payload['goal'],
            'numStates': payload['numStates'],
            'shapes': [{'id': i, 'points': s['points']}
                       for i, s in enumerate(shapes)],
        }
        resp = post_json('/solve', req)
        assert resp['solved'], f'no solution with {len(shapes)} shapes left'
        if resp.get('cached'):
            hits += 1
        else:
            misses += 1
        step = next(s for s in resp['steps'] if s['shapeId'] == 0)
        grid = apply_shape(grid, payload, shapes[0], step['row'], step['col'])
        shapes = shapes[1:]
    assert all(v == payload['goal'] for v in grid), 'walk did not end at goal'
    assert misses == 1, f'expected exactly 1 fresh solve, got {misses}'
    print(f'{name}: walked to goal in forced order'
          f' ({misses} solve, {hits} cache hits)')


def cancel_on_new_board():
    """A request for a DIFFERENT board must cancel an in-flight hard solve
    and be answered promptly; the superseded request gets cancelled:true."""
    hard = load('hard_synthetic.json')
    result = {}

    def post_hard():
        try:
            result['resp'] = post_json('/solve', hard)
        except Exception as e:  # noqa: BLE001 - record for the assert below
            result['err'] = repr(e)

    th = threading.Thread(target=post_hard)
    th.start()
    time.sleep(2)  # let the hard solve get going

    t0 = time.time()
    check_level('level_31.json')  # different board: must preempt
    dt = time.time() - t0
    assert dt < 30, f'fast board waited {dt:.1f}s behind the hard solve'

    th.join(30)
    resp = result.get('resp')
    assert resp is not None and resp.get('cancelled'), \
        f'expected cancelled response for superseded solve, got {result}'
    print(f'cancel-on-new-board: hard solve cancelled,'
          f' fast board answered in {dt:.2f}s')


if __name__ == '__main__':
    with urllib.request.urlopen(SERVER + '/health', timeout=5) as r:
        assert json.loads(r.read())['status'] == 'ok'
    print('health: ok')
    check_level('level_31.json')
    check_level('level_48.json')
    check_level('level_96.json')
    walk_level('level_31.json')
    walk_level('level_96.json')
    cancel_on_new_board()
    print('All server tests passed.')
