#![forbid(unsafe_code)]

//! The YAML content contract — a technology of `xmip-core-contract`.
//!
//! Two claims (ADR-0042): **well-formedness is a given** — every Stream is
//! parsed as YAML and a failure names its line and column — and **conformance
//! is a given once the contract is named** — a Receive or Send Location that
//! refers to this contract with a layout bound has every Stream held to the
//! keys and types the layout names. A bare contract is the first claim alone.
//!
//! The layout language is the one the TOML contract reads, written in YAML,
//! and [`layout`] documents it. A Stream may carry several documents; every
//! one must be well-formed, and the first is the one a layout is held to.

pub mod layout;

use contract::{
    Contract, ContractDescriptor, ContractError, ContractFactory, ContractId, ValidationIssue,
    ValidationResult,
};
use layout::Layout;
use stream::Stream;
use yaml_rust2::{ScanError, Yaml, YamlLoader};

const REPRESENTATION: &str = "application/yaml";

/// The YAML contract, bare or bound to a layout.
pub struct YamlContract {
    descriptor: ContractDescriptor,
    layout: Option<Layout>,
}

impl YamlContract {
    /// Well-formedness only.
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptor: descriptor("yaml"),
            layout: None,
        }
    }

    /// Well-formedness and conformance to `layout`, named `name` in the
    /// descriptor.
    #[must_use]
    pub fn with_layout(name: &str, layout: Layout) -> Self {
        Self {
            descriptor: descriptor(&format!("yaml:{name}")),
            layout: Some(layout),
        }
    }

    /// Whether a layout is bound.
    #[must_use]
    pub const fn is_bound(&self) -> bool {
        self.layout.is_some()
    }
}

impl Default for YamlContract {
    fn default() -> Self {
        Self::new()
    }
}

fn descriptor(id: &str) -> ContractDescriptor {
    ContractDescriptor {
        id: ContractId(id.to_string()),
        version: "1".to_string(),
        representation: REPRESENTATION.to_string(),
    }
}

impl Contract for YamlContract {
    fn descriptor(&self) -> &ContractDescriptor {
        &self.descriptor
    }

    fn identify(&self, stream: &Stream) -> Result<bool, ContractError> {
        if let Some(media_type) = stream.media_type() {
            return Ok(is_yaml_media_type(media_type));
        }
        Ok(std::str::from_utf8(stream.bytes()).is_ok_and(looks_like_yaml))
    }

    fn validate(&self, stream: &Stream) -> Result<ValidationResult, ContractError> {
        let text = std::str::from_utf8(stream.bytes()).map_err(|error| ContractError {
            message: format!("not UTF-8 text: {error}"),
        })?;
        let documents = match YamlLoader::load_from_str(text) {
            Ok(documents) => documents,
            Err(error) => return Ok(malformed(&error)),
        };
        let issues = match &self.layout {
            None => Vec::new(),
            Some(layout) => match documents.first() {
                Some(document) => layout.check(document),
                None => vec![ValidationIssue {
                    code: "required".to_string(),
                    message: "the Stream holds no document".to_string(),
                    path: Some(String::new()),
                }],
            },
        };
        Ok(ValidationResult {
            valid: issues.is_empty(),
            issues,
        })
    }
}

fn is_yaml_media_type(media_type: &str) -> bool {
    let essence = media_type.split(';').next().unwrap_or("").trim();
    ["application/yaml", "application/x-yaml", "text/yaml"]
        .iter()
        .any(|known| essence.eq_ignore_ascii_case(known))
        || essence.ends_with("+yaml")
}

/// The first line that is neither blank nor a comment is a document marker,
/// a directive, a sequence item, or a `key: value` with a YAML-shaped key.
fn looks_like_yaml(text: &str) -> bool {
    let Some(line) = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
    else {
        return false;
    };
    if line == "---" || line.starts_with("--- ") || line.starts_with("%YAML") {
        return true;
    }
    if line == "-" || line.starts_with("- ") {
        return true;
    }
    line.split_once(':').is_some_and(|(key, rest)| {
        let key = key.trim();
        !key.is_empty()
            && !key.starts_with(['[', '{', '"', '\''])
            && !key.contains('=')
            && !key.contains('[')
            && (rest.is_empty() || rest.starts_with(' '))
    })
}

fn malformed(error: &ScanError) -> ValidationResult {
    let marker = error.marker();
    ValidationResult {
        valid: false,
        issues: vec![ValidationIssue {
            code: "malformed".to_string(),
            message: format!("not valid YAML: {}", error.info()),
            path: Some(format!(
                "line {} column {}",
                marker.line(),
                marker.col() + 1
            )),
        }],
    }
}

/// Loads the contract a Location names: `yaml` or an empty reference is the
/// bare contract, anything else is the path of a layout file.
pub struct YamlFactory;

impl ContractFactory for YamlFactory {
    fn technology(&self) -> &'static str {
        "yaml"
    }

    fn load(&self, reference: &str) -> Result<Box<dyn Contract>, ContractError> {
        let reference = reference.trim();
        if reference.is_empty() || reference == self.technology() {
            return Ok(Box::new(YamlContract::new()));
        }
        let text = std::fs::read_to_string(reference).map_err(|error| ContractError {
            message: format!("cannot read layout {reference}: {error}"),
        })?;
        let layout = Layout::parse(&text).map_err(|error| ContractError {
            message: format!("layout {reference}: {error}"),
        })?;
        let name = std::path::Path::new(reference)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(reference);
        Ok(Box::new(YamlContract::with_layout(name, layout)))
    }
}

