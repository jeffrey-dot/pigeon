//! Test-only observer: OSC parsing and lifecycle stay in pigeon_core::api.

use std::io::{self, Read, Write};

use pigeon_core::api::{Config, Core};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(80, 24)?;
    // Byte-sized chunks also exercise real OSC framing across read boundaries.
    for chunk in bytes.chunks(1) {
        core.feed(pane, chunk)?;
    }
    let snapshot = core.status_snapshot(pane)?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    for record in snapshot.records {
        // All text has already been validated by core; tabs/newlines cannot occur.
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            record.id,
            record.state.as_str(),
            record.kind.as_str(),
            record.app,
            record.msg
        )?;
    }
    Ok(())
}
