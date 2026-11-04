use super::*;

// A fresh subprocess isolates RSS from the other renderer tests. /proc gives the
// Linux test an allocator-independent measurement without unsafe allocator hooks.
#[test]
fn repeated_font_loads_release_the_font_bytes() {
    const CHILD: &str = "PIGEON_FONT_MEMORY_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "render::font::tests::repeated_font_loads_release_the_font_bytes",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .expect("isolated font test");
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        eprint!("{}", String::from_utf8_lossy(&result.stdout));
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
        return;
    }
    let (data, index) =
        by_name("DejaVu Sans Mono").expect("install fonts-dejavu-core (CI fixture)");
    let mut cache = super::super::shaped_cell::ShapedCell::default();
    let mut cycle = |size| {
        let primary = Face::load(data.clone(), index).expect("valid reference font");
        let fallback = Face::load(data.clone(), index).expect("valid fallback font");
        let metrics = measure_of(primary.font_ref(), size);
        let font = Font {
            faces: vec![primary, fallback],
            size,
            metrics,
        };
        assert_ne!(font.face(0).expect("primary").charmap().map('M'), 0);
        assert!(!cache.get(&font, "e\u{301}").is_empty());
        drop(font);
    };
    for _ in 0..16 {
        cycle(13.0);
    }
    let before = resident_bytes();
    for index in 0..256 {
        cycle(if index % 2 == 0 { 13.0 } else { 26.0 });
    }
    let growth = resident_bytes().saturating_sub(before);
    eprintln!(
        "256 font load/shape/drop cycles (two faces): {} bytes/face, RSS growth {growth} bytes",
        data.len()
    );
    // Permit allocator retention/noise, but not 256 permanently retained faces.
    assert!(
        growth < data.len() * 32,
        "font bytes retained after drop: {growth}"
    );
}

#[test]
fn borrowed_face_identity_survives_moves_and_changes_after_reload() {
    let (data, index) =
        by_name("DejaVu Sans Mono").expect("install fonts-dejavu-core (CI fixture)");
    let face = Face::load(data.clone(), index).expect("reference font");
    let key = face.font_ref().key;
    let moved = Box::new(face);
    assert_eq!(moved.font_ref().key, key);
    drop(moved);
    let reloaded = Face::load(data, index).expect("reload");
    assert_ne!(reloaded.font_ref().key, key);
}

fn resident_bytes() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux process status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .expect("resident memory")
        .parse::<usize>()
        .expect("RSS in KiB")
        * 1024
}
