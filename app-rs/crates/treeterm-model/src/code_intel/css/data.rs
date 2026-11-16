//! What Monaco's CSS worker documents of the CSS properties (vscode-css-languageservice's
//! `webCustomData`, generated into `data/css.json` by `data/generate.mjs`), and how its hover
//! writes that (`getEntryMarkdownDescription`).

use std::sync::LazyLock;

use serde::Deserialize;

/// A link the hover shows (`IReference`).
#[derive(Debug, Deserialize)]
struct Reference {
    name: String,
    url: String,
}

/// A property (`IPropertyData`), as the hover documents it.
#[derive(Debug, Deserialize)]
pub(super) struct Property {
    name: String,
    /// Plain text.
    description: Option<String>,
    /// `experimental`, `nonstandard` or `obsolete`; none for a standard one.
    status: Option<String>,
    /// The browsers that support it, each a code and perhaps a version (`FF49`, `IE`).
    browsers: Vec<String>,
    syntax: Option<String>,
    references: Vec<Reference>,
}

#[derive(Debug, Deserialize)]
struct CssData {
    properties: Vec<Property>,
}

/// The property named exactly `name` (`getProperty`).
pub(super) fn property(name: &str) -> Option<&'static Property> {
    static DATA: LazyLock<CssData> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../data/css.json")).unwrap_or_else(|error| panic!("data/css.json: {error}"))
    });
    DATA.properties.iter().find(|property| property.name == name)
}

/// `textToMarkedString`: `text` with markdown's syntax characters escaped, and `<` and `>`
/// written as entities.
fn marked(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '.' | '!' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// `getBrowserLabel`: `["E12","FF49","IE"]` as `Edge 12, Firefox 49, IE`, a version being the
/// digits after the code (`O3.5` is Opera 3).
fn browser_label(browsers: &[String]) -> Option<String> {
    if browsers.is_empty() {
        return None;
    }
    let label = browsers.iter().map(|browser| {
        let code_len = browser.bytes().take_while(u8::is_ascii_uppercase).count();
        let (code, rest) = browser.split_at(code_len);
        let version: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let name = match code {
            "E" => "Edge",
            "FF" => "Firefox",
            "S" => "Safari",
            "C" => "Chrome",
            "IE" => "IE",
            "O" => "Opera",
            _ => "",
        };
        if version.is_empty() { name.to_owned() } else { format!("{name} {version}") }
    });
    Some(label.collect::<Vec<_>>().join(", "))
}

impl Property {
    /// The hover's markdown (`getEntryMarkdownDescription`): its status, description,
    /// browsers, syntax and references; `None` for a property without a description.
    pub(super) fn documentation(&self) -> Option<String> {
        let description = self.description.as_deref().filter(|description| !description.is_empty())?;
        let mut value = match self.status.as_deref() {
            Some("experimental") => "\u{26a0}\u{fe0f} Property is experimental. Be cautious when using it.\u{fe0f}\n\n",
            Some("nonstandard") => "\u{1f6a8}\u{fe0f} Property is nonstandard. Avoid using it.\n\n",
            Some("obsolete") => "\u{1f6a8}\u{fe0f}\u{fe0f}\u{fe0f} Property is obsolete. Avoid using it.\n\n",
            Some(_) | None => "",
        }
        .to_owned();
        value.push_str(&marked(description));
        if let Some(browsers) = browser_label(&self.browsers) {
            value.push_str(&format!("\n\n({})", marked(&browsers)));
        }
        if let Some(syntax) = self.syntax.as_deref().filter(|syntax| !syntax.is_empty()) {
            value.push_str(&format!("\n\nSyntax: {}", marked(syntax)));
        }
        if !self.references.is_empty() {
            let links: Vec<String> =
                self.references.iter().map(|reference| format!("[{}]({})", reference.name, reference.url)).collect();
            value.push_str("\n\n");
            value.push_str(&links.join(" | "));
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As Monaco's CSS worker writes them (checked against vscode-css-languageservice).
    #[test]
    fn a_property_is_documented_with_its_status_browsers_syntax_and_references() {
        let color = property("color").expect("color").documentation();
        assert_eq!(
            color.as_deref(),
            Some(
                "Sets the color of an element's text\n\n(Edge 12, Firefox 1, Safari 1, Chrome 1, IE 3, Opera 3)\n\n\
                 Syntax: &lt;color&gt;\n\n[MDN Reference](https://developer.mozilla.org/docs/Web/CSS/color)"
            )
        );
        let zoom = property("zoom").expect("zoom").documentation().expect("documented");
        assert!(zoom.starts_with(
            "\u{1f6a8}\u{fe0f} Property is nonstandard. Avoid using it.\n\nNon\\-standard\\. Specifies the magnification"
        ));
        assert!(zoom.contains("\n\n(Edge 12, Firefox, Safari 3, Chrome 1, IE 5, Opera 15)\n\n"), "{zoom}");
        // Exactly the name, in its case.
        assert!(property("COLOR").is_none() && property("--x").is_none());
    }

    #[test]
    fn markdown_syntax_is_escaped() {
        assert_eq!(marked("a-b. [c] <d> e_f!"), "a\\-b\\. \\[c\\] &lt;d&gt; e\\_f\\!");
        assert_eq!(browser_label(&[]), None);
        assert_eq!(browser_label(&["O3.5".to_owned(), "X".to_owned()]).as_deref(), Some("Opera 3, "));
    }
}
