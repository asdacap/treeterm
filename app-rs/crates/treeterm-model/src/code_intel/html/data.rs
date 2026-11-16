//! What Monaco's HTML worker knows of HTML (vscode-html-languageservice's `webCustomData` and
//! `htmlEntities`, generated into `data/html.json` and `data/entities.json` by
//! `data/generate.mjs`): the elements, their attributes and the values those take, with their
//! descriptions and MDN references, and the character entities.

use std::sync::LazyLock;

use serde::Deserialize;

/// A link the hover and the suggestions' details show (`IReference`).
#[derive(Debug, Deserialize)]
pub(crate) struct Reference {
    pub(crate) name: String,
    pub(crate) url: String,
}

/// An attribute value (`IValueData`).
#[derive(Debug, Deserialize)]
pub(crate) struct Value {
    pub(crate) name: String,
    /// Markdown.
    pub(crate) description: Option<String>,
    pub(crate) references: Vec<Reference>,
}

/// An attribute (`IAttributeData`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Attribute {
    pub(crate) name: String,
    /// Markdown.
    pub(crate) description: Option<String>,
    /// The value set its values come from; `v` for an attribute that takes none, `handler`
    /// for an event handler.
    pub(crate) value_set: Option<String>,
    pub(crate) values: Vec<Value>,
    pub(crate) references: Vec<Reference>,
}

impl Attribute {
    /// It takes no value (`valueSet: 'v'`), as `hidden`.
    pub(crate) fn is_valueless(&self) -> bool {
        self.value_set.as_deref() == Some("v")
    }
}

/// An element (`ITagData`).
#[derive(Debug, Deserialize)]
pub(crate) struct Tag {
    pub(crate) name: String,
    /// It has no end tag.
    pub(crate) void: bool,
    /// Markdown.
    pub(crate) description: Option<String>,
    pub(crate) attributes: Vec<Attribute>,
    pub(crate) references: Vec<Reference>,
}

#[derive(Debug, Deserialize)]
struct ValueSet {
    name: String,
    values: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HtmlData {
    tags: Vec<Tag>,
    global_attributes: Vec<Attribute>,
    value_sets: Vec<ValueSet>,
}

fn data() -> &'static HtmlData {
    static DATA: LazyLock<HtmlData> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../data/html.json")).unwrap_or_else(|error| panic!("data/html.json: {error}"))
    });
    &DATA
}

/// The elements, in the data's order.
pub(crate) fn tags() -> &'static [Tag] {
    &data().tags
}

/// The element named `name`, in any case.
pub(crate) fn tag(name: &str) -> Option<&'static Tag> {
    data().tags.iter().find(|tag| tag.name.eq_ignore_ascii_case(name))
}

/// Whether the element named `name` (in any case) is void: it has no end tag.
pub(crate) fn is_void(name: &str) -> bool {
    tag(name).is_some_and(|tag| tag.void)
}

/// The attributes of the element named `name` (`provideAttributes`): its own, then the
/// global ones.
pub(crate) fn attributes(name: &str) -> impl Iterator<Item = &'static Attribute> {
    let own = tag(name).map_or(&[][..], |tag| &tag.attributes[..]);
    own.iter().chain(&data().global_attributes)
}

/// The values the attribute `attribute` of the element `tag` takes (`provideValues`): those of
/// each attribute of that name (in any case), its own and its value set's.
pub(crate) fn values(tag: &str, attribute: &str) -> Vec<&'static Value> {
    let attribute = attribute.to_lowercase();
    let mut found = Vec::new();
    for entry in attributes(tag).filter(|entry| entry.name.to_lowercase() == attribute) {
        found.extend(&entry.values);
        let set = entry.value_set.as_deref().and_then(|name| data().value_sets.iter().find(|set| set.name == name));
        found.extend(set.map_or(&[][..], |set| &set.values[..]));
    }
    found
}

/// The markdown the hover and the suggestions' details show for an entry
/// (`generateDocumentation`): its description, then its references as links joined by ` | `;
/// `None` when it has neither.
pub(crate) fn documentation(description: Option<&str>, references: &[Reference]) -> Option<String> {
    let mut value = description.unwrap_or("").to_owned();
    if !references.is_empty() {
        if !value.is_empty() {
            value.push_str("\n\n");
        }
        let links: Vec<String> = references.iter().map(|reference| format!("[{}]({})", reference.name, reference.url)).collect();
        value.push_str(&links.join(" | "));
    }
    (!value.is_empty()).then_some(value)
}

/// The character entities, each name (`amp;`, and `amp` for those also read without their
/// `;`) with the text it stands for, in the data's order.
pub(crate) fn entities() -> &'static [(String, String)] {
    static ENTITIES: LazyLock<Vec<(String, String)>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../data/entities.json"))
            .unwrap_or_else(|error| panic!("data/entities.json: {error}"))
    });
    &ENTITIES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_is_read() {
        assert_eq!(tags().len(), 113);
        assert!(is_void("BR") && !is_void("div") && !is_void("nope"));
        assert_eq!(attributes("a").next().map(|attribute| attribute.name.as_str()), Some("href"));
        assert!(attributes("nope").any(|attribute| attribute.name == "class"));
        let types: Vec<&str> = values("INPUT", "Type").iter().map(|value| value.name.as_str()).collect();
        assert_eq!(types[..3], ["hidden", "text", "search"]);
        assert!(values("a", "href").is_empty());
        assert_eq!(entities().iter().find(|(name, _)| name == "amp;").map(|(_, text)| text.as_str()), Some("&"));
    }

    #[test]
    fn documentation_is_the_description_then_the_references() {
        let mdn = [Reference { name: "MDN Reference".to_owned(), url: "https://mdn/x".to_owned() }];
        assert_eq!(documentation(Some("A thing."), &mdn).as_deref(), Some("A thing.\n\n[MDN Reference](https://mdn/x)"));
        assert_eq!(documentation(None, &mdn).as_deref(), Some("[MDN Reference](https://mdn/x)"));
        assert_eq!(documentation(Some("A thing."), &[]).as_deref(), Some("A thing."));
        assert_eq!(documentation(None, &[]), None);
    }
}
