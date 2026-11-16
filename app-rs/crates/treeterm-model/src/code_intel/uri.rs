//! vscode-uri's `URI` (Monaco's `vs/base/common/uri`), as Monaco's JSON worker reads schema
//! ids and its opener reads the links it opens: parsed into decoded components, written
//! back, and read as a file path (not on Windows).

/// A run of `%XX` escapes (`%` and two letters or digits) at the start of `text`, in bytes.
fn escapes_len(text: &str) -> usize {
    text.as_bytes()
        .chunks(3)
        .take_while(|escape| matches!(escape, [b'%', first, second] if first.is_ascii_alphanumeric() && second.is_ascii_alphanumeric()))
        .count()
        * 3
}

/// A run of escapes `decodeURIComponent` throws on: one is not hexadecimal, or their bytes
/// are not UTF-8.
struct Undecodable;

/// `decodeURIComponent` of a run of `%XX` escapes.
fn decode_escapes(run: &str) -> Result<String, Undecodable> {
    let hex = |digit: u8| (digit as char).to_digit(16).ok_or(Undecodable);
    let bytes = run
        .as_bytes()
        .chunks(3)
        .map(|escape| Ok((hex(escape[1])? * 16 + hex(escape[2])?) as u8))
        .collect::<Result<Vec<u8>, Undecodable>>()?;
    String::from_utf8(bytes).map_err(|_| Undecodable)
}

/// `%XX` escapes read as vscode-uri decodes a component (`percentDecode`): each run of them
/// as UTF-8, and a run that is not from its next escape on, the escapes before left as they
/// are (`decodeURIComponentGraceful`).
pub(super) fn percent_decode(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(percent) = rest.find('%') {
        decoded.push_str(&rest[..percent]);
        rest = &rest[percent..];
        let mut run = &rest[..escapes_len(rest)];
        rest = &rest[run.len()..];
        if run.is_empty() {
            // Not an escape.
            decoded.push('%');
            rest = &rest[1..];
            continue;
        }
        while !run.is_empty() {
            match decode_escapes(run) {
                Ok(text) => {
                    decoded.push_str(&text);
                    break;
                }
                // Left as it is.
                Err(Undecodable) => {
                    decoded.push_str(&run[..3]);
                    run = &run[3..];
                }
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Whether `scheme` is one (`/^\w[\w\d+.-]*$/`).
pub(super) fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphanumeric() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '.' | '-'))
}

/// `#` and `?` escaped (`encodeURIComponentMinimal`), the rest written as it is.
fn encode_minimal(component: &str) -> String {
    component.replace('#', "%23").replace('?', "%3F")
}

/// How [`Uri`]'s file path writes a Windows drive letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriveLetter {
    /// In lower case (`fsPath`).
    Lowered,
    /// As written (`originalFSPath`).
    Kept,
}

/// vscode-uri's `URI`, its components decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Uri {
    pub(super) scheme: String,
    pub(super) authority: String,
    pub(super) path: String,
    pub(super) query: String,
    pub(super) fragment: String,
}

impl Uri {
    /// `URI.parse(value)`; `None` where it throws.
    pub(super) fn parse(value: &str) -> Option<Self> {
        // `^(([^:/?#]+?):)?(\/\/([^/?#]*))?([^?#]*)(\?([^#]*))?(#(.*))?`
        let (scheme, rest) = match value.find([':', '/', '?', '#']) {
            Some(colon) if colon > 0 && value[colon..].starts_with(':') => (&value[..colon], &value[colon + 1..]),
            Some(_) | None => ("", value),
        };
        let (authority, rest) = match rest.strip_prefix("//") {
            Some(after) => after.split_at(after.find(['/', '?', '#']).unwrap_or(after.len())),
            None => ("", rest),
        };
        let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        // No scheme reads as `file`.
        let scheme = if scheme.is_empty() { "file" } else { scheme };
        let mut path = percent_decode(path);
        // `_referenceResolution`
        if matches!(scheme, "https" | "http" | "file") {
            if path.is_empty() {
                path = "/".to_owned();
            } else if !path.starts_with('/') {
                path.insert(0, '/');
            }
        }
        let uri = Self {
            scheme: scheme.to_owned(),
            authority: percent_decode(authority),
            path,
            query: percent_decode(query),
            fragment: percent_decode(fragment),
        };
        // `_validateUri`
        let valid = is_scheme(&uri.scheme)
            && (uri.path.is_empty()
                || if uri.authority.is_empty() { !uri.path.starts_with("//") } else { uri.path.starts_with('/') });
        valid.then_some(uri)
    }

