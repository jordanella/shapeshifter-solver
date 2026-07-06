//! Bitplane DFS engine.
//!
//! Works in deficit space: the grid becomes k bitset planes, where
//! `plane[v]` holds the cells that still need v more increments. Placing a
//! shape rotates the planes down by one under the shape's mask — no
//! per-cell loops — and states are small word arrays, cheap to snapshot
//! per depth (no undo pass).
//!
//! Pruning rests on the toggle budget: a solution must overshoot the total
//! deficit by exactly (squares − deficits) / k full cycles, so covering an
//! already-satisfied cell burns one wrap from a fixed budget, and the
//! budget cost of a placement is a single popcount. If every shape places
//! within budget the board is solved by construction — the prune and the
//! goal condition are the same invariant. Shapes go largest-first (burns
//! budget fastest), identical shapes must pick non-decreasing placements,
//! and per-depth capacity masks catch local starvation (a cell needing
//! more hits than the remaining shapes can deliver).
//!
//! Parallelism: placement prefixes are enumerated to a depth with enough
//! fan-out, then worker threads drain them from a shared queue, each
//! running an independent DFS below its prefix.
//!
//! Every returned solution is re-simulated against the input (in original
//! shape order — the order the game forces) before being returned.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::{SolutionStep, SolverInput, CANCEL_FLAG};

#[derive(Default)]
pub struct Stats {
    pub nodes: u64,
    pub cap_prunes: u64,
    pub threads: usize,
    pub prefixes: usize,
}

pub fn solve(input: &SolverInput) -> (Option<Vec<SolutionStep>>, Stats) {
    let cells = input.width * input.height;
    match cells.div_ceil(64) {
        1 => run::<1>(input),
        2 => run::<2>(input),
        3 => run::<3>(input),
        4 => run::<4>(input),
        _ => panic!("grid larger than 256 cells"),
    }
}

struct ShapeInfo<const W: usize> {
    original_id: usize,
    ax: usize,
    masks: Vec<[u64; W]>,
    equivalent_to: Option<usize>,
}

struct Ctx<const W: usize> {
    k: usize,
    ns: usize,
    shapes: Vec<ShapeInfo<W>>,
    cap_lt: Vec<Vec<[u64; W]>>,
    cap_active: Vec<bool>,
}

/// A search state snapshot at a fixed depth, used as a parallel work item.
struct Prefix<const W: usize> {
    planes: Vec<[u64; W]>,
    bud: i64,
    seq: Vec<usize>,
}

#[inline(always)]
fn and_popcount<const W: usize>(a: &[u64; W], b: &[u64; W]) -> u32 {
    let mut t = 0;
    for w in 0..W {
        t += (a[w] & b[w]).count_ones();
    }
    t
}

#[inline(always)]
fn intersects<const W: usize>(a: &[u64; W], b: &[u64; W]) -> bool {
    let mut t = 0;
    for w in 0..W {
        t |= a[w] & b[w];
    }
    t != 0
}

fn effective_k(input: &SolverInput) -> usize {
    if input.num_states > 0 {
        input.num_states as usize
    } else {
        input.grid.iter().copied().max().unwrap_or(0) as usize + 1
    }
}

