//! Items of Electron's `localStorage`: Chromium keeps it in a LevelDB under
//! `<userData>/Local Storage/leveldb`, read here to carry over what the Electron app kept
//! there.
//!
//! The files are read into memory and the database opened there: Electron may have it open,
//! and opening it in place would take its lock and could compact (rewrite) its files.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use rusty_leveldb::{DB, Options, Status};

/// The origin of the packaged Electron app's pages (`file://` URLs). The dev server's
/// (`http://localhost:5173`) is a different one.
pub const ELECTRON_ORIGIN: &str = "file://";

/// Chromium's localStorage directory in Electron's userData directory.
pub fn leveldb_dir(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("Local Storage").join("leveldb")
}

#[derive(Debug, thiserror::Error)]
pub enum LocalStorageError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot open the localStorage database in {path}: {source}")]
    Open { path: PathBuf, source: Status },
    #[error("cannot read {key} from the localStorage database in {path}: {source}")]
    Get { path: PathBuf, key: String, source: Status },
    #[error("{key} in the localStorage database in {path} is not a stored string: {reason}")]
    Decode { path: PathBuf, key: String, reason: &'static str },
}

/// The item `key` of `origin`'s localStorage in the LevelDB `dir`; `None` when it has none.
pub fn read_item(dir: &Path, origin: &str, key: &str) -> Result<Option<String>, LocalStorageError> {
    let options = Options { create_if_missing: false, ..rusty_leveldb::in_memory() };
    let name = Path::new("leveldb");
    copy_into(dir, &options, name)?;
    let open = |source| LocalStorageError::Open { path: dir.to_owned(), source };
    let mut db = DB::open(name, options).map_err(open)?;
    let snapshot = db.get_snapshot();
    let item = [b"_", origin.as_bytes(), b"\0", &encode(key)].concat();
    let get = |source| LocalStorageError::Get { path: dir.to_owned(), key: key.to_owned(), source };
    let Some(value) = db.get_at(&snapshot, &item).map_err(get)? else {
        return Ok(None);
    };
    decode(&value)
        .map(Some)
        .map_err(|reason| LocalStorageError::Decode { path: dir.to_owned(), key: key.to_owned(), reason })
}

/// Every file of `dir` but Chromium's `LOCK`, into `options`' (in-memory) env as `name`.
fn copy_into(dir: &Path, options: &Options, name: &Path) -> Result<(), LocalStorageError> {
    let read = |path: &Path| {
        let path = path.to_owned();
        move |source| LocalStorageError::Read { path, source }
    };
    let open = |source| LocalStorageError::Open { path: dir.to_owned(), source };
    for entry in std::fs::read_dir(dir).map_err(read(dir))? {
        let entry = entry.map_err(read(dir))?;
        let path = entry.path();
        if entry.file_name() == "LOCK" || !entry.file_type().map_err(read(&path))?.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(read(&path))?;
        let mut copy = options.env.open_writable_file(&name.join(entry.file_name())).map_err(open)?;
        copy.write_all(&bytes).map_err(|error| open(Status::from(error)))?;
    }
    Ok(())
}

/// Chromium's localStorage string format (`StorageFormat` in its `LocalStorageDetectFormat`):
/// a string whose characters all fit in a byte is `1` and its Latin-1 bytes; any other is `0`
/// and UTF-16LE.
fn encode(text: &str) -> Vec<u8> {
    match text.chars().map(|c| u8::try_from(u32::from(c))).collect::<Result<Vec<u8>, _>>() {
        Ok(latin1) => [&[1], latin1.as_slice()].concat(),
        Err(_) => std::iter::once(0).chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect(),
    }
}

fn decode(bytes: &[u8]) -> Result<String, &'static str> {
    match bytes.split_first() {
        Some((1, latin1)) => Ok(latin1.iter().map(|&byte| char::from(byte)).collect()),
        Some((0, utf16)) => {
            let (units, []) = utf16.as_chunks::<2>() else {
                return Err("odd UTF-16 length");
            };
            String::from_utf16(&units.iter().map(|unit| u16::from_le_bytes(*unit)).collect::<Vec<_>>())
                .map_err(|_| "invalid UTF-16")
        }
        Some(_) => Err("unknown format prefix"),
        None => Err("empty"),
    }
}