    /// `URI.file(path)`: a path as a `file` URI, `//host/…` as its authority.
    fn file(path: &str) -> Self {
        let (authority, path) = match path.strip_prefix("//") {
            Some(unc) => match unc.find('/') {
                Some(slash) => (&unc[..slash], &unc[slash..]),
                None => (unc, "/"),
            },
            None => ("", path),
        };
        // `_referenceResolution`
        let path = if path.starts_with('/') { path.to_owned() } else { format!("/{path}") };
        Self { scheme: "file".to_owned(), authority: authority.to_owned(), path, query: String::new(), fragment: String::new() }
    }

    /// `toString(true)` (`_asFormatted` without encoding): as read, but `#` and `?` escaped in
    /// the path and query, the host and a drive letter in lower case.
    pub(super) fn to_display_string(&self) -> String {
        let mut formatted = format!("{}:", self.scheme);
        if !self.authority.is_empty() || self.scheme == "file" {
            formatted.push_str("//");
        }
        let host = match self.authority.split_once('@') {
            Some((user_info, host)) => {
                formatted.push_str(&encode_minimal(user_info));
                formatted.push('@');
                host
            }
            None => &self.authority,
        };
        formatted.push_str(&encode_minimal(&host.to_lowercase()));
        let path = self.path.as_bytes();
        let path = match path {
            [b'/', letter @ b'A'..=b'Z', b':', ..] => format!("/{}:{}", letter.to_ascii_lowercase() as char, &self.path[3..]),
            [letter @ b'A'..=b'Z', b':', ..] => format!("{}:{}", letter.to_ascii_lowercase() as char, &self.path[2..]),
            _ => self.path.clone(),
        };
        formatted.push_str(&encode_minimal(&path));
        if !self.query.is_empty() {
            formatted.push('?');
            formatted.push_str(&encode_minimal(&self.query));
        }
        if !self.fragment.is_empty() {
            formatted.push('#');
            formatted.push_str(&self.fragment);
        }
        formatted
    }

    /// `uriToFsPath` (not on Windows): `//host/path` for a `file` URI with a host; a drive
    /// letter's path without the `/` before it.
    fn fs_path_with(&self, letter: DriveLetter) -> String {
        if !self.authority.is_empty() && self.path.len() > 1 && self.scheme == "file" {
            return format!("//{}{}", self.authority, self.path);
        }
        match (self.path.as_bytes(), letter) {
            ([b'/', drive, b':', ..], DriveLetter::Lowered) if drive.is_ascii_alphabetic() => {
                format!("{}{}", drive.to_ascii_lowercase() as char, &self.path[2..])
            }
            ([b'/', drive, b':', ..], DriveLetter::Kept) if drive.is_ascii_alphabetic() => self.path[1..].to_owned(),
            _ => self.path.clone(),
        }
    }

    /// `fsPath`.
    pub(super) fn fs_path(&self) -> String {
        self.fs_path_with(DriveLetter::Lowered)
    }

