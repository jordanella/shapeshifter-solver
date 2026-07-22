use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use solver_core::{SolutionStep, SolverInput};

const DEFAULT_PORT: u16 = 8977;

/// A single long-lived monitor thread prints heartbeats for whichever
/// solve is currently registered: from 5 seconds in, elapsed time and live
/// node throughput every 5 seconds. Registering a solve costs two mutex
/// ops, so fast solves pay nothing (a per-solve monitor thread measured a
/// ~250 ms latency floor on every solve).
struct HeartbeatSlot {
    t0: Instant,
    live: Arc<AtomicU64>,
    next: u64,
}

fn start_heartbeat_monitor() -> Arc<Mutex<Option<HeartbeatSlot>>> {
    let slot: Arc<Mutex<Option<HeartbeatSlot>>> = Arc::new(Mutex::new(None));
    let watched = Arc::clone(&slot);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(hb) = &mut *watched.lock().unwrap() {
            let el = hb.t0.elapsed().as_secs();
            if el >= hb.next {
                let nodes = hb.live.load(Ordering::Relaxed);
                let rate = nodes as f64 / hb.t0.elapsed().as_secs_f64() / 1e6;
                println!(
                    "  ... {el}s elapsed, {nodes} nodes ({rate:.0}M/s), still searching"
                );
                hb.next += 5;
            }
        }
    });
    slot
}

fn solve_with_heartbeat(
    input: &SolverInput,
    monitor: &Mutex<Option<HeartbeatSlot>>,
) -> (Option<Vec<SolutionStep>>, solver_core::engine::Stats) {
    let live = Arc::new(AtomicU64::new(0));
    *monitor.lock().unwrap() = Some(HeartbeatSlot {
        t0: Instant::now(),
        live: Arc::clone(&live),
        next: 5,
    });
    let r = solver_core::engine::solve_observed(input, &live);
    *monitor.lock().unwrap() = None;
    r
}

fn main() {
    let mut args = std::env::args().skip(1);
    // No arguments (e.g. double-clicking the binary) starts the server on
    // the default port; `solve <file>` / a bare path runs a one-off solve.
    let first = match args.next() {
        Some(a) => a,
        None => {
            serve(DEFAULT_PORT);
            return;
        }
    };

    if first == "serve" {
        let port = args
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        serve(port);
        return;
    }

    let data = std::fs::read_to_string(&first).expect("read input file");
    let input: SolverInput = serde_json::from_str(&data).expect("parse input JSON");
    println!(
        "solving {}x{}, {} states, {} shapes ...",
        input.height, input.width, effective_k(&input), input.shapes.len()
    );
    let monitor = start_heartbeat_monitor();
    let t0 = Instant::now();
    let (result, stats) = solve_with_heartbeat(&input, &monitor);
    let dt = t0.elapsed().as_secs_f64();
    match result {
        Some(steps) => {
            println!(
                "solved in {dt:.3}s ({} nodes, {} threads, {} prefixes)",
                stats.nodes, stats.threads, stats.prefixes
            );
            for s in steps {
                println!(
                    "shape {:2} -> row {}, col {}",
                    s.original_shape_id, s.placement_y, s.placement_x
                );
            }
        }
        None => println!("no solution ({dt:.3}s, {} nodes)", stats.nodes),
    }
}

fn effective_k(input: &SolverInput) -> usize {
    if input.num_states > 0 {
        input.num_states as usize
    } else {
        input.grid.iter().copied().max().unwrap_or(0) as usize + 1
    }
}

/// Cached solve trajectory. The game forces shape order, so after t
/// placements the board must equal grids[t] and the remaining shapes must
/// equal the suffix — if so, the cached steps answer without re-solving.
/// Any mismatch (misclick, new level) falls through to a fresh solve.
struct Cache {
    width: usize,
    height: usize,
    k: usize,
    goal: u8,
    grids: Vec<Vec<u8>>,           // grids[t] = board before placing step t
    shape_points: Vec<Vec<usize>>, // sorted normalized points, original order
    steps: Vec<SolutionStep>,      // sorted by original shape id
}

