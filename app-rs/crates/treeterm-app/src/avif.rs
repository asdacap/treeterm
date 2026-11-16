//! AVIF images for Review's image diffs, which Electron's `ImageDiffViewer` shows as Chromium
//! decodes them and GPUI cannot decode. rav1d (through avif-decode) decodes them, in a child
//! process: this app run with [`ARG`]. rav1d 1.1.0 aborts the process on some corrupt AV1
//! data (a panic in its C interface cannot unwind; about one in twenty single-bit flips of a
//! frame does it), so a corrupt `.avif` ends the child only, and its side shows the broken
//! image, as Chromium shows an image it cannot decode.

use std::fmt;
use std::io::{self, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;

use gpui_kit::{App, Asset, ImageCacheError, ImageSource, RenderImage, SharedString};

/// The argument that runs the app as the decoder: the AVIF on stdin, its pixels on stdout
/// ([`write_pixels`]). Not for people to type.
pub const ARG: &str = "--decode-avif";

/// The child's exit code when it panics (Rust's own for a panic that ends a process).
const PANICKED: i32 = 101;

/// Decoded pixels, rows top to bottom, in GPUI's BGRA order with straight alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pixels {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

#[derive(Debug)]
enum DecodeError {
    /// The child could not be started, fed or read.
    Io(io::Error),
    /// The child failed or was killed (rav1d aborting): how it ended and what it printed.
    Failed { status: ExitStatus, stderr: String },
    /// The child wrote something other than a size and that many pixels.
    Malformed,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "the AVIF decoder could not be run: {error}"),
            Self::Failed { status, stderr } => {
                write!(f, "the AVIF decoder failed ({status}): {stderr}")
            }
            Self::Malformed => {
                f.write_str("the AVIF decoder's output is not a size and its pixels")
            }
        }
    }
}

/// What a Review image diff draws for an AVIF side: its pixels once a child has decoded them
/// (nothing until then), or the image's fallback if it could not.
pub fn image_source(avif: Arc<Vec<u8>>) -> ImageSource {
    ImageSource::Custom(Arc::new(move |window, cx| {
        window.use_asset::<Decoded>(&avif, cx)
    }))
}

/// A decoded AVIF in GPUI's asset cache, keyed by its bytes, decoded on the background
/// executor (where GPUI runs asset loads).
enum Decoded {}

impl Asset for Decoded {
    type Source = Arc<Vec<u8>>;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        avif: Self::Source,
        _: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let app = std::env::current_exe();
        async move {
            let decoded = app.map_err(DecodeError::Io).and_then(|app| {
                let mut command = Command::new(app);
                command.arg(ARG);
                decode_with(command, &avif)
            });
            match decoded {
                Ok(pixels) => Ok(Arc::new(render_image(pixels))),
                Err(error) => {
                    tracing::warn!(%error, "AVIF image not decoded");
                    Err(ImageCacheError::Asset(SharedString::from(
                        error.to_string(),
                    )))
                }
            }
        }
    }
}

fn render_image(pixels: Pixels) -> RenderImage {
    let buffer = image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.bgra)
        .expect("read_pixels checked there are width × height pixels");
    RenderImage::new(vec![image::Frame::new(buffer)])
}

/// Runs `command` (the app with [`ARG`], or a stand-in in tests) on `avif` and reads the
/// pixels it writes.
fn decode_with(mut command: Command, avif: &[u8]) -> Result<Pixels, DecodeError> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(DecodeError::Io)?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // Fed from its own thread while the child's output is read, so neither waits on the other.
    let (written, output) = std::thread::scope(|scope| {
        let feed = scope.spawn(move || stdin.write_all(avif));
        let output = child.wait_with_output();
        (
            feed.join().expect("writing to a pipe does not panic"),
            output,
        )
    });
    let output = output.map_err(DecodeError::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(DecodeError::Failed {
            status: output.status,
            stderr,
        });
    }
    // A child that succeeded read all of it.
    written.map_err(DecodeError::Io)?;
    read_pixels(&output.stdout)
}

