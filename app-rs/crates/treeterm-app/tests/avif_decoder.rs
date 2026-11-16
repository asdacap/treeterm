//! The app run as Review's AVIF decoder (`--decode-avif`, `src/avif.rs`): it decodes the AVIF
//! on stdin without opening a window, and a corrupt one that makes rav1d abort ends it alone.

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// 16×8: the left half red, the right half blue, 8-bit, opaque.
const OPAQUE_8BIT: &[u8] = include_bytes!("fixtures/opaque-8bit.avif");

/// The app as the decoder, fed `avif`. Its HOME is empty, so were it to start the app anyway,
/// it would find nothing of the user's; it is killed if it has not exited within a minute.
fn decode(avif: &[u8]) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_treeterm"))
        .arg("--decode-avif")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Every input here is far smaller than a pipe's buffer.
    child.stdin.take().unwrap().write_all(avif).unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            panic!("the decoder did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn it_writes_the_size_then_bgra_pixels() {
    let output = decode(OPAQUE_8BIT);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout[..8], [16, 0, 0, 0, 8, 0, 0, 0]);
    assert_eq!(output.stdout.len(), 8 + 16 * 8 * 4);
    // The top-left pixel is red: B, G, R, A.
    let pixel = &output.stdout[8..12];
    assert!(
        pixel[0] < 8 && pixel[1] < 8 && pixel[2] > 247 && pixel[3] == 255,
        "{pixel:?}"
    );
}

#[test]
fn it_fails_on_what_is_not_an_avif() {
    let output = decode(b"not an AVIF");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with("decoding the AVIF: "),
        "{output:?}"
    );
}

/// 128×128, stored as a grid of four 64×64 tiles (made with `heif-enc -T`).
const GRID: &[u8] = include_bytes!("fixtures/grid-2x2.avif");

/// avif-decode's parser (avif-parse 2.1.0) reads still, single-image AVIFs only, so an
/// animated AVIF and a grid of tiles fail, and Review shows such a side as a broken image
/// where Electron draws it (README, Appearance). When this fails, avif-decode has learnt one:
/// update the README.
#[test]
fn it_fails_on_an_animated_or_grid_avif() {
    // The parser turns an animated AVIF away by the `ftyp` box's major brand, `avis`.
    let mut animated = OPAQUE_8BIT.to_vec();
    assert_eq!(&animated[8..12], b"avif");
    animated[8..12].copy_from_slice(b"avis");
    for (avif, error) in [
        (animated.as_slice(), "Animated AVIF is not supported"),
        (GRID, "Grid-based AVIF collage is not supported"),
    ] {
        let output = decode(avif);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(error),
            "{output:?}"
        );
    }
}

/// A bit flipped in the frame's data, where rav1d 1.1.0 panics in its C interface, which
/// would abort the process: it exits with a failure instead (no crash report).
#[test]
fn a_corrupt_frame_ends_the_decoder_only() {
    let mut corrupt = OPAQUE_8BIT.to_vec();
    corrupt[280] ^= 1;
    let output = decode(&corrupt);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        output.status.code().is_some(),
        "killed by a signal: {output:?}"
    );
    assert!(output.stdout.is_empty());
}