fn build_cache(input: &SolverInput, steps: &[SolutionStep]) -> Cache {
    let k = effective_k(input);
    let mut grids = Vec::with_capacity(steps.len() + 1);
    let mut g = input.grid.clone();
    grids.push(g.clone());
    for step in steps {
        let sd = input
            .shapes
            .iter()
            .find(|s| s.id == step.original_shape_id)
            .expect("step for unknown shape");
        for &pt in &sd.points {
            let ci = (pt / input.width + step.placement_y) * input.width
                + (pt % input.width + step.placement_x);
            g[ci] = ((g[ci] as usize + 1) % k) as u8;
        }
        grids.push(g.clone());
    }
    let shape_points = input
        .shapes
        .iter()
        .map(|s| {
            let mut p = s.points.clone();
            p.sort_unstable();
            p
        })
        .collect();
    Cache {
        width: input.width,
        height: input.height,
        k,
        goal: input.goal,
        grids,
        shape_points,
        steps: steps.to_vec(),
    }
}

/// If the request matches the cached trajectory at some step t, return the
/// remaining steps re-keyed so the request's active shape is id 0.
fn try_cache(cache: &Option<Cache>, input: &SolverInput) -> Option<(usize, Vec<serde_json::Value>)> {
    let c = cache.as_ref()?;
    let ns_total = c.shape_points.len();
    let rem = input.shapes.len();
    if rem == 0 || rem > ns_total {
        return None;
    }
    let t = ns_total - rem;
    if input.width != c.width
        || input.height != c.height
        || effective_k(input) != c.k
        || input.goal != c.goal
        || input.grid != c.grids[t]
    {
        return None;
    }
    for (i, sd) in input.shapes.iter().enumerate() {
        let mut pts = sd.points.clone();
        pts.sort_unstable();
        if pts != c.shape_points[t + i] {
            return None;
        }
    }
    let steps = c.steps[t..]
        .iter()
        .enumerate()
        .map(|(i, s)| {
            serde_json::json!({
                "shapeId": i,
                "row": s.placement_y,
                "col": s.placement_x,
            })
        })
        .collect();
    Some((t, steps))
}

fn cors_headers() -> Vec<tiny_http::Header> {
    [
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Methods", "POST, GET, OPTIONS"),
        ("Access-Control-Allow-Headers", "Content-Type"),
        ("Content-Type", "application/json"),
    ]
    .iter()
    .map(|(k, v)| tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap())
    .collect()
}

fn json_response(status: u32, body: String) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let mut resp = tiny_http::Response::from_string(body).with_status_code(status);
    for h in cors_headers() {
        resp.add_header(h);
    }
    resp
}

/// A solve running on a worker thread. `waiter` holds the request to
/// answer when it finishes; a re-request of the same board swaps itself in
/// there. `cancelled` distinguishes an aborted search from a genuine
/// no-solution result.
struct InFlight {
    input: SolverInput,
    waiter: Arc<Mutex<Option<tiny_http::Request>>>,
    cancelled: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

fn spawn_solve(
    input: SolverInput,
    request: tiny_http::Request,
    cache: Arc<Mutex<Option<Cache>>>,
    monitor: Arc<Mutex<Option<HeartbeatSlot>>>,
) -> InFlight {
    let waiter = Arc::new(Mutex::new(Some(request)));
    let cancelled = Arc::new(AtomicBool::new(false));
    let w = Arc::clone(&waiter);
    let c = Arc::clone(&cancelled);
    let inp = input.clone();
    let handle = std::thread::spawn(move || {
        let level = format!(
            "{}x{}, {} states, {} shapes",
            inp.height, inp.width, effective_k(&inp), inp.shapes.len()
        );
        println!("solve {level} ...");
        let t0 = Instant::now();
        let (result, stats) = solve_with_heartbeat(&inp, &monitor);
        let ms = t0.elapsed().as_millis() as u64;
        let body = match result {
            Some(steps) => {
                println!("  -> solved in {ms}ms ({} nodes)", stats.nodes);
                *cache.lock().unwrap() = Some(build_cache(&inp, &steps));
                let steps_json: Vec<_> = steps
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "shapeId": s.original_shape_id,
                            "row": s.placement_y,
                            "col": s.placement_x,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "solved": true,
                    "steps": steps_json,
                    "ms": ms,
                    "nodes": stats.nodes,
                    "cached": false,
                })
            }
            None if c.load(Ordering::Relaxed) => {
                println!("  -> cancelled after {ms}ms (superseded by a new board)");
                serde_json::json!({ "solved": false, "cancelled": true, "ms": ms })
            }
            None => {
                println!("  -> NO SOLUTION in {ms}ms");
                serde_json::json!({ "solved": false, "ms": ms, "nodes": stats.nodes })
            }
        };
        if let Some(req) = w.lock().unwrap().take() {
            let _ = req.respond(json_response(200, body.to_string()));
        }
    });
    InFlight { input, waiter, cancelled, handle }
}

