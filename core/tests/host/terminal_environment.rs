//! A pane advertises pigeon's terminal capabilities, not its starter's terminal.

use super::*;

#[test]
fn panes_do_not_inherit_an_unrelated_terminal_type() {
    let _serial = one_at_a_time();
    let program = script("#!/bin/sh\nprintf 'term=<%s> color=<%s>\\n' \"$TERM\" \"$COLORTERM\"\nexec env PS1= /bin/sh -i\n");
    for inherited in [Some("linux"), Some("dumb"), Some("xterm-kitty"), None] {
        let host = start_host_on_with_term(
            &unique_socket(),
            program.to_str().expect("script path"),
            inherited,
        );
        let mut client = Client::connect(&host.socket);
        hello(&mut client);
        let pane = attach_first(&mut client);
        let screen = stable_snapshot(&mut client, pane);
        assert!(
            screen_text(&screen).contains("term=<xterm-256color> color=<truecolor>"),
            "starter TERM={inherited:?}: {}",
            screen_text(&screen),
        );
    }
    std::fs::remove_file(program).expect("remove script");
}
