//! Renders a byte stream through the kernel and prints the resulting screen as text.
//!
//! This exists for `tools/vt-parity`: the same corpus goes through this and through a
//! reference terminal (tmux `capture-pane`), and the two texts have to match. It is not a
//! product binary — nothing else should depend on it.

use std::io::Read;
use std::process::ExitCode;

use pigeon_core::term::alacritty::AlacrittyTerm;
use pigeon_core::term::Term;

fn main() -> ExitCode {
    let mut cols: u16 = 80;
    let mut rows: u16 = 24;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--cols" => {
                let Some(value) = args.next().and_then(|value| value.parse().ok()) else {
                    eprintln!("vt-dump: --cols needs a number");
                    return ExitCode::FAILURE;
                };
                cols = value;
            }
            "--rows" => {
                let Some(value) = args.next().and_then(|value| value.parse().ok()) else {
                    eprintln!("vt-dump: --rows needs a number");
                    return ExitCode::FAILURE;
                };
                rows = value;
            }
            other => {
                eprintln!("usage: vt-dump [--cols N] [--rows N] < stream");
                eprintln!("unknown argument: {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() {
        eprintln!("vt-dump: cannot read stdin");
        return ExitCode::FAILURE;
    }
    let mut term = AlacrittyTerm::new(cols, rows, 0);
    term.feed(&input);
    let snapshot = term.snapshot();

    let mut lines: Vec<String> = snapshot
        .screen
        .iter()
        .map(|row| {
            let text: String = row
                .iter()
                // Width 0 is the second half of a wide character; a renderer skips it, so
                // the text comparison has to as well.
                .filter(|cell| cell.width != 0)
                .map(|cell| char::from_u32(cell.codepoint).unwrap_or(' '))
                .collect();
            text.trim_end().to_owned()
        })
        .collect();
    // Trailing blank rows carry no information; internal blanks stay (they are layout).
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    for line in &lines {
        println!("{line}");
    }
    ExitCode::SUCCESS
}