/// Chromium's localStorage written for tests. Panics on any error.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use super::*;
    use rusty_leveldb::compressor::{CompressorId as _, SnappyCompressor};

    /// A LevelDB written to `dir` as Chromium writes one (snappy tables, a write-ahead log).
    /// `compacted` entries go into a table file; `logged` ones stay in the log.
    pub fn write_db(dir: &Path, compacted: &[(&[u8], &[u8])], logged: &[(&[u8], &[u8])]) {
        let options = Options { compressor: SnappyCompressor::ID, ..rusty_leveldb::in_memory() };
        let env = options.env.clone();
        let mut db = DB::open("db", options).unwrap();
        for (key, value) in compacted {
            db.put(key, value).unwrap();
        }
        db.compact_range(b"\0", b"\xff").unwrap();
        for (key, value) in logged {
            db.put(key, value).unwrap();
        }
        db.close().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        for file in env.children(Path::new("db")).unwrap() {
            let mut bytes = Vec::new();
            let mut reader = env.open_sequential_file(&Path::new("db").join(&file)).unwrap();
            io::Read::read_to_end(&mut reader, &mut bytes).unwrap();
            std::fs::write(dir.join(file), bytes).unwrap();
        }
    }

    /// The LevelDB key of `origin`'s localStorage item `key`.
    pub fn item_key(origin: &str, key: &str) -> Vec<u8> {
        [b"_", origin.as_bytes(), b"\0", &encode(key)].concat()
    }

    /// A stored string's LevelDB value.
    pub fn item_value(text: &str) -> Vec<u8> {
        encode(text)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{item_key, write_db};
    use super::*;

    const KEY: &str = "treeterm-recent-directories";

    #[test]
    fn reads_a_latin1_item_of_the_origin_from_tables_and_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = leveldb_dir(dir.path());
        let compacted_key = item_key(ELECTRON_ORIGIN, KEY);
        let other_origin = item_key("http://localhost:5173", "logged");
        let logged_key = item_key(ELECTRON_ORIGIN, "logged");
        write_db(
            &path,
            &[(b"VERSION", b"1"), (&compacted_key, b"\x01{\"caf\xe9\":1}"), (&other_origin, b"\x01dev")],
            &[(&logged_key, b"\x01from the log")],
        );
        let has = |extension: &str| {
            path.read_dir().unwrap().any(|file| file.unwrap().path().extension().is_some_and(|e| e == extension))
        };
        assert!(has("ldb") && has("log"));
        // Electron holds LOCK while it runs: it is not copied, so never taken.
        std::fs::write(path.join("LOCK"), b"").unwrap();

        assert_eq!(read_item(&path, ELECTRON_ORIGIN, KEY).unwrap().as_deref(), Some("{\"café\":1}"));
        assert_eq!(read_item(&path, ELECTRON_ORIGIN, "logged").unwrap().as_deref(), Some("from the log"));
        assert_eq!(read_item(&path, "http://localhost:5173", "logged").unwrap().as_deref(), Some("dev"));
        assert_eq!(read_item(&path, ELECTRON_ORIGIN, "absent").unwrap(), None);
    }

    #[test]
    fn reads_a_utf16_item_and_the_files_are_left_as_they_were() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leveldb");
        let value: Vec<u8> = std::iter::once(0).chain("/work/日本".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let utf16_key = item_key(ELECTRON_ORIGIN, "ключ");
        write_db(&path, &[(&item_key(ELECTRON_ORIGIN, KEY), &value)], &[(&utf16_key, b"\x01x")]);
        let listing = || {
            let mut files: Vec<_> = path
                .read_dir()
                .unwrap()
                .map(|f| {
                    let f = f.unwrap();
                    (f.file_name(), std::fs::read(f.path()).unwrap())
                })
                .collect();
            files.sort();
            files
        };
        let before = listing();

        assert_eq!(read_item(&path, ELECTRON_ORIGIN, KEY).unwrap().as_deref(), Some("/work/日本"));
        // A key with characters beyond Latin-1 is stored as UTF-16.
        assert_eq!(read_item(&path, ELECTRON_ORIGIN, "ключ").unwrap().as_deref(), Some("x"));
        assert_eq!(listing(), before);
    }

    #[test]
    fn a_value_in_no_known_format_or_a_missing_database_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leveldb");
        write_db(
            &path,
            &[(&item_key(ELECTRON_ORIGIN, "bad"), b"\x02x"), (&item_key(ELECTRON_ORIGIN, "odd"), b"\x00x")],
            &[],
        );
        assert!(matches!(
            read_item(&path, ELECTRON_ORIGIN, "bad"),
            Err(LocalStorageError::Decode { reason: "unknown format prefix", .. })
        ));
        assert!(matches!(
            read_item(&path, ELECTRON_ORIGIN, "odd"),
            Err(LocalStorageError::Decode { reason: "odd UTF-16 length", .. })
        ));

        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(matches!(read_item(&empty, ELECTRON_ORIGIN, KEY), Err(LocalStorageError::Open { .. })));
        let missing = dir.path().join("missing");
        assert!(matches!(read_item(&missing, ELECTRON_ORIGIN, KEY), Err(LocalStorageError::Read { .. })));
    }

    #[test]
    fn strings_decode_as_chromium_encodes_them() {
        assert_eq!(encode("é"), b"\x01\xe9");
        assert_eq!(encode("€"), b"\x00\xac\x20");
        assert_eq!(decode(b"\x01\xe9"), Ok("é".to_owned()));
        assert_eq!(decode(b"\x00\xac\x20"), Ok("€".to_owned()));
        assert_eq!(decode(b"\x00\x00\xd8"), Err("invalid UTF-16"));
        assert_eq!(decode(b""), Err("empty"));
    }
}