fn build_ctx<const W: usize>(input: &SolverInput, order: &[usize], k: usize) -> Ctx<W> {
    let x = input.width;
    let y = input.height;
    let ns = order.len();

    let mut shapes: Vec<ShapeInfo<W>> = Vec::with_capacity(ns);
    let mut seen: Vec<(Vec<usize>, usize)> = Vec::new(); // (sorted points, depth)
    for (idx, &oi) in order.iter().enumerate() {
        let sd = &input.shapes[oi];
        let mut pts = sd.points.clone();
        pts.sort_unstable();
        let max_x = pts.iter().map(|p| p % x).max().unwrap();
        let max_y = pts.iter().map(|p| p / x).max().unwrap();
        let ax = x - max_x;
        let ay = y - max_y;
        let mut masks = Vec::with_capacity(ax * ay);
        for dy in 0..ay {
            for dx in 0..ax {
                let mut m = [0u64; W];
                for &pt in &pts {
                    let ci = (pt / x + dy) * x + (pt % x + dx);
                    m[ci / 64] |= 1u64 << (ci % 64);
                }
                masks.push(m);
            }
        }
        let mut eq_to = None;
        for (prev_pts, prev_idx) in seen.iter().rev() {
            if *prev_pts == pts {
                eq_to = Some(*prev_idx);
                break;
            }
        }
        seen.push((pts, idx));
        shapes.push(ShapeInfo { original_id: sd.id, ax, masks, equivalent_to: eq_to });
    }

    // Capacity masks: cap_lt[i][v] = cells that fewer than v of the shapes
    // at depths i.. can reach; a cell with deficit v there is starved.
    // cap_active gates the check off while every cell has full capacity.
    let mut cap_lt: Vec<Vec<[u64; W]>> = vec![vec![[0u64; W]; k]; ns];
    let mut cap_active = vec![false; ns];
    {
        let mut counts = vec![0u32; x * y];
        for i in (0..ns).rev() {
            let mut reach = [0u64; W];
            for m in &shapes[i].masks {
                for w in 0..W {
                    reach[w] |= m[w];
                }
            }
            for (ci, count) in counts.iter_mut().enumerate() {
                if reach[ci / 64] >> (ci % 64) & 1 == 1 {
                    *count += 1;
                }
            }
            for v in 1..k {
                for (ci, count) in counts.iter().enumerate() {
                    if (*count as usize) < v {
                        cap_lt[i][v][ci / 64] |= 1u64 << (ci % 64);
                        cap_active[i] = true;
                    }
                }
            }
        }
    }

    Ctx { k, ns, shapes, cap_lt, cap_active }
}

fn run<const W: usize>(input: &SolverInput) -> (Option<Vec<SolutionStep>>, Stats) {
    let k = effective_k(input);
    let goal = input.goal as usize;
    let ns = input.shapes.len();
    let mut stats = Stats { threads: 1, ..Stats::default() };
    if ns == 0 {
        return (None, stats);
    }

    // Deficit planes: plane[v] = cells needing v more increments
    let mut planes0 = vec![[0u64; W]; k];
    let mut total_deficit: i64 = 0;
    for (ci, &v) in input.grid.iter().enumerate() {
        let d = (goal + k - v as usize) % k;
        planes0[d][ci / 64] |= 1u64 << (ci % 64);
        total_deficit += d as i64;
    }
    let total_squares: i64 = input.shapes.iter().map(|s| s.points.len() as i64).sum();
    let overshoot = total_squares - total_deficit;
    if overshoot < 0 || overshoot % k as i64 != 0 {
        return (None, stats); // infeasible: budget must be a whole number of wraps
    }
    let budget0 = overshoot / k as i64;

    // Largest shapes first
    let canonical: Vec<usize> = {
        let mut o: Vec<usize> = (0..ns).collect();
        o.sort_by_key(|&i| std::cmp::Reverse(input.shapes[i].points.len()));
        o
    };
    let ctx = build_ctx::<W>(input, &canonical, k);

    let n_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);

    if n_threads == 1 || ns < 2 {
        let mut planes = vec![vec![[0u64; W]; k]; ns + 1];
        planes[0].copy_from_slice(&planes0);
        let mut bud = vec![0i64; ns + 1];
        bud[0] = budget0;
        let mut seq = vec![0usize; ns];
        let stop = AtomicBool::new(false);
        let found = dfs(&ctx, &mut planes, &mut bud, &mut seq, 0, &mut stats, &stop);
        let result = found.then(|| build_steps(&ctx, &seq));
        return (check_solution(input, result, k), stats);
    }

    // Enumerate prefixes deep enough for decent fan-out
    let target = n_threads * 8;
    let mut depth = 1;
    let mut prefixes = enum_prefixes(&ctx, &planes0, budget0, depth);
    while prefixes.len() < target && depth < ns - 1 && !prefixes.is_empty() {
        depth += 1;
        prefixes = enum_prefixes(&ctx, &planes0, budget0, depth);
    }
    stats.threads = n_threads;
    stats.prefixes = prefixes.len();
    if prefixes.is_empty() {
        return (None, stats);
    }

    let next_prefix = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let solution: Mutex<Option<Vec<usize>>> = Mutex::new(None);
    let nodes_total = AtomicU64::new(0);
    let cap_total = AtomicU64::new(0);

    std::thread::scope(|scope| {
        for _ in 0..n_threads {
            scope.spawn(|| {
                let mut planes = vec![vec![[0u64; W]; ctx.k]; ctx.ns + 1];
                let mut bud = vec![0i64; ctx.ns + 1];
                let mut seq = vec![0usize; ctx.ns];
                let mut local = Stats::default();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let idx = next_prefix.fetch_add(1, Ordering::Relaxed);
                    if idx >= prefixes.len() {
                        break;
                    }
                    let p = &prefixes[idx];
                    planes[depth].copy_from_slice(&p.planes);
                    bud[depth] = p.bud;
                    seq[..depth].copy_from_slice(&p.seq);
                    if dfs(&ctx, &mut planes, &mut bud, &mut seq, depth, &mut local, &stop) {
                        *solution.lock().unwrap() = Some(seq.clone());
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                nodes_total.fetch_add(local.nodes, Ordering::Relaxed);
                cap_total.fetch_add(local.cap_prunes, Ordering::Relaxed);
            });
        }
    });

    stats.nodes = nodes_total.load(Ordering::Relaxed);
    stats.cap_prunes = cap_total.load(Ordering::Relaxed);
    let result = solution.into_inner().unwrap().map(|s| build_steps(&ctx, &s));
    (check_solution(input, result, k), stats)
}

