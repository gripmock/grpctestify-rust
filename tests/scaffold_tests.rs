#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! `scaffold` shipped without any test coverage. It is the command that writes
//! a user's first `.gctf`, so the property that matters most is that its output
//! is something the rest of the tool accepts — a generator that emits files its
//! own validator rejects is worse than no generator.
//!
//! The generated REQUEST body is populated with randomised sample values, so
//! these assert on structure, never on an exact rendering.

#[path = "support/mod.rs"]
mod support;

fn helloworld_proto() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/server/helloworld.proto")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn scaffold_from_proto_emits_the_expected_sections() {
    let output = support::cli_command()
        .args([
            "scaffold",
            "--proto",
            &helloworld_proto(),
            "--endpoint",
            "helloworld.Greeter/SayHello",
        ])
        .output()
        .expect("failed to run scaffold");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for section in [
        "--- ADDRESS ---",
        "--- ENDPOINT ---",
        "--- PROTO ---",
        "--- REQUEST ---",
    ] {
        assert!(stdout.contains(section), "missing {section} in:\n{stdout}");
    }
    assert!(
        stdout.contains("helloworld.Greeter/SayHello"),
        "the endpoint must be carried through:\n{stdout}"
    );
}

/// The round trip that matters: whatever `scaffold` writes, `check` accepts.
#[test]
fn scaffolded_file_passes_check() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scaffolded.gctf");

    let generated = support::cli_command()
        .args([
            "scaffold",
            "--proto",
            &helloworld_proto(),
            "--endpoint",
            "helloworld.Greeter/SayHello",
            "-o",
            &file.to_string_lossy(),
        ])
        .output()
        .expect("failed to run scaffold");
    assert!(
        generated.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(file.exists(), "-o must write the file");

    let checked = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    assert!(
        checked.status.success(),
        "scaffold must emit a file its own validator accepts\nscaffolded:\n{}\ncheck said:\n{}",
        std::fs::read_to_string(&file).unwrap_or_default(),
        String::from_utf8_lossy(&checked.stdout)
    );
}

/// Overwriting is opt-in, so an accidental second run can't destroy edits.
#[test]
fn scaffold_refuses_to_overwrite_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("existing.gctf");
    std::fs::write(&file, "--- ENDPOINT ---\nkeep.Me/Intact\n").unwrap();

    let output = support::cli_command()
        .args([
            "scaffold",
            "--proto",
            &helloworld_proto(),
            "--endpoint",
            "helloworld.Greeter/SayHello",
            "-o",
            &file.to_string_lossy(),
        ])
        .output()
        .expect("failed to run scaffold");

    assert!(
        !output.status.success(),
        "writing over an existing file must need --force"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "--- ENDPOINT ---\nkeep.Me/Intact\n",
        "the existing file must be left untouched"
    );

    let forced = support::cli_command()
        .args([
            "scaffold",
            "--proto",
            &helloworld_proto(),
            "--endpoint",
            "helloworld.Greeter/SayHello",
            "-o",
            &file.to_string_lossy(),
            "--force",
        ])
        .output()
        .expect("failed to run scaffold --force");
    assert!(
        forced.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("helloworld.Greeter/SayHello"),
        "--force must replace the contents"
    );
}

#[test]
fn scaffold_reports_an_unknown_method() {
    let output = support::cli_command()
        .args([
            "scaffold",
            "--proto",
            &helloworld_proto(),
            "--endpoint",
            "helloworld.Greeter/NoSuchMethod",
        ])
        .output()
        .expect("failed to run scaffold");

    assert!(
        !output.status.success(),
        "a method that isn't in the proto must fail, not scaffold an empty test"
    );
}
