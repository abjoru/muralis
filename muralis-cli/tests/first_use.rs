//! The CLI on a machine muralis has never run on: every XDG location absent,
//! no `config.toml`, no daemon having created anything first.
//!
//! Nothing here reaches the network or the developer's real environment — the
//! XDG locations are redirected into a temporary directory, which is the whole
//! point of the fix these cover.

use std::path::Path;
use std::process::Output;

/// `muralis`, run with its XDG locations pointed at `root`. `root` itself is
/// only created by the temporary directory; nothing under it exists.
fn muralis(root: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_muralis"))
        .args(args)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .output()
        .expect("the CLI binary must run")
}

fn succeeded(out: &Output, what: &str) -> String {
    assert!(
        out.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn sources_list_answers_on_a_machine_with_no_muralis_directories() {
    let tmp = tempfile::tempdir().unwrap();

    let out = muralis(tmp.path(), &["sources", "list"]);

    let stdout = succeeded(&out, "sources list");
    serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
        .expect("sources list emits a JSON array, configured or not");
}

/// `favorites stats` opens the database with no daemon in the picture at all,
/// so it is where "the **Library** reads from nothing" is actually decided.
#[test]
fn the_library_opens_on_a_machine_with_no_muralis_directories() {
    let tmp = tempfile::tempdir().unwrap();

    let out = muralis(tmp.path(), &["favorites", "stats"]);

    let stdout = succeeded(&out, "favorites stats");
    assert!(
        stdout.contains("favorites: 0"),
        "an empty Library is the right answer, not a failure to open it: {stdout}"
    );
}

/// `favorites list` prefers the daemon and falls back to the database. Either
/// way it must answer — this asserts the shape, because whether a daemon is
/// running is not something a test gets to decide.
#[test]
fn favorites_list_answers_on_a_machine_with_no_muralis_directories() {
    let tmp = tempfile::tempdir().unwrap();

    let out = muralis(tmp.path(), &["favorites", "list"]);

    let stdout = succeeded(&out, "favorites list");
    serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
        .expect("favorites list emits a JSON array of wallpapers");
}

#[test]
fn cache_stats_answers_on_a_machine_with_no_muralis_directories() {
    let tmp = tempfile::tempdir().unwrap();

    let out = muralis(tmp.path(), &["cache", "stats"]);

    let stdout = succeeded(&out, "cache stats");
    assert!(stdout.contains("total:"), "{stdout}");
}

/// A command that does no IO touches no directories. clap answers both of
/// these before anything resolves a path, and that is the point: asking what
/// muralis is must not make it lay claim to a filesystem.
#[test]
fn asking_for_help_or_version_creates_nothing() {
    for arg in ["--help", "--version"] {
        let tmp = tempfile::tempdir().unwrap();

        let out = muralis(tmp.path(), &[arg]);

        succeeded(&out, arg);
        let created: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(created.is_empty(), "{arg} created {created:?}");
    }
}

/// The fix itself: a CLI-first machine gets its directories from the CLI.
/// Keeping writes a wallpaper file and a **Library** thumbnail, and both
/// directories have to be there before the download the CLI cannot test
/// offline ever returns.
#[test]
fn a_command_that_writes_finds_its_directories_already_made() {
    let tmp = tempfile::tempdir().unwrap();

    succeeded(
        &muralis(tmp.path(), &["favorites", "stats"]),
        "favorites stats",
    );

    for dir in [
        tmp.path().join("config/muralis"),
        tmp.path().join("data/muralis"),
        tmp.path().join("data/muralis/wallpapers"),
        tmp.path().join("cache/muralis"),
        tmp.path().join("cache/muralis/thumbnails"),
    ] {
        assert!(dir.is_dir(), "{} was not created", dir.display());
    }
}