/// Enumerate all budget-feasible placement prefixes of the first `depth`
/// shapes, returning the search state under each.
fn enum_prefixes<const W: usize>(
    ctx: &Ctx<W>,
    planes0: &[[u64; W]],
    budget0: i64,
    depth: usize,
) -> Vec<Prefix<W>> {
    let k = ctx.k;
    let mut out = Vec::new();
    let mut planes = vec![vec![[0u64; W]; k]; depth + 1];
    planes[0].copy_from_slice(planes0);
    let mut bud = vec![0i64; depth + 1];
    bud[0] = budget0;
    let mut seq = vec![0usize; depth];

    let mut i = 0usize;
    loop {
        if i == depth {
            out.push(Prefix {
                planes: planes[depth].clone(),
                bud: bud[depth],
                seq: seq.clone(),
            });
            i -= 1;
            seq[i] += 1;
            continue;
        }
        let (below, above) = planes.split_at_mut(i + 1);
        let cur = &below[i];
        let next = &mut above[0];
        let masks = &ctx.shapes[i].masks;
        let mut s = seq[i];
        let mut placed = false;
        while s < masks.len() {
            let m = &masks[s];
            let ti = bud[i] - and_popcount(&cur[0], m) as i64;
            if ti >= 0 {
                for v in 0..k {
                    let nxt = if v + 1 == k { 0 } else { v + 1 };
                    for w in 0..W {
                        next[v][w] = (cur[v][w] & !m[w]) | (cur[nxt][w] & m[w]);
                    }
                }
                seq[i] = s;
                bud[i + 1] = ti;
                placed = true;
                break;
            }
            s += 1;
        }
        if placed {
            i += 1;
            if i < depth {
                seq[i] = ctx.shapes[i].equivalent_to.map(|e| seq[e]).unwrap_or(0);
            }
            continue;
        }
        if i == 0 {
            return out;
        }
        i -= 1;
        seq[i] += 1;
    }
}

