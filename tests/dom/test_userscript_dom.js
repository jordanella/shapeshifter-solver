// DOM-level test of the userscript against a saved level page and the
// live solver server: verifies the full footprint is highlighted (anchor
// and margin cells alike), the board lockdown, the duplicate-submission
// latch, and that off-board elements are untouched.
// Usage: node test_userscript_dom.js   (server must be on 127.0.0.1:8977)

const fs = require('fs');
const http = require('http');
const path = require('path');
const { JSDOM, VirtualConsole } = require('jsdom');
const { renderPage } = require('./render_page');

const FIXTURE = path.join(__dirname, '..', 'fixtures', 'level_31.json');
const USERSCRIPT = path.join(__dirname, '..', '..', 'userscript',
    'shapeshifter-solver.user.js');

const payload = JSON.parse(fs.readFileSync(FIXTURE, 'utf8'));
const html = renderPage(payload);
const script = fs.readFileSync(USERSCRIPT, 'utf8');
const shapePoints = payload.shapes.map(s =>
    s.points.map(p => [Math.floor(p / payload.width), p % payload.width]));

const vc = new VirtualConsole(); // swallow jsdom "not implemented: navigation"
const dom = new JSDOM(html, {
    url: 'https://www.neopets.com/medieval/shapeshifter.phtml',
    runScripts: 'outside-only',
    virtualConsole: vc,
});
const { window } = dom;

const SERVER = process.env.SOLVER_URL || 'http://127.0.0.1:8977';

let solveResponse = null;
window.GM_xmlhttpRequest = (opts) => {
    const req = http.request(SERVER + '/solve', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
    }, (res) => {
        let body = '';
        res.on('data', (c) => { body += c; });
        res.on('end', () => {
            solveResponse = JSON.parse(body);
            opts.onload({ responseText: body });
            setTimeout(runAssertions, 50);
        });
    });
    req.on('error', (e) => {
        console.error('server unreachable:', e.message);
        process.exit(1);
    });
    req.end(opts.data);
};

window.eval(script);

const isGridCell = (a) =>
    a && (a.getAttribute('onmouseover') || '').startsWith('mouseon(');

function cellAt(col, row) {
    return window.document.querySelector(
        `a[onmouseover="mouseon(${col},${row})"]`);
}

function clickResult(el) {
    // dispatchEvent returns false iff preventDefault was called
    const ev = new window.MouseEvent('click', { bubbles: true, cancelable: true });
    return el.dispatchEvent(ev);
}

function runAssertions() {
    const assert = (cond, msg) => {
        if (!cond) { console.error('FAIL:', msg); process.exit(1); }
        console.log('ok:', msg);
    };

    assert(solveResponse && solveResponse.solved, 'server solved the level');
    const step = solveResponse.steps.find(s => s.shapeId === 0);

    const boxText = [...window.document.querySelectorAll('div')]
        .map(d => d.textContent).find(t => t && t.includes('Shapeshifter:'));
    assert(boxText && boxText.includes(`row ${step.row}, col ${step.col}`),
        `status box shows placement (row ${step.row}, col ${step.col})`);

    // Budget forecast: independently recompute (squares - deficits) / k
    const squares = payload.shapes.reduce((t, s) => t + s.points.length, 0);
    const deficit = payload.grid.reduce(
        (t, v) => t + (((payload.goal - v) % payload.numStates) + payload.numStates)
            % payload.numStates, 0);
    const budget = (squares - deficit) / payload.numStates;
    const infoText = [...window.document.querySelectorAll('div')]
        .map(d => d.textContent).find(t => t && t.includes('wraps'));
    assert(infoText && infoText.includes(`budget ${budget} wraps`),
        `status box shows wrap budget (${budget})`);

    const target = cellAt(step.col, step.row);
    assert(target, 'target cell exists');
    assert(target.href.includes('process_shapeshifter'),
        'target cell is a real action link');

    // EVERY footprint cell must be highlighted: green, or red for the
    // target itself. This covers margin (javascript:;) cells too.
    let marginGreens = 0;
    for (const [dr, dc] of shapePoints[0]) {
        const cell = cellAt(step.col + dc, step.row + dr);
        assert(cell, `footprint cell (${step.row + dr},${step.col + dc}) exists`);
        const img = cell.querySelector('img');
        const outline = img ? img.style.outline : '';
        assert(outline.includes(cell === target ? '4px' : '3px'),
            `footprint cell (${step.row + dr},${step.col + dc}) highlighted`);
        if (cell !== target && cell.href.startsWith('javascript')) marginGreens++;
    }
    console.log(`   (${marginGreens} footprint cells lie on non-placeable margin)`);

    // Non-footprint board cells: dimmed and blocked, action or margin alike
    let wrongAction = null, wrongMargin = null;
    for (const a of window.document.querySelectorAll('a[onmouseover]')) {
        if (!isGridCell(a) || a === target) continue;
        const img = a.querySelector('img');
        if (img && img.style.outline.includes('3px')) continue; // footprint
        if (a.href.includes('process_shapeshifter') && !wrongAction) wrongAction = a;
        if (a.href.startsWith('javascript') && !wrongMargin) wrongMargin = a;
    }
    for (const [label, cell] of [['placeable-but-wrong', wrongAction],
                                 ['margin', wrongMargin]]) {
        assert(cell, `found a ${label} cell`);
        assert(cell.style.pointerEvents === 'none',
            `${label} cell has pointer-events: none`);
        assert((cell.querySelector('img') || cell).style.opacity === '0.55',
            `${label} cell is dimmed`);
        assert(clickResult(cell) === false, `click on ${label} cell is blocked`);
    }

    // Off-board elements must be completely untouched
    let navLink = null;
    for (const a of window.document.querySelectorAll('a[href]')) {
        if (!isGridCell(a) && a.href.startsWith('http') &&
            !a.href.includes('process_shapeshifter')) { navLink = a; break; }
    }
    assert(navLink, 'found a non-grid nav link');
    assert(navLink.style.pointerEvents !== 'none', 'nav link NOT disabled');
    assert((navLink.querySelector('img') || navLink).style.opacity !== '0.55',
        'nav link NOT dimmed');
    assert(clickResult(navLink) === true, 'click on nav link is allowed');
    const formControl = window.document.querySelector('input, button, select');
    if (formControl) {
        assert(formControl.style.pointerEvents !== 'none',
            `form control <${formControl.tagName.toLowerCase()}> untouched`);
    }

    assert(clickResult(target) === true, 'first click on target is allowed');
    assert(clickResult(target) === false,
        'second click on target is blocked (duplicate latch)');
    if (wrongAction) {
        assert(clickResult(wrongAction) === false,
            'wrong cell still blocked after submission');
    }
    assert(clickResult(navLink) === true, 'nav link still works after submission');

    console.log('All DOM lockdown tests passed.');
    process.exit(0);
}