fn serve(port: u16) {
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("cannot bind 127.0.0.1:{port}: {e}"));
    println!("shapeshifter solver listening on http://127.0.0.1:{port}");
    println!("  POST /solve  {{width, height, grid, goal, numStates, shapes}}");
    println!("  GET  /health");
    println!("close this window (or Ctrl+C) to stop the solver");

    let cache: Arc<Mutex<Option<Cache>>> = Arc::new(Mutex::new(None));
    let monitor = start_heartbeat_monitor();
    let mut in_flight: Option<InFlight> = None;

    for mut request in server.incoming_requests() {
        // Reap a finished worker so its slot frees up
        if in_flight.as_ref().is_some_and(|f| f.handle.is_finished()) {
            let _ = in_flight.take().unwrap().handle.join();
        }

        let url = request.url().to_string();
        let method = request.method().clone();

        match (method, url.as_str()) {
            (tiny_http::Method::Options, _) => {
                let _ = request.respond(json_response(204, String::new()));
            }
            (tiny_http::Method::Get, "/health") => {
                let busy = in_flight.is_some();
                let _ = request.respond(json_response(
                    200,
                    serde_json::json!({ "status": "ok", "solving": busy }).to_string(),
                ));
            }
            (tiny_http::Method::Post, "/solve") => {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let _ = request
                        .respond(json_response(400, r#"{"error":"unreadable body"}"#.into()));
                    continue;
                }
                let input: SolverInput = match serde_json::from_str(&body) {
                    Err(e) => {
                        let _ = request.respond(json_response(
                            400,
                            serde_json::json!({ "error": format!("bad input: {e}") })
                                .to_string(),
                        ));
                        continue;
                    }
                    Ok(i) => i,
                };

                if let Some((t, steps_json)) = try_cache(&cache.lock().unwrap(), &input) {
                    let total = input.shapes.len() + t;
                    println!("solve cache hit (step {}/{total})", t + 1);
                    let _ = request.respond(json_response(
                        200,
                        serde_json::json!({
                            "solved": true,
                            "steps": steps_json,
                            "ms": 0,
                            "cached": true,
                        })
                        .to_string(),
                    ));
                    continue;
                }

                // Same board already being solved: become its waiter (a
                // reload of the same page must not restart the search)
                if let Some(f) = &in_flight {
                    if f.input == input {
                        println!("  (same board re-requested; joining in-flight solve)");
                        let displaced = f.waiter.lock().unwrap().replace(request);
                        if let Some(d) = displaced {
                            let _ = d.respond(json_response(
                                200,
                                r#"{"solved":false,"superseded":true}"#.into(),
                            ));
                        }
                        // If the worker finished in the tiny window before the
                        // swap, nobody will answer the slot: reclaim and serve
                        // from the cache the worker just wrote.
                        if f.handle.is_finished() {
                            if let Some(req) = f.waiter.lock().unwrap().take() {
                                let resp = match try_cache(&cache.lock().unwrap(), &input) {
                                    Some((_, steps_json)) => serde_json::json!({
                                        "solved": true,
                                        "steps": steps_json,
                                        "ms": 0,
                                        "cached": true,
                                    }),
                                    None => serde_json::json!({ "solved": false }),
                                };
                                let _ = req.respond(json_response(200, resp.to_string()));
                            }
                        }
                        continue;
                    }
                    // Different board: cancel the in-flight solve and restart.
                    // The game rerolls the level on restart, so the player
                    // deliberately abandoning a struggling board should not
                    // wait behind its search.
                    println!("  -> new board received; cancelling in-flight solve");
                    f.cancelled.store(true, Ordering::Relaxed);
                    solver_core::CANCEL_FLAG.store(true, Ordering::Relaxed);
                    let f = in_flight.take().unwrap();
                    let _ = f.handle.join();
                    solver_core::CANCEL_FLAG.store(false, Ordering::Relaxed);
                }

                in_flight = Some(spawn_solve(
                    input,
                    request,
                    Arc::clone(&cache),
                    Arc::clone(&monitor),
                ));
            }
            _ => {
                let _ = request.respond(json_response(404, r#"{"error":"not found"}"#.into()));
            }
        }
    }
}
