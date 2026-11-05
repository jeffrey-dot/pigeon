#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use pigeon_core::api::{self, Message, PgError};
use std::collections::BTreeMap;

#[test]
fn environment_budgets_and_portable_names_are_validated() {
    let valid = BTreeMap::from([
        ("_X2".into(), "你好=\n".into()),
        ("EMPTY".into(), "".into()),
    ]);
    assert_eq!(api::validate_session_environment(&valid), Ok(()));
    for (name, value) in [
        ("", "v"),
        ("2X", "v"),
        ("x=y", "v"),
        ("é", "v"),
        ("x", "a\0b"),
        ("TERM", "x"),
        ("COLORTERM", "x"),
        ("PI_PROGRAM_STATUS", "0"),
    ] {
        assert_eq!(
            api::validate_session_environment(&BTreeMap::from([(name.into(), value.into())])),
            Err(PgError::InvalidArg)
        );
    }
    let count = (0..129).map(|n| (format!("X{n}"), "".into())).collect();
    assert_eq!(
        api::validate_session_environment(&count),
        Err(PgError::InvalidArg)
    );
    let count = (0..128).map(|n| (format!("X{n}"), "".into())).collect();
    assert_eq!(api::validate_session_environment(&count), Ok(()));
    let value_limit = BTreeMap::from([("X".into(), "a".repeat(32768))]);
    assert_eq!(api::validate_session_environment(&value_limit), Ok(()));
    let large = BTreeMap::from([("X".into(), "a".repeat(32769))]);
    assert_eq!(
        api::validate_session_environment(&large),
        Err(PgError::InvalidArg)
    );
    let total = BTreeMap::from([
        ("X".into(), "a".repeat(32768)),
        ("Y".into(), "a".repeat(32768)),
    ]);
    assert_eq!(
        api::validate_session_environment(&total),
        Err(PgError::InvalidArg)
    );
}

#[test]
fn environment_builder_is_atomic_and_roundtrips() {
    let mut message = Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    };
    api::set_session_env(&mut message, "X", "first").unwrap();
    api::set_session_env(&mut message, "X", "你好").unwrap();
    let old = message.clone();
    assert_eq!(
        api::set_session_env(&mut message, "BAD=NAME", "v"),
        Err(PgError::InvalidArg)
    );
    assert_eq!(message, old);
    assert_eq!(
        api::decode_frame(&api::encode_frame(&message).unwrap())
            .unwrap()
            .unwrap()
            .0,
        old
    );
    assert_eq!(
        api::set_session_env(&mut Message::Ping { ts: 1 }, "X", "v"),
        Err(PgError::InvalidArg)
    );
}
