// Mirror of the userscript's parsePage(), operating on raw HTML text.
// Kept in lockstep with userscript/shapeshifter-solver.user.js — the
// parity test (test_parse_parity.py) checks this against the Python
// parser, and the DOM test uses it to know the active shape's footprint.

function parseHtml(html) {
    const gx = html.match(/gX\s*=\s*(\d+)/);
    const gy = html.match(/gY\s*=\s*(\d+)/);
    if (!gx || !gy) return null;
    const width = parseInt(gx[1], 10);
    const height = parseInt(gy[1], 10);

    const cellMap = {};
    for (const m of html.matchAll(/imgLocStr\[(\d+)\]\[(\d+)\]\s*=\s*"(\w+)"/g)) {
        cellMap[`${m[1]},${m[2]}`] = m[3];
    }
    if (Object.keys(cellMap).length !== width * height) return null;

    const activeIdx = html.indexOf('ACTIVE SHAPE');
    if (activeIdx < 0) return null;

    const cycleStart = html.indexOf('<table border="1"');
    const cycleSection = html.slice(cycleStart >= 0 ? cycleStart : 0, activeIdx);
    const states = [];
    for (const m of cycleSection.matchAll(/\/shapeshifter\/(\w+)_0\.gif/g)) {
        if (!states.includes(m[1])) states.push(m[1]);
    }
    if (states.length < 2) return null;
    const goal = states.length - 1;
    const s2i = Object.fromEntries(states.map((s, i) => [s, i]));

    const grid = [];
    for (let y = 0; y < height; y++)
        for (let x = 0; x < width; x++)
            grid.push(s2i[cellMap[`${x},${y}`]]);

    const shapesSection = html.slice(activeIdx);
    const shapes = [];
    const shapePoints = [];
    for (const tm of shapesSection.matchAll(
        /<table border="0" cellpadding="0" cellspacing="0">([\s\S]*?)<\/table>/g)) {
        const rows = tm[1].split('</tr>');
        const cells = [];
        rows.forEach((rowHtml, r) => {
            if (!rowHtml.includes('<td')) return;
            let c = 0;
            for (const td of rowHtml.matchAll(/<td[^>]*>([\s\S]*?)<\/td>/g)) {
                if (td[1].includes('square.gif')) cells.push([r, c]);
                c++;
            }
        });
        if (!cells.length) continue;
        const minR = Math.min(...cells.map(p => p[0]));
        const minC = Math.min(...cells.map(p => p[1]));
        const norm = cells.map(([r, c]) => [r - minR, c - minC]);
        shapePoints.push(norm);
        shapes.push({
            id: shapes.length,
            points: norm.map(([r, c]) => r * width + c),
        });
    }
    if (!shapes.length) return null;

    return {
        payload: { width, height, grid, goal, numStates: states.length, shapes },
        shapePoints,
    };
}

module.exports = { parseHtml };
