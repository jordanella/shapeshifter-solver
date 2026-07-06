// Render a synthetic Shapeshifter-shaped page from a solver JSON payload.
//
// Contains no Neopets content: it reproduces only the structural patterns
// the parsers and userscript depend on (gX/gY script vars, imgLocStr
// assignments, the bordered cycle table, ACTIVE SHAPE tables with
// square.gif cells, and per-cell board anchors with mouseon handlers,
// where non-placeable margin cells are javascript:; links).
//
// Module: renderPage(payload) -> html string
// CLI:    node render_page.js <fixture.json>   (html to stdout)

function renderPage(payload) {
    const { width, height, grid, goal, numStates, shapes } = payload;
    if (goal !== numStates - 1) {
        // parse_html defines the goal as the last distinct cycle state, so
        // any faithful page must encode it in that position
        throw new Error('payload goal must be numStates - 1');
    }
    const stateName = (i) => `st${i}`;
    const img = (name) => `<img src="//img.example/shapeshifter/${name}.gif" width="30" height="30">`;

    // Active shape bounding box determines where placement anchors exist
    const pts = shapes[0].points.map(p => [Math.floor(p / width), p % width]);
    const bboxH = Math.max(...pts.map(([r]) => r)) + 1;
    const bboxW = Math.max(...pts.map(([, c]) => c)) + 1;

    const lines = [];
    lines.push('<html><head><title>fixture</title></head><body>');
    lines.push('<a href="https://example.com/nav">a nav link</a>');
    lines.push('<form><input type="text" name="q"></form>');

    // Board state script (imgLocStr[x][y] = "state")
    lines.push('<script type="text/javascript">');
    lines.push(`gX = ${width};`);
    lines.push(`gY = ${height};`);
    lines.push(`imgLocStr = new Array(${width});`);
    for (let x = 0; x < width; x++) {
        lines.push(`imgLocStr[${x}] = new Array(${height});`);
        for (let y = 0; y < height; y++) {
            lines.push(`imgLocStr[${x}][${y}] = "${stateName(grid[y * width + x])}";`);
        }
    }
    lines.push('</script>');

    // Board: placeable cells are action links, margin cells alert links
    lines.push('<table border="0" cellpadding="0" cellspacing="0">');
    for (let y = 0; y < height; y++) {
        lines.push('<tr>');
        for (let x = 0; x < width; x++) {
            const cell = img(stateName(grid[y * width + x]));
            const hover = `onmouseover="mouseon(${x},${y})" onmouseout="mouseoff(${x},${y})"`;
            const placeable = x <= width - bboxW && y <= height - bboxH;
            const a = placeable
                ? `<a href="process_shapeshifter.phtml?type=action&amp;posx=${x}&amp;posy=${y}" ${hover}>${cell}</a>`
                : `<a href="javascript:;" onclick="alert('The whole game shape is not on the board.')" ${hover}>${cell}</a>`;
            lines.push(`<td>${a}</td>`);
        }
        lines.push('</tr>');
    }
    lines.push('</table>');

    // State cycle: s0 -> ... -> s_{k-1} -> s0 (goal is the last distinct)
    lines.push('<table border="1"><tr>');
    for (let i = 0; i < numStates; i++) {
        lines.push(`<td><img src="//img.example/shapeshifter/${stateName(i)}_0.gif"></td><td>-&gt;</td>`);
    }
    lines.push(`<td><img src="//img.example/shapeshifter/${stateName(0)}_0.gif"></td>`);
    lines.push('</tr></table>');

    // Shapes: active first, then upcoming
    lines.push('<b>ACTIVE SHAPE</b>');
    for (const shape of shapes) {
        const cells = shape.points.map(p => [Math.floor(p / width), p % width]);
        const h = Math.max(...cells.map(([r]) => r)) + 1;
        const w = Math.max(...cells.map(([, c]) => c)) + 1;
        const filled = new Set(cells.map(([r, c]) => `${r},${c}`));
        lines.push('<table border="0" cellpadding="0" cellspacing="0">');
        for (let r = 0; r < h; r++) {
            lines.push('<tr>');
            for (let c = 0; c < w; c++) {
                lines.push(`<td>${img(filled.has(`${r},${c}`) ? 'square' : 'blank')}</td>`);
            }
            lines.push('</tr>');
        }
        lines.push('</table>');
    }

    lines.push('</body></html>');
    return lines.join('\n');
}

module.exports = { renderPage };

if (require.main === module) {
    const fs = require('fs');
    const payload = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
    process.stdout.write(renderPage(payload));
}
