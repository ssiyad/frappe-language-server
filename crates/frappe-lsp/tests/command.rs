//! The one-shot commands of spec 26, run as a coding agent runs them: the
//! release binary, from a directory inside the fixture bench.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("tests/fixtures/bench")
}

fn run(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_frappe-lsp"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("the binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn check_reports_a_finding_as_path_line_column_and_exits_1() {
    let output = run(
        &bench(),
        &["check", "apps/circulation-app/circulation/api.py"],
    );

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stdout(&output).lines().any(|line| line.starts_with(
            "apps/circulation-app/circulation/api.py:49:53: warning: `Library Member` has no field `emial_id`"
        )),
        "{}",
        stdout(&output)
    );
}

#[test]
fn check_on_a_clean_file_exits_0_and_says_so() {
    let output = run(&bench(), &["check", "apps/library/library/__init__.py"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert_eq!(stderr(&output).trim(), "0 findings in 1 file checked");
}

#[test]
fn check_walks_a_directory_and_answers_in_json() {
    let app = bench().join("apps/circulation-app");
    let output = run(&app, &["check", "--json"]);

    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert!(
        report["checked"]
            .as_u64()
            .is_some_and(|checked| checked > 1)
    );
    let findings = report["findings"].as_array().expect("findings");
    assert!(findings.iter().any(|finding| {
        finding["path"] == "circulation/api.py"
            && finding["line"] == 49
            && finding["code"] == "frappe-lsp/unknown-field"
    }));
}

#[test]
fn check_outside_any_bench_fails_with_a_reason() {
    let outside = std::env::temp_dir().join(format!("frappe-lsp-outside-{}", std::process::id()));
    std::fs::create_dir_all(&outside).expect("temp dir");

    let output = run(&outside, &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("no Frappe bench or app above"));

    std::fs::remove_dir_all(&outside).expect("cleanup");
}

#[test]
fn describe_prints_one_field_per_line() {
    let output = run(&bench(), &["describe", "Library Member"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("Library Member — "), "{text}");
    assert!(
        text.lines()
            .any(|line| line.trim_start().starts_with("member_name  ")),
        "{text}"
    );
}

#[test]
fn describe_an_unknown_name_offers_its_near_spelling() {
    let output = run(&bench(), &["describe", "library member"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("did you mean Library Member?"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn references_list_sites_of_one_field_with_their_lines() {
    let output = run(&bench(), &["references", "Library Member", "member_name"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output)
            .lines()
            .any(|line| line.starts_with("apps/circulation-app/circulation/api.py:61: ")),
        "{}",
        stdout(&output)
    );
}

#[test]
fn references_to_a_field_that_does_not_exist_say_so() {
    // An empty list would read as "nothing uses this field".
    let output = run(&bench(), &["references", "Library Member", "emial_id"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("`Library Member` has no field `emial_id`"));
}
