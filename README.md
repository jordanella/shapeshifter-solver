# shapeshifter-solver

A headless solving service for the Neopets **Shapeshifter** puzzle, with an
in-browser assistant: a userscript parses the live game page, a local Rust
server solves it, and the next placement is highlighted directly on the
board — with every other board cell locked so you can't misclick.

```
┌───────────────┐  page HTML   ┌──────────────────┐   JSON    ┌─────────────┐
│  game page    │ ──parsed──▶  │  userscript      │ ──POST──▶ │ solver-core │
│  (browser)    │ ◀─annotate── │  (Tampermonkey)  │ ◀─steps── │ serve :8977 │
└───────────────┘              └──────────────────┘           └─────────────┘
```

Because Shapeshifter forces you to place the *active* shape each turn and
reloads the page, the userscript is stateless: on every page load it
re-parses the board and asks the server. The server keeps a **trajectory
cache** — after one real solve it precomputes the expected board after each
forced-order placement, so every subsequent request during a level answers
in ~0ms. A misclicked board (which the lockdown makes hard to produce)
simply misses the cache and triggers a fresh solve of the actual position.

## Quick start

```sh
# 1. build and start the server
cd solver-core
cargo build --release
./target/release/solver-core serve

# 2. install userscript/shapeshifter-solver.user.js in Tampermonkey,
#    open Shapeshifter, and click the pulsing red cell.
```

No browser needed for one-off solves — the CLI works standalone:

```sh
solver-core level.json                    # solve an exported level
python tools/solve_page.py level.html     # solve a saved page in one step
python tools/export_level.py a.html a.json  # just the HTML -> JSON export
```

The userscript's server address (default `http://127.0.0.1:8977`) is
editable directly in the on-page status box — type the address and press
Enter or "set"; it persists in Tampermonkey storage. Hosts other than
127.0.0.1/localhost also need a matching `@connect` line in the script
header.

## The solver

The engine is a depth-first search working in *deficit space* — the grid is
held as k bitset planes, where plane v contains the cells still needing v
increments. Placing a shape rotates the planes under the shape's bitmask;
evaluating a placement is a single AND+popcount.

Pruning rests on the **toggle budget**: any solution must overshoot the
total deficit by exactly `(squares − deficits) / k` full state cycles, so
covering an already-satisfied cell burns one wrap from a fixed budget, and
a branch dies the moment its budget goes negative. If every shape places
within budget, the board is solved *by construction* — the prune and the
goal condition are the same invariant. On top of that: largest shapes are
placed first (burns budget fastest), identical shapes must choose
non-decreasing placements (symmetry breaking), and per-depth capacity masks
kill branches where a cell needs more hits than the remaining shapes can
deliver. The search parallelizes by enumerating placement prefixes into a
work queue drained by one DFS worker per core, and every solution is
re-simulated in the game's forced order before being returned.

### Benchmarks

Specific saved instances, measured on a 24-thread desktop:

| Instance | Board | This solver | Reference C-style DFS* |
|---|---|---|---|
| level 31 | 6x6, 2 states, 12 shapes | 0.013 s | 0.04 s |
| level 48 | 8x7, 3 states, 18 shapes | **0.9 s** | 14.6 s |
| level 100 | 14x14, 5 states, 36 shapes | **29 s** | 244 s |

\* the classic per-cell largest-first budget DFS (Kvho's algorithm, as
ported by Bakeru), same machine.

**These are per-instance numbers, not per-level guarantees.** Solve time
is heavy-tailed and driven by the instance's wrap budget
(`(squares − deficits) / k`) and shape mix, not the level number — the
level 96/100 fixtures above rolled unusually tight budgets. Random
instances at level-100 dimensions routinely exceed 90 s on the same
machine. Deep boards can be slow; the trajectory cache means you pay that
cost once per level, not per placement.

## The userscript

- Highlights the active shape's full footprint (green) and the cell to
  click (pulsing red) — including footprint cells on the non-placeable
  margin, which the game renders as alert links rather than action links.
- Locks the board, and only the board: every non-target cell gets
  `pointer-events: none`, a dim overlay, and a capture-phase click guard;
  the first click on the target arms a one-shot latch so a double click
  can't submit twice. Navigation and the rest of the page stay untouched.
- No auto-play: the script never clicks for you.
- Fails safe: if the server is unreachable or the parse fails, the page is
  left completely unmodified.

## Tests

```sh
# server + gameplay: start `solver-core serve` first
python tests/test_server.py        # HTTP solves + full forced-order level walks

# parser parity between the userscript (JS) and tools/ (Python)
python tests/test_parse_parity.py  # requires node

# userscript behavior in a real DOM (jsdom)
cd tests/dom && npm install && node test_userscript_dom.js
```

Fixtures under `tests/fixtures/` are JSON puzzle extractions (pure game
state — no Neopets page content ships with this repo). Page-dependent
tests render synthetic Shapeshifter-shaped pages from the JSON via
`tests/dom/render_page.js`, and the parity test round-trips
JSON -> page -> parser -> JSON for both parsers. To additionally check the
parsers against real saved captures on your machine, set `REAL_PAGES` to a
directory of them when running `test_parse_parity.py`. Puzzle data
originates from levels shipped with the MIT-licensed
[Bakeru](https://github.com/willnjohnson/Bakeru) repository.

## Building

Pure Rust, no platform-specific code: `cargo build --release` inside
`solver-core/` works as-is on macOS, Linux, and Windows. Tagging a release
(`git tag v1.0.0 && git push --tags`) builds binaries for macOS (Apple
Silicon + Intel), Windows, and Linux via GitHub Actions and attaches them
to the release; CI runs the full test suite on every push.

### Windows GNU-toolchain note

With `x86_64-pc-windows-gnu`, a stray old GCC on PATH (e.g. TDM-GCC) can
break linking (`cannot find -lgcc_eh`). Install a current mingw-w64 (e.g.
WinLibs via winget) and pin it in `solver-core/.cargo/config.toml`:

```toml
[target.x86_64-pc-windows-gnu]
linker = "C:\\path\\to\\mingw64\\bin\\gcc.exe"
```

(The MSVC toolchain, including CI's, is unaffected.)

## Credits

The core algorithm — deficit-space DFS with the toggle-budget prune — is
the classic Shapeshifter approach by **Kvho**, which reached this project
via **William N. Johnson's [Bakeru](https://github.com/willnjohnson/Bakeru)**
(MIT), whose Rust port served as the reference implementation and benchmark
baseline. The engine here is an independent implementation of that
algorithm with a bitplane state representation, capacity pruning,
parallel search, solution self-verification, the HTTP/trajectory-cache
service, and the userscript client.

## Disclaimer

This is assistance tooling for a single-player puzzle: it parses, solves, and
annotates the solution but applying it is up to you. Whether to use helpers on
Neopets is between you and the site's terms of service.

## License

MIT — see [LICENSE](LICENSE).
