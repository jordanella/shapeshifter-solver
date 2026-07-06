"""Parse saved Neopets Shapeshifter pages and simulate placements.

Standalone (stdlib only). Used by the exporter and the test suite.
"""

import re


def parse_html(html: str) -> dict:
    gx = int(re.search(r'gX\s*=\s*(\d+)', html).group(1))
    gy = int(re.search(r'gY\s*=\s*(\d+)', html).group(1))

    cell_pattern = re.compile(r'imgLocStr\[(\d+)]\[(\d+)]\s*=\s*"(\w+)"')
    cell_map = {}
    for m in cell_pattern.finditer(html):
        x, y, state = int(m.group(1)), int(m.group(2)), m.group(3)
        cell_map[(x, y)] = state

    grid = []
    for y in range(gy):
        row = []
        for x in range(gx):
            row.append(cell_map[(x, y)])
        grid.append(row)

    # The cycle row displays s0 -> s1 -> ... -> s_{k-1} -> s0 (wrapping);
    # the goal is the last distinct state.
    cycle_section = html[html.index('<table border="1"'):html.index('ACTIVE SHAPE')]
    cycle_imgs = re.findall(r'/shapeshifter/(\w+)_0\.gif', cycle_section)
    states = list(dict.fromkeys(cycle_imgs))
    goal = states[-1]

    shapes_section = html[html.index('ACTIVE SHAPE'):]
    shape_tables = re.findall(
        r'<table border="0" cellpadding="0" cellspacing="0">(.*?)</table>',
        shapes_section, re.DOTALL
    )

    shapes = []
    for table_html in shape_tables:
        rows = table_html.split('</tr>')
        shape_cells = []
        for r_idx, row_html in enumerate(rows):
            if '<td' not in row_html:
                continue
            tds = re.findall(r'<td[^>]*>(.*?)</td>', row_html, re.DOTALL)
            for c_idx, td_content in enumerate(tds):
                if 'square.gif' in td_content:
                    shape_cells.append((r_idx, c_idx))
        if shape_cells:
            min_r = min(r for r, _ in shape_cells)
            min_c = min(c for _, c in shape_cells)
            shape_cells = [(r - min_r, c - min_c) for r, c in shape_cells]
            shapes.append(shape_cells)

    return {'grid': grid, 'states': states, 'goal': goal, 'shapes': shapes}


def to_payload(parsed: dict) -> dict:
    """Convert a parse_html() result to the solver-core JSON input."""
    states = parsed['states']
    s2i = {s: i for i, s in enumerate(states)}
    rows, cols = len(parsed['grid']), len(parsed['grid'][0])
    return {
        'width': cols,
        'height': rows,
        'grid': [s2i[parsed['grid'][r][c]]
                 for r in range(rows) for c in range(cols)],
        'goal': s2i[parsed['goal']],
        'numStates': len(states),
        'shapes': [
            {'id': i, 'points': [dr * cols + dc for dr, dc in sh]}
            for i, sh in enumerate(parsed['shapes'])
        ],
    }


def next_state(current: str, states: list) -> str:
    idx = states.index(current)
    return states[(idx + 1) % len(states)]


def apply_shape(grid, shape, row_off, col_off, states):
    rows, cols = len(grid), len(grid[0])
    new_grid = [row[:] for row in grid]
    for dr, dc in shape:
        r, c = row_off + dr, col_off + dc
        if r < 0 or r >= rows or c < 0 or c >= cols:
            return None
        new_grid[r][c] = next_state(new_grid[r][c], states)
    return new_grid


def all_goal(grid, goal):
    return all(cell == goal for row in grid for cell in row)
