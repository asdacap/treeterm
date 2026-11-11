//! `cargo build` that reports where the built executable is (honouring `CARGO_TARGET_DIR`,
//! `--target` and profiles) instead of guessing `target/<profile>/<bin>`.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::{Context, Result};

/// Run `command` (a `cargo build …` for `bin`) and return the path of the `bin` executable.
/// Diagnostics and progress go to the terminal as usual.
pub fn build_executable(mut command: Command, bin: &str) -> Result<PathBuf> {
    command
        .arg("--message-format=json-render-diagnostics")
        .stdout(Stdio::piped());
    let shown = format!("{command:?}");
    let mut child = command.spawn().context(|| format!("start {shown}"))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut executable = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.context(|| format!("read the output of {shown}"))?;
        match serde_json::from_str::<Value>(&line) {
            Ok(message) => {
                if let Some(path) = executable_in(&message, bin) {
                    executable = Some(path);
                }
            }
            // Not one of cargo's messages: pass it through.
            Err(_) => println!("{line}"),
        }
    }
    let status = child.wait().context(|| format!("wait for {shown}"))?;
    if !status.success() {
        return Err(format!("{shown} failed ({status})"));
    }
    executable.ok_or_else(|| format!("{shown} did not report a `{bin}` executable"))
}

/// The executable of a `compiler-artifact` message for the binary target `bin`.
fn executable_in(message: &Value, bin: &str) -> Option<PathBuf> {
    let is_bin = message["reason"] == "compiler-artifact"
        && message["target"]["name"] == bin
        && message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"));
    if !is_bin {
        return None;
    }
    message["executable"].as_str().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn picks_the_executable_of_the_named_binary() {
        let artifact = message(
            r#"{"reason":"compiler-artifact","target":{"name":"treeterm","kind":["bin"]},"executable":"/t/release/treeterm","fresh":true}"#,
        );
        assert_eq!(
            executable_in(&artifact, "treeterm"),
            Some(PathBuf::from("/t/release/treeterm"))
        );
    }

    #[test]
    fn ignores_other_targets_libraries_and_other_messages() {
        let other_bin = r#"{"reason":"compiler-artifact","target":{"name":"xtask","kind":["bin"]},"executable":"/t/xtask"}"#;
        let library = r#"{"reason":"compiler-artifact","target":{"name":"treeterm","kind":["lib"]},"executable":null}"#;
        let finished = r#"{"reason":"build-finished","success":true}"#;
        for json in [other_bin, library, finished] {
            assert_eq!(executable_in(&message(json), "treeterm"), None, "{json}");
        }
    }
}