/// DFS from `floor` to the last shape. On success returns true with the
/// full placement in `seq`. Backtracking never rises above `floor`.
fn dfs<const W: usize>(
    ctx: &Ctx<W>,
    planes: &mut [Vec<[u64; W]>],
    bud: &mut [i64],
    seq: &mut [usize],
    floor: usize,
    stats: &mut Stats,
    stop: &AtomicBool,
) -> bool {
    let k = ctx.k;
    let ns = ctx.ns;
    let mut i = floor;
    let mut arrived = true;
    if i < ns {
        seq[i] = ctx.shapes[i].equivalent_to.map(|e| seq[e]).unwrap_or(0);
    }

    loop {
        stats.nodes += 1;
        if stats.nodes & 4095 == 0
            && (CANCEL_FLAG.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed))
        {
            return false;
        }

        let mut dead = false;
        if arrived && ctx.cap_active[i] {
            for v in 1..k {
                if intersects(&planes[i][v], &ctx.cap_lt[i][v]) {
                    stats.cap_prunes += 1;
                    dead = true;
                    break;
                }
            }
        }

        if !dead {
            let (below, above) = planes.split_at_mut(i + 1);
            let cur = &below[i];
            let next = &mut above[0];
            let masks = &ctx.shapes[i].masks;
            let budget_i = bud[i];
            let mut s = seq[i];
            let mut placed = false;

            while s < masks.len() {
                let m = &masks[s];
                let ti = budget_i - and_popcount(&cur[0], m) as i64;
                if ti >= 0 {
                    // decrement deficits mod k under the mask:
                    // new[v] = old[v] outside mask, old[v+1 mod k] inside
                    for v in 0..k {
                        let nxt = if v + 1 == k { 0 } else { v + 1 };
                        for w in 0..W {
                            next[v][w] = (cur[v][w] & !m[w]) | (cur[nxt][w] & m[w]);
                        }
                    }
                    seq[i] = s;
                    bud[i + 1] = ti;
                    placed = true;
                    break;
                }
                s += 1;
            }

            if placed {
                if i == ns - 1 {
                    return true;
                }
                i += 1;
                seq[i] = ctx.shapes[i].equivalent_to.map(|e| seq[e]).unwrap_or(0);
                arrived = true;
                continue;
            }
        }

        if i == floor {
            return false;
        }
        arrived = false;
        i -= 1;
        seq[i] += 1;
    }
}

/// Belt and braces: simulate the placements against the input, in original
/// shape order (the order the game forces), and drop the solution if it
/// doesn't reach the goal.
fn check_solution(
    input: &SolverInput,
    result: Option<Vec<SolutionStep>>,
    k: usize,
) -> Option<Vec<SolutionStep>> {
    let steps = result?;
    let x = input.width;
    let mut grid: Vec<usize> = input.grid.iter().map(|&v| v as usize).collect();
    for step in &steps {
        let sd = input.shapes.iter().find(|s| s.id == step.original_shape_id)?;
        for &pt in &sd.points {
            let ci = (pt / x + step.placement_y) * x + (pt % x + step.placement_x);
            grid[ci] = (grid[ci] + 1) % k;
        }
    }
    if grid.iter().all(|&v| v == input.goal as usize) {
        Some(steps)
    } else {
        eprintln!("solver bug: solution failed self-verification, discarding");
        None
    }
}

fn build_steps<const W: usize>(ctx: &Ctx<W>, seq: &[usize]) -> Vec<SolutionStep> {
    let mut steps: Vec<SolutionStep> = ctx
        .shapes
        .iter()
        .enumerate()
        .map(|(idx, sh)| SolutionStep {
            original_shape_id: sh.original_id,
            placement_x: seq[idx] % sh.ax,
            placement_y: seq[idx] / sh.ax,
        })
        .collect();
    steps.sort_by_key(|s| s.original_shape_id);
    steps
}