/// The first document of a well-formed Stream, for callers that want the
/// value and not only the verdict.
///
/// # Errors
/// The Stream is not UTF-8 or not YAML.
pub fn first_document(stream: &Stream) -> Result<Option<Yaml>, ContractError> {
    let text = std::str::from_utf8(stream.bytes()).map_err(|error| ContractError {
        message: format!("not UTF-8 text: {error}"),
    })?;
    let documents = YamlLoader::load_from_str(text).map_err(|error| ContractError {
        message: format!("not valid YAML: {error}"),
    })?;
    Ok(documents.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcore::StreamId;

    fn stream(text: &str, media_type: Option<&str>) -> Stream {
        Stream::new(
            StreamId::new(1),
            text.as_bytes().to_vec(),
            media_type.map(str::to_string),
        )
    }

    fn service_layout() -> Layout {
        Layout::parse("service:\n  name: string\n  port: integer").expect("a layout")
    }

    #[test]
    fn the_bare_contract_holds_well_formed_yaml_and_names_where_it_breaks() {
        let bare = YamlContract::new();
        assert!(!bare.is_bound());
        assert_eq!(bare.descriptor().id.0, "yaml");
        let held = bare
            .validate(&stream("any:\n  shape: true\n---\nsecond: 1", None))
            .expect("validates");
        assert!(held.valid, "{:?}", held.issues);
        let broken = bare
            .validate(&stream("service:\n  name: a\n  port: [1, 2\n", None))
            .expect("validates");
        assert!(!broken.valid);
        assert_eq!(broken.issues[0].code, "malformed");
        assert!(
            broken.issues[0]
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with("line ") && path.contains(" column ")),
            "{:?}",
            broken.issues[0].path
        );
        let bytes = Stream::new(StreamId::new(2), vec![0xff, 0xfe], None);
        assert!(
            bare.validate(&bytes).is_err(),
            "not text is an error, not an issue"
        );
    }

    #[test]
    fn the_bound_contract_holds_the_layout_and_names_every_departure() {
        let bound = YamlContract::with_layout("service", service_layout());
        assert!(bound.is_bound());
        assert_eq!(bound.descriptor().id.0, "yaml:service");
        let held = bound
            .validate(&stream(
                "service:\n  name: edge\n  port: 80\nextra: 1",
                None,
            ))
            .expect("validates");
        assert!(held.valid, "{:?}", held.issues);
        let departed = bound
            .validate(&stream("service:\n  name: 1", None))
            .expect("validates");
        assert!(!departed.valid);
        let codes: Vec<(&str, Option<&str>)> = departed
            .issues
            .iter()
            .map(|issue| (issue.code.as_str(), issue.path.as_deref()))
            .collect();
        assert_eq!(
            codes,
            vec![
                ("type", Some("service.name")),
                ("required", Some("service.port"))
            ]
        );
        let empty = bound.validate(&stream("", None)).expect("validates");
        assert_eq!(empty.issues[0].message, "the Stream holds no document");
    }

    #[test]
    fn identifies_by_media_type_or_by_the_first_line() {
        let bare = YamlContract::new();
        let is = |text: &str, media: Option<&str>| bare.identify(&stream(text, media)).expect("ok");
        assert!(is("x", Some("application/yaml")));
        assert!(is("x", Some("application/x-yaml")));
        assert!(is("x", Some("text/yaml; charset=utf-8")));
        assert!(!is("a: 1", Some("text/csv")));
        assert!(is("# comment\n\n---\nservice:\n  name: a", None));
        assert!(is("name: a", None));
        assert!(is("- one\n- two", None));
        assert!(!is("name = \"a\"", None));
        assert!(!is("{\"name\": \"a\"}", None));
        assert!(!is("tags[2]: a,b", None));
        assert!(!is("", None));
    }

    #[test]
    fn the_factory_loads_bare_and_bound_and_refuses_a_bad_layout() {
        let factory = YamlFactory;
        assert_eq!(factory.technology(), "yaml");
        assert_eq!(factory.load("").expect("bare").descriptor().id.0, "yaml");
        assert_eq!(
            factory.load("yaml").expect("bare").descriptor().id.0,
            "yaml"
        );
        let dir = std::env::temp_dir().join("xmip-contract-yaml-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("service.layout.yaml");
        std::fs::write(&file, "service:\n  name: string").expect("write layout");
        let bound = factory.load(file.to_str().expect("path")).expect("bound");
        assert_eq!(bound.descriptor().id.0, "yaml:service.layout");
        let bad = dir.join("bad.layout.yaml");
        std::fs::write(&bad, "service:\n  name: text").expect("write layout");
        let refused = factory
            .load(bad.to_str().expect("path"))
            .err()
            .expect("refused");
        assert!(refused.message.contains("service.name asks for \"text\""));
        assert!(
            factory
                .load(dir.join("missing.yaml").to_str().expect("path"))
                .is_err()
        );
    }

    #[test]
    fn the_first_document_comes_back_as_a_value() {
        let first = first_document(&stream("a: 1\n---\nb: 2", None)).expect("yaml");
        assert_eq!(first.expect("a document")["a"].as_i64(), Some(1));
        assert!(first_document(&stream("", None)).expect("yaml").is_none());
        assert!(first_document(&stream("a: [", None)).is_err());
    }
}
