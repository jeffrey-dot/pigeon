mod client;

use client::Client;
use pigeon_core::api::Message;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let socket = args.get(1).ok_or("socket required")?;
    if args.get(2).is_some_and(|arg| arg == "--shutdown") {
        return Client::connect(socket)?.send(&Message::Shutdown);
    }
    let samples = number(&args, 2, 1, 10000)?;
    let pane_count = number(&args, 3, 1, 16)?;
    let bytes = number(&args, 4, 1024, 64 * 1024 * 1024)?;
    let start = Instant::now();
    let mut client = Client::connect(socket)?;
    let handshake_us = start.elapsed().as_secs_f64() * 1e6;
    let mut panes = Vec::new();
    let mut ready_us = Vec::new();
    // Independent sessions keep every pane at 80x24 instead of shrinking a split tree.
    for id in 1..=pane_count as u32 {
        let start = Instant::now();
        client.send(&Message::NewSession {
            id,
            cwd: None,
            shell: None,
            env: None,
        })?;
        let message =
            client.wait(|m| matches!(m, Message::Created { id: got, .. } if *got == id))?;
        let Message::Created { pane_id, .. } = message else {
            return Err("missing pane".into());
        };
        client.attach(pane_id)?;
        client.wait_line(pane_id, "READY")?;
        panes.push(pane_id);
        ready_us.push(start.elapsed().as_secs_f64() * 1e6);
    }
    let pane = *panes.first().ok_or("no probe pane")?;
    let mut echo_us = Vec::new();
    for sequence in 1..=samples + 5 {
        let start = Instant::now();
        client.keys(pane, &format!("E {sequence}\n"))?;
        client.wait_line(pane, &format!("ACK:{sequence}:{sequence}"))?;
        if sequence > 5 {
            echo_us.push(start.elapsed().as_secs_f64() * 1e6);
        }
    }
    let before = client.screens.clone();
    let start = Instant::now();
    drop(client);
    let mut client = Client::connect(socket)?;
    for &pane in &panes {
        client.attach(pane)?;
    }
    let recovery_us = start.elapsed().as_secs_f64() * 1e6;
    if client.screens != before {
        return Err("reconnect changed a quiescent screen or sequence".into());
    }

    let initial_wire = client.wire_bytes;
    let initial_snapshots = client.snapshots;
    let initial_deltas = client.deltas;
    let initial_stale_deltas = client.stale_deltas;
    let start = Instant::now();
    for &pane in &panes {
        client.keys(pane, &format!("B {bytes}\n"))?;
    }
    let marker = format!("DONE:{bytes}");
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut completions = vec![None; panes.len()];
    while completions.iter().any(Option::is_none) {
        client.next(deadline)?;
        for (&pane, completion) in panes.iter().zip(&mut completions) {
            if completion.is_none() && client.has_line(pane, &marker) {
                *completion = Some(start.elapsed().as_secs_f64() * 1e6);
            }
        }
    }
    let burst_us = start.elapsed().as_secs_f64() * 1e6;
    let completion_us: Vec<_> = completions
        .into_iter()
        .collect::<Option<_>>()
        .ok_or("missing completion")?;
    println!("{{\"schema\":1,\"grid\":[80,24],\"panes\":{pane_count},\"warmup\":5,\"samples\":{samples},\"handshake_us\":{handshake_us},\"ready_us\":{ready_us:?},\"echo_us\":{echo_us:?},\"recovery_us\":{recovery_us},\"recovery_equal\":true,\"burst_bytes_per_pane\":{bytes},\"burst_us\":{burst_us},\"burst_completion_us\":{completion_us:?},\"burst_wire_bytes\":{},\"burst_snapshots\":{},\"burst_deltas\":{},\"burst_stale_deltas\":{}}}", client.wire_bytes - initial_wire, client.snapshots - initial_snapshots, client.deltas - initial_deltas, client.stale_deltas - initial_stale_deltas);
    client.send(&Message::Shutdown)?;
    Ok(())
}

fn number(args: &[String], index: usize, low: usize, high: usize) -> Result<usize> {
    let value: usize = args.get(index).ok_or("missing numeric argument")?.parse()?;
    if !(low..=high).contains(&value) {
        return Err(format!("argument {index} outside {low}..={high}").into());
    }
    Ok(value)
}
