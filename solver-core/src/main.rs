use std::time::Instant;

use solver_core::{SolutionStep, SolverInput};

const DEFAULT_PORT: u16 = 8977;

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .expect("usage: solver-core <input.json> | solver-core serve [port]");

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
    let t0 = Instant::now();
    let (result, stats) = solver_core::engine::solve(&input);
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

fn serve(port: u16) {
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("cannot bind 127.0.0.1:{port}: {e}"));
    println!("shapeshifter solver listening on http://127.0.0.1:{port}");
    println!("  POST /solve  {{width, height, grid, goal, numStates, shapes}}");
    println!("  GET  /health");

    let mut cache: Option<Cache> = None;
    for mut request in server.incoming_requests() {
        let url = request.url().to_string();
        let method = request.method().clone();

        let response = match (method, url.as_str()) {
            (tiny_http::Method::Options, _) => json_response(204, String::new()),
            (tiny_http::Method::Get, "/health") => {
                json_response(200, r#"{"status":"ok"}"#.to_string())
            }
            (tiny_http::Method::Post, "/solve") => {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    json_response(400, r#"{"error":"unreadable body"}"#.to_string())
                } else {
                    match serde_json::from_str::<SolverInput>(&body) {
                        Err(e) => json_response(
                            400,
                            serde_json::json!({ "error": format!("bad input: {e}") })
                                .to_string(),
                        ),
                        Ok(input) => {
                            let level = format!(
                                "{}x{}, {} states, {} shapes",
                                input.height, input.width, effective_k(&input),
                                input.shapes.len()
                            );
                            if let Some((t, steps_json)) = try_cache(&cache, &input) {
                                let total = cache.as_ref().unwrap().steps.len();
                                println!("solve {level} ... cache hit (step {}/{total})", t + 1);
                                json_response(
                                    200,
                                    serde_json::json!({
                                        "solved": true,
                                        "steps": steps_json,
                                        "ms": 0,
                                        "cached": true,
                                    })
                                    .to_string(),
                                )
                            } else {
                                print!("solve {level} ... ");
                                let t0 = Instant::now();
                                let (result, stats) = solver_core::engine::solve(&input);
                                let ms = t0.elapsed().as_millis() as u64;
                                match result {
                                    Some(steps) => {
                                        println!("solved in {ms}ms ({} nodes)", stats.nodes);
                                        cache = Some(build_cache(&input, &steps));
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
                                        json_response(
                                            200,
                                            serde_json::json!({
                                                "solved": true,
                                                "steps": steps_json,
                                                "ms": ms,
                                                "nodes": stats.nodes,
                                                "cached": false,
                                            })
                                            .to_string(),
                                        )
                                    }
                                    None => {
                                        println!("NO SOLUTION in {ms}ms");
                                        json_response(
                                            200,
                                            serde_json::json!({
                                                "solved": false,
                                                "ms": ms,
                                                "nodes": stats.nodes,
                                            })
                                            .to_string(),
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => json_response(404, r#"{"error":"not found"}"#.to_string()),
        };

        let _ = request.respond(response);
    }
}