/// The child ([`ARG`]): decodes the AVIF on stdin and writes its pixels to stdout. Returns
/// the process's exit code.
pub fn serve() -> i32 {
    // A panic in rav1d cannot unwind out of its C interface, so the process would abort (and
    // leave a crash report): it exits at the panic instead, as a failure.
    std::panic::set_hook(Box::new(|panic| {
        eprintln!("{panic}");
        std::process::exit(PANICKED);
    }));
    match serve_streams(&mut io::stdin().lock(), &mut io::stdout().lock()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn serve_streams(input: &mut impl Read, output: &mut impl Write) -> Result<(), String> {
    let mut avif = Vec::new();
    input
        .read_to_end(&mut avif)
        .map_err(|error| format!("reading the AVIF: {error}"))?;
    let pixels = decode(&avif).map_err(|error| format!("decoding the AVIF: {error}"))?;
    write_pixels(&pixels, output).map_err(|error| format!("writing its pixels: {error}"))
}

/// The size (width then height, little-endian `u32`s), then the pixels.
fn write_pixels(pixels: &Pixels, output: &mut impl Write) -> io::Result<()> {
    output.write_all(&pixels.width.to_le_bytes())?;
    output.write_all(&pixels.height.to_le_bytes())?;
    output.write_all(&pixels.bgra)?;
    output.flush()
}

fn read_pixels(bytes: &[u8]) -> Result<Pixels, DecodeError> {
    let (size, bgra) = bytes.split_at_checked(8).ok_or(DecodeError::Malformed)?;
    let (width, height) = size.split_at(4);
    let width = u32::from_le_bytes(width.try_into().expect("four bytes"));
    let height = u32::from_le_bytes(height.try_into().expect("four bytes"));
    let length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if length != Some(bgra.len()) {
        return Err(DecodeError::Malformed);
    }
    Ok(Pixels {
        width,
        height,
        bgra: bgra.to_vec(),
    })
}

/// Decodes `avif` in this process: the child's work.
fn decode(avif: &[u8]) -> Result<Pixels, avif_decode::Error> {
    Ok(bgra(avif_decode::Decoder::from_avif(avif)?.to_image()?))
}

/// `image` as 8-bit BGRA, deeper samples rounded to 8 bits (as Chromium draws them).
fn bgra(image: avif_decode::Image) -> Pixels {
    use avif_decode::Image;
    /// A 16-bit sample (avif-decode widens 10- and 12-bit ones) rounded to 8 bits.
    fn eight(sample: u16) -> u8 {
        u8::try_from((u32::from(sample) * 255 + 32_767) / 65_535).expect("at most 255")
    }
    fn pixels(width: usize, height: usize, bgra: impl Iterator<Item = [u8; 4]>) -> Pixels {
        Pixels {
            width: u32::try_from(width).expect("an AVIF's width is a u32"),
            height: u32::try_from(height).expect("an AVIF's height is a u32"),
            bgra: bgra.flatten().collect(),
        }
    }
    match image {
        Image::Rgb8(image) => pixels(
            image.width(),
            image.height(),
            image.pixels().map(|p| [p.b, p.g, p.r, u8::MAX]),
        ),
        Image::Rgba8(image) => pixels(
            image.width(),
            image.height(),
            image.pixels().map(|p| [p.b, p.g, p.r, p.a]),
        ),
        Image::Rgb16(image) => pixels(
            image.width(),
            image.height(),
            image
                .pixels()
                .map(|p| [eight(p.b), eight(p.g), eight(p.r), u8::MAX]),
        ),
        Image::Rgba16(image) => pixels(
            image.width(),
            image.height(),
            image
                .pixels()
                .map(|p| [eight(p.b), eight(p.g), eight(p.r), eight(p.a)]),
        ),
        Image::Gray8(image) => pixels(
            image.width(),
            image.height(),
            image.pixels().map(|p| {
                let value = p.value();
                [value, value, value, u8::MAX]
            }),
        ),
        Image::Gray16(image) => pixels(
            image.width(),
            image.height(),
            image.pixels().map(|p| {
                let value = eight(p.value());
                [value, value, value, u8::MAX]
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16×8: the left half red, the right half blue, 8-bit, opaque (decodes as `Rgb8`).
    const OPAQUE_8BIT: &[u8] = include_bytes!("../tests/fixtures/opaque-8bit.avif");
    /// The same, 10-bit, the blue half at alpha 128 (decodes as `Rgba16`).
    const ALPHA_10BIT: &[u8] = include_bytes!("../tests/fixtures/alpha-10bit.avif");

    /// The BGRA pixel at `x`, `y`.
    fn at(pixels: &Pixels, x: usize, y: usize) -> [u8; 4] {
        let start = (y * pixels.width as usize + x) * 4;
        pixels.bgra[start..start + 4].try_into().unwrap()
    }

    /// Whether `pixel` is `expected` give or take the encoder's loss.
    fn near(pixel: [u8; 4], expected: [u8; 4]) -> bool {
        pixel
            .iter()
            .zip(expected)
            .all(|(got, want)| got.abs_diff(want) <= 4)
    }

    #[test]
    fn avifs_decode_to_bgra_pixels() {
        let opaque = decode(OPAQUE_8BIT).unwrap();
        assert_eq!(
            (opaque.width, opaque.height, opaque.bgra.len()),
            (16, 8, 16 * 8 * 4)
        );
        assert!(
            near(at(&opaque, 1, 4), [0, 0, 255, 255]),
            "red: {:?}",
            at(&opaque, 1, 4)
        );
        assert!(
            near(at(&opaque, 14, 4), [255, 0, 0, 255]),
            "blue: {:?}",
            at(&opaque, 14, 4)
        );
        let alpha = decode(ALPHA_10BIT).unwrap();
        assert_eq!((alpha.width, alpha.height), (16, 8));
        assert!(
            near(at(&alpha, 1, 4), [0, 0, 255, 255]),
            "red: {:?}",
            at(&alpha, 1, 4)
        );
        assert!(
            near(at(&alpha, 14, 4), [255, 0, 0, 128]),
            "blue: {:?}",
            at(&alpha, 14, 4)
        );
        assert!(decode(b"not an AVIF").is_err());
    }

    #[test]
    fn every_kind_of_decoded_image_becomes_bgra() {
        use avif_decode::Image;
        use imgref::ImgVec;
        use rgb::{Gray, Rgb, Rgba};
        let cases = [
            (
                Image::Rgb8(ImgVec::new(vec![Rgb::new(10, 20, 30)], 1, 1)),
                [30, 20, 10, 255],
            ),
            (
                Image::Rgba8(ImgVec::new(vec![Rgba::new(10, 20, 30, 40)], 1, 1)),
                [30, 20, 10, 40],
            ),
            // 16-bit samples round to the nearest 8-bit one: 0x8080 is 128, 0x807f 128 too.
            (
                Image::Rgb16(ImgVec::new(vec![Rgb::new(0xffff, 0x8080, 0)], 1, 1)),
                [0, 128, 255, 255],
            ),
            (
                Image::Rgba16(ImgVec::new(
                    vec![Rgba::new(0, 0x807f, 0xffff, 0x0101)],
                    1,
                    1,
                )),
                [255, 128, 0, 1],
            ),
            (
                Image::Gray8(ImgVec::new(vec![Gray::new(7)], 1, 1)),
                [7, 7, 7, 255],
            ),
            (
                Image::Gray16(ImgVec::new(vec![Gray::new(0x0707)], 1, 1)),
                [7, 7, 7, 255],
            ),
        ];
        for (image, pixel) in cases {
            assert_eq!(
                bgra(image),
                Pixels {
                    width: 1,
                    height: 1,
                    bgra: pixel.to_vec()
                }
            );
        }
        // Rows in order.
        let wide = Image::Gray8(ImgVec::new(
            vec![Gray::new(1), Gray::new(2), Gray::new(3), Gray::new(4)],
            2,
            2,
        ));
        assert_eq!(
            bgra(wide).bgra,
            [1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255, 4, 4, 4, 255]
        );
    }

    #[test]
    fn the_child_writes_the_size_then_the_pixels() {
        let mut output = Vec::new();
        serve_streams(&mut &OPAQUE_8BIT[..], &mut output).unwrap();
        assert_eq!(output[..8], [16, 0, 0, 0, 8, 0, 0, 0]);
        assert_eq!(read_pixels(&output).unwrap(), decode(OPAQUE_8BIT).unwrap());
        let error = serve_streams(&mut &b"not an AVIF"[..], &mut Vec::new()).unwrap_err();
        assert!(error.starts_with("decoding the AVIF: "), "{error}");
    }

    #[test]
    fn output_that_is_not_a_size_and_its_pixels_is_malformed() {
        let pixels = Pixels {
            width: 2,
            height: 1,
            bgra: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let mut output = Vec::new();
        write_pixels(&pixels, &mut output).unwrap();
        assert_eq!(read_pixels(&output).unwrap(), pixels);
        let longer = [output.as_slice(), &[0]].concat();
        let bad: [&[u8]; 4] = [&output[..output.len() - 1], &longer, &output[..7], &[]];
        for bad in bad {
            assert!(
                matches!(read_pixels(bad), Err(DecodeError::Malformed)),
                "{bad:?}"
            );
        }
        // A size whose pixels would not fit in memory.
        assert!(matches!(
            read_pixels(&[0xff; 8]),
            Err(DecodeError::Malformed)
        ));
    }

    /// `sh -c <script>` as the decoder, with `args` as `$1`, ….
    fn shell(script: &str, args: &[&str]) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script).arg("sh").args(args);
        command
    }

    #[test]
    fn the_parent_reads_what_the_child_writes_and_survives_it_dying() {
        let dir = tempfile::tempdir().unwrap();
        let pixels = Pixels {
            width: 1,
            height: 2,
            bgra: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let reply = dir.path().join("reply");
        let mut bytes = Vec::new();
        write_pixels(&pixels, &mut bytes).unwrap();
        std::fs::write(&reply, bytes).unwrap();
        // The child reads all of its input before it answers; a large one is fed while it does.
        let input = vec![7; 1 << 20];
        let echoed = dir.path().join("input");
        let echo = shell(
            "cat > \"$2\"; cat \"$1\"",
            &[reply.to_str().unwrap(), echoed.to_str().unwrap()],
        );
        assert_eq!(decode_with(echo, &input).unwrap(), pixels);
        assert_eq!(std::fs::read(&echoed).unwrap(), input);
        // rav1d aborting.
        match decode_with(
            shell(
                "cat > /dev/null; echo 'rav1d gave up' >&2; kill -ABRT $$",
                &[],
            ),
            &input,
        ) {
            Err(DecodeError::Failed { status, stderr }) => {
                assert!(!status.success());
                assert_eq!(stderr, "rav1d gave up");
            }
            other => panic!("{other:?}"),
        }
        // Failing, and answering nonsense.
        assert!(matches!(
            decode_with(shell("cat > /dev/null; exit 1", &[]), &input),
            Err(DecodeError::Failed { .. })
        ));
        assert!(matches!(
            decode_with(shell("cat > /dev/null; echo nonsense", &[]), &input),
            Err(DecodeError::Malformed)
        ));
        assert!(matches!(
            decode_with(Command::new(dir.path().join("missing")), &input),
            Err(DecodeError::Io(_))
        ));
    }
}
