use super::*;
use std::collections::BTreeMap;

fn sample_cell() -> Cell {
    Cell {
        codepoint: 'é' as u32,
        attrs: 0b101,
        fg: 0x0100_0002,
        bg: 0x0012_3456,
        link: None,
    }
}

pub(super) fn samples() -> Vec<Message> {
    vec![
        Message::Hello {
            proto: PROTO_VERSION,
            client: "pigeon-mac".into(),
            features: vec!["delta".into()],
        },
        Message::HelloAck {
            proto: PROTO_VERSION,
            server: "pigeon-server".into(),
            features: vec!["delta".into(), "scrollback".into()],
            host_id: "local".into(),
            client_id: 7,
            instance: Some("1a2b-42".into()),
        },
        Message::HelloError {
            reason: "version".into(),
            detail: "need 1.x".into(),
        },
        Message::Sessions {
            items: vec![
                SessionInfo {
                    session_id: 1,
                    title: "pigeon".into(),
                    cwd: "/tmp".into(),
                    panes: vec![1, 2],
                    active_pane: None,
                    active_title: None,
                    active_program: None,
                },
                SessionInfo {
                    session_id: 2,
                    title: "web".into(),
                    cwd: "/tmp/web".into(),
                    panes: vec![3],
                    active_pane: Some(3),
                    active_title: Some("vim".into()),
                    active_program: Some("vim".into()),
                },
            ],
        },
        Message::NewSession {
            id: 1,
            cwd: None,
            shell: None,
            env: None,
        },
        Message::NewSession {
            id: 2,
            cwd: Some("/tmp".into()),
            shell: Some("/bin/zsh".into()),
            env: Some(BTreeMap::from([(
                "PI_PROGRAM_STATUS".to_owned(),
                "1".to_owned(),
            )])),
        },
        Message::Created {
            id: 3,
            session_id: 1,
            pane_id: 1,
        },
        Message::ClosePane { id: 4, pane_id: 1 },
        Message::SplitPane {
            id: 5,
            pane_id: 1,
            axis: SplitAxis::Vertical,
            ratio: 0.5,
        },
        Message::ResizeSplit {
            id: 6,
            split_id: 2,
            ratio: 0.35,
        },
        Message::Focus { pane_id: 3 },
        Message::Layout {
            session_id: 1,
            tree: Node::Split {
                split: SplitNode {
                    id: 2,
                    axis: SplitAxis::Horizontal,
                    ratio: 0.5,
                    a: Box::new(Node::Leaf { leaf: 1 }),
                    b: Box::new(Node::Split {
                        split: SplitNode {
                            id: 3,
                            axis: SplitAxis::Vertical,
                            ratio: 0.25,
                            a: Box::new(Node::Leaf { leaf: 2 }),
                            b: Box::new(Node::Leaf { leaf: 3 }),
                        },
                    }),
                },
            },
            zoom: None,
            focus: Some(3),
        },
        Message::Attach {
            id: 5,
            pane_id: 1,
            size_policy: None,
            history: None,
            since: None,
        },
        Message::Attach {
            id: 6,
            pane_id: 1,
            size_policy: Some("latest".into()),
            history: Some(false),
            since: None,
        },
        Message::Snapshot {
            text: Vec::new(),
            pane_id: 1,
            seq: 1,
            cols: 2,
            rows: 1,
            alt: false,
            cursor: Cursor {
                x: 1,
                y: 0,
                visible: true,
                shape: 0,
            },
            scroll: Scroll { top: 0, bottom: 0 },
            screen: vec![vec![
                sample_cell(),
                Cell {
                    link: Some(9),
                    ..sample_cell()
                },
            ]],
            history: Some(vec![vec![sample_cell(), sample_cell()]]),
            links: Some(vec![Link {
                id: 9,
                uri: "https://example.com".into(),
            }]),
            history_len: Some(1000),
            offset: Some(5),
            modes: Some(0b10),
        },
        Message::Delta {
            text: Vec::new(),
            pane_id: 1,
            seq: 2,
            rows: vec![Row {
                y: 0,
                cells: vec![sample_cell()],
            }],
            cursor: Some(Cursor {
                x: 0,
                y: 0,
                visible: false,
                shape: 2,
            }),
            scroll: None,
            links: None,
            alt: Some(true),
            modes: Some(0b11),
        },
        Message::Appearance {
            fg: Some(0x0011_1111),
            bg: Some(0x00ff_ffff),
            ansi: vec![
                0x212121, 0xC30771, 0x10A778, 0xA89C14, 0x008EC4, 0x523C79, 0x20A5BA, 0xE0E0E0,
                0x212121, 0xFB007A, 0x5FD7AF, 0xF3E430, 0x20BBFC, 0x6855DE, 0x4FB8CC, 0xF1F1F1,
            ],
        },
        Message::Keys {
            pane_id: 1,
            bytes: vec![0x1b, b'[', b'A'],
        },
        Message::ResizePane {
            pane_id: 1,
            cols: 120,
            rows: 40,
        },
        Message::SizeState {
            pane_id: 1,
            cols: 120,
            rows: 40,
            source: "latest".into(),
            clients: 2,
        },
        Message::Scroll {
            pane_id: 1,
            offset: -5,
        },
        Message::ZoomPane { id: 7, pane_id: 1 },
        Message::Unzoom { id: 8, pane_id: 1 },
        Message::PaneExited {
            pane_id: 1,
            code: -1,
        },
        Message::Status {
            pane_id: 1,
            state: "blocked".into(),
            seq: 3,
            kind: Some("permission".into()),
            msg: Some("Run cargo test?".into()),
            app: Some("claude".into()),
            id: Some("main".into()),
            age_ms: Some(1500),
        },
        Message::Paste {
            pane_id: 1,
            text: "ls -la\n".into(),
        },
        Message::Ping { ts: 1 },
        Message::Shutdown,
        Message::Pong { ts: 1 },
        Message::Error {
            code: "no_such_pane".into(),
            detail: "pane 9".into(),
            id: Some(2),
        },
        Message::Unsupported {
            t_of: "split_pane".into(),
        },
    ]
}