    /// `resources.normalizePath`: a `file` URI's path normalized as a file path (through
    /// `originalFSPath`, so `file://host/x` becomes `/host/x`), another's as a POSIX path.
    pub(super) fn normalize_path(&self) -> Self {
        if self.path.is_empty() {
            return self.clone();
        }
        let path = match self.scheme.as_str() {
            "file" => Self::file(&posix_normalize(&self.fs_path_with(DriveLetter::Kept))).path,
            _ => posix_normalize(&self.path),
        };
        Self { path, ..self.clone() }
    }
}

/// Node's `path.posix.normalize`: `.` and `..` segments resolved and repeated slashes
/// collapsed, a trailing slash kept.
fn posix_normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&last) if last != ".." => {
                    segments.pop();
                }
                // Above the root of an absolute path: dropped.
                Some(_) | None if absolute => {}
                Some(_) | None => segments.push(".."),
            },
            segment => segments.push(segment),
        }
    }
    let joined = segments.join("/");
    match (joined.is_empty(), absolute, trailing) {
        (true, true, _) => "/".to_owned(),
        (true, false, true) => "./".to_owned(),
        (true, false, false) => ".".to_owned(),
        (false, _, _) => {
            let joined = if trailing { format!("{joined}/") } else { joined };
            if absolute { format!("/{joined}") } else { joined }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decoded as vscode-uri decodes them (checked with Monaco's `URI.parse`).
    #[test]
    fn escapes_are_decoded_where_they_spell_text() {
        assert_eq!(percent_decode("My%20Files/a%2Bb%2f"), "My Files/a+b/");
        assert_eq!(percent_decode("caf%C3%A9 %E6%96%87"), "café 文");
        // Not UTF-8: each escape from the first on, until the rest is.
        assert_eq!(percent_decode("bad%FF.ts"), "bad%FF.ts");
        assert_eq!(percent_decode("%E6%96%87%FFb%41"), "%E6%96%87%FFbA");
        assert_eq!(percent_decode("%FF%C3%A9"), "%FFé");
        // Not an escape.
        assert_eq!(percent_decode("%zz %4 % 100%"), "%zz %4 % 100%");
    }

    #[test]
    fn paths_are_normalized_as_node_normalizes_them() {
        assert_eq!(posix_normalize("/a/../b/./c//d.ts"), "/b/c/d.ts");
        assert_eq!(posix_normalize("/a/b/../../../c"), "/c");
        assert_eq!(posix_normalize("/dir/"), "/dir/");
        assert_eq!(posix_normalize("//host/x"), "/host/x");
        assert_eq!(posix_normalize("/"), "/");
        assert_eq!(posix_normalize("c:/x/../.."), ".");
        assert_eq!(posix_normalize("../a/.."), "..");
        assert_eq!(posix_normalize("a/../../b"), "../b");
        assert_eq!(posix_normalize("/a/../.."), "/");
        assert_eq!(posix_normalize("a/./"), "a/");
        assert_eq!(posix_normalize("./"), "./");
        assert_eq!(posix_normalize(""), ".");
    }

    #[test]
    fn a_uri_is_read_and_written_back_as_vscode_uri_does() {
        let uri = Uri::parse("FILE://Host/C:/x%20y.ts?q?r#L1").expect("a URI");
        assert_eq!((uri.scheme.as_str(), uri.authority.as_str(), uri.path.as_str()), ("FILE", "Host", "/C:/x y.ts"));
        assert_eq!((uri.query.as_str(), uri.fragment.as_str()), ("q?r", "L1"));
        assert_eq!(uri.to_display_string(), "FILE://host/c:/x y.ts?q%3Fr#L1");
        assert_eq!(Uri::parse("https://User@Example.com:8080").expect("a URI").to_display_string(), "https://User@example.com:8080/");
        assert_eq!(Uri::parse("file:///C:/x.json").expect("a URI").fs_path(), "c:/x.json");
        assert_eq!(Uri::parse("file://host/a").expect("a URI").fs_path(), "//host/a");
        // No authority but a path starting `//`: vscode-uri throws.
        assert_eq!(Uri::parse("file:////x"), None);
    }
}
