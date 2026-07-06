// ==UserScript==
// @name         Neopets Shapeshifter Solver
// @namespace    shapeshifter-solver
// @version      1.6
// @description  Parses the Shapeshifter board, solves via local solver-core server, highlights where to click, and locks the board (and only the board) except the target cell.
// @match        *://www.neopets.com/medieval/shapeshifter.phtml*
// @match        *://www.neopets.com/medieval/process_shapeshifter.phtml*
// @grant        GM_xmlhttpRequest
// @grant        GM_getValue
// @grant        GM_setValue
// @connect      127.0.0.1
// @connect      localhost
// @run-at       document-idle
// ==/UserScript==

(function () {
    'use strict';

    // Server address lives in userscript storage and is editable directly
    // in the status box. Hosts other than 127.0.0.1/localhost also need a
    // matching @connect line above.
    const DEFAULT_SERVER = 'http://127.0.0.1:8977';
    const SERVER = (typeof GM_getValue === 'function')
        ? GM_getValue('server', DEFAULT_SERVER)
        : DEFAULT_SERVER;

    // ------------------------------------------------------------------
    // Status box (message + editable server address)
    // ------------------------------------------------------------------
    const box = document.createElement('div');
    box.style.cssText =
        'position:fixed;top:12px;right:12px;z-index:99999;padding:10px 14px;' +
        'background:#1e1e28;color:#eee;font:13px/1.5 monospace;border-radius:8px;' +
        'box-shadow:0 2px 12px rgba(0,0,0,.5);max-width:320px;';
    const msgEl = document.createElement('div');
    box.appendChild(msgEl);

    const cfgRow = document.createElement('div');
    cfgRow.style.cssText =
        'margin-top:6px;display:flex;gap:6px;align-items:center;';
    const cfgLabel = document.createElement('span');
    cfgLabel.textContent = 'server';
    cfgLabel.style.cssText = 'font-size:11px;color:#9a9aa5;';
    const cfgInput = document.createElement('input');
    cfgInput.type = 'text';
    cfgInput.value = SERVER;
    cfgInput.placeholder = DEFAULT_SERVER;
    cfgInput.style.cssText =
        'flex:1;min-width:170px;background:#2a2a36;color:#eee;' +
        'border:1px solid #444;border-radius:4px;font:11px monospace;' +
        'padding:2px 6px;';
    const cfgSave = document.createElement('button');
    cfgSave.textContent = 'set';
    cfgSave.style.cssText =
        'background:#3a3a48;color:#eee;border:1px solid #555;' +
        'border-radius:4px;font:11px monospace;padding:2px 8px;cursor:pointer;';
    const saveServer = () => {
        const v = cfgInput.value.trim().replace(/\/+$/, '');
        if (!v || v === SERVER) return;
        if (typeof GM_setValue === 'function') GM_setValue('server', v);
        location.reload();
    };
    cfgSave.addEventListener('click', saveServer);
    cfgInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') saveServer();
    });
    cfgRow.append(cfgLabel, cfgInput, cfgSave);
    box.appendChild(cfgRow);

    let mainMsg = '';
    let flashTimer = null;
    const setStatus = (msg, color, transient) => {
        if (!transient) mainMsg = 'Shapeshifter: ' + msg;
        msgEl.textContent = transient ? 'Shapeshifter: ' + msg : mainMsg;
        box.style.borderLeft = '5px solid ' + (color || '#888');
        if (!box.parentNode) document.body.appendChild(box);
        if (transient) {
            clearTimeout(flashTimer);
            flashTimer = setTimeout(() => { msgEl.textContent = mainMsg; }, 1400);
        }
    };

    // ------------------------------------------------------------------
    // Parse the page (same patterns as tools/shapeshifter_page.py)
    // ------------------------------------------------------------------
    function parsePage() {
        const html = document.documentElement.innerHTML;

        const gx = html.match(/gX\s*=\s*(\d+)/);
        const gy = html.match(/gY\s*=\s*(\d+)/);
        if (!gx || !gy) return null; // not a live board (won/menu page)
        const width = parseInt(gx[1], 10);
        const height = parseInt(gy[1], 10);

        const cellMap = {};
        for (const m of html.matchAll(/imgLocStr\[(\d+)\]\[(\d+)\]\s*=\s*"(\w+)"/g)) {
            cellMap[`${m[1]},${m[2]}`] = m[3]; // [x][y] = state
        }
        if (Object.keys(cellMap).length !== width * height) return null;

        const activeIdx = html.indexOf('ACTIVE SHAPE');
        if (activeIdx < 0) return null;

        // State cycle: s0 -> s1 -> ... -> s_{k-1} -> s0; goal is the last
        // distinct state
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

        // Shapes: bordered zero tables after ACTIVE SHAPE, cells with
        // square.gif, normalized to a 0-based bounding box
        const shapesSection = html.slice(activeIdx);
        const shapes = [];
        const shapePoints = []; // [row, col] pairs per shape, for overlays
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

    // ------------------------------------------------------------------
    // Board cells
    // ------------------------------------------------------------------
    // Every board cell (placeable or not) is an <a> carrying
    // onmouseover="mouseon(x,y)". Cells where the active shape can anchor
    // are process_shapeshifter action links; margin cells are javascript:;
    // alert links. The footprint must therefore be located by coordinates,
    // not by action href.
    function isGridCell(el) {
        return !!(el && el.tagName === 'A' &&
            (el.getAttribute('onmouseover') || '').startsWith('mouseon('));
    }

    function isGridAction(a) {
        if (!a || !a.href || !a.href.includes('process_shapeshifter')) return false;
        const u = new URL(a.href, location.href);
        return u.searchParams.get('type') === 'action' &&
            u.searchParams.has('posx') && u.searchParams.has('posy');
    }

    function cellElement(col, row) {
        const el = document.querySelector(`a[onmouseover="mouseon(${col},${row})"]`);
        if (el) return el;
        // fallback: scan action links by posx/posy
        for (const a of document.querySelectorAll('a[href*="process_shapeshifter"]')) {
            if (!isGridAction(a)) continue;
            const u = new URL(a.href, location.href);
            if (u.searchParams.get('posx') === String(col) &&
                u.searchParams.get('posy') === String(row)) return a;
        }
        return null;
    }

    // ------------------------------------------------------------------
    // Annotation + board lockdown
    // ------------------------------------------------------------------
    function annotate(step, activePoints) {
        const footprint = new Set();
        for (const [dr, dc] of activePoints) {
            const a = cellElement(step.col + dc, step.row + dr);
            if (!a) continue;
            footprint.add(a);
            const img = a.querySelector('img');
            if (img) {
                img.style.outline = '3px solid #46e04a';
                img.style.outlineOffset = '-3px';
                img.style.filter = 'brightness(1.25)';
            }
        }
        const target = cellElement(step.col, step.row);
        if (!isGridAction(target)) return { target: null, footprint };
        const img = target.querySelector('img');
        if (img) {
            img.style.outline = '4px solid #ff3b30';
            img.style.outlineOffset = '-4px';
            if (img.animate) img.animate(
                [{ opacity: 1 }, { opacity: 0.45 }, { opacity: 1 }],
                { duration: 900, iterations: Infinity });
        }
        return { target, footprint };
    }

    // Board-only lockdown: every grid cell except the target is unclickable
    // and dimmed (the shape preview stays bright). The first click on the
    // target arms a one-shot latch so a double click can't submit twice.
    // Everything off the board (navigation, forms) is left untouched.
    function lockdown(target, footprint) {
        let submitted = false;
        const guard = (e) => {
            const a = e.target && e.target.closest ? e.target.closest('a') : null;
            if (!isGridCell(a)) return; // off-board clicks untouched
            if (submitted) {
                e.preventDefault();
                e.stopImmediatePropagation();
                setStatus('already placed - waiting for reload', '#e6a23c', true);
                return;
            }
            if (a !== target) {
                e.preventDefault();
                e.stopImmediatePropagation();
                setStatus('locked - click the red cell', '#e6a23c', true);
                return;
            }
            submitted = true;
            setStatus('placing...', '#e6a23c');
        };
        document.addEventListener('click', guard, true);
        document.addEventListener('auxclick', guard, true);

        for (const a of document.querySelectorAll('a[onmouseover^="mouseon("]')) {
            if (a === target) continue;
            a.style.pointerEvents = 'none';
            if (footprint.has(a)) continue; // keep the shape preview bright
            const img = a.querySelector('img');
            (img || a).style.opacity = '0.55';
        }
    }

    // ------------------------------------------------------------------
    // Main
    // ------------------------------------------------------------------
    const parsed = parsePage();
    if (!parsed) return; // not a solvable board view

    const n = parsed.payload.shapes.length;
    setStatus(`solving (${n} shapes left)...`, '#e6a23c');

    GM_xmlhttpRequest({
        method: 'POST',
        url: SERVER + '/solve',
        headers: { 'Content-Type': 'application/json' },
        data: JSON.stringify(parsed.payload),
        timeout: 300000,
        onload(resp) {
            let data;
            try { data = JSON.parse(resp.responseText); }
            catch { setStatus('bad server response', '#ff3b30'); return; }
            if (!data.solved) {
                setStatus('no solution found - parse bug or unwinnable board?', '#ff3b30');
                return;
            }
            const step = data.steps.find(s => s.shapeId === 0);
            const { target, footprint } = annotate(step, parsed.shapePoints[0]);
            if (!target) {
                setStatus('solved, but no matching cell found on page', '#ff3b30');
                return; // don't lock anything if we can't point at the answer
            }
            lockdown(target, footprint);
            const src = data.cached ? 'cached' : `${data.ms}ms`;
            setStatus(
                `place at row ${step.row}, col ${step.col} (${src},` +
                ` ${n} shapes left)`, '#46e04a');
        },
        onerror() {
            setStatus(`server unreachable at ${SERVER} - run: solver-core serve`,
                '#ff3b30');
        },
        ontimeout() {
            setStatus('solver timed out', '#ff3b30');
        },
    });
})();
