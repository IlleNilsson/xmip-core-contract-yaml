//! The layout a Location binds to the YAML contract: which keys a document
//! must hold, and of what type each.
//!
//! A layout is itself a small YAML document, the same language the TOML
//! contract reads written in YAML. Every leaf is a key the document must
//! have, and its value names the type: `string`, `integer`, `float`,
//! `boolean`, `table` (a mapping), `array` (a sequence) or `datetime` (a
//! scalar in the ISO 8601 shape a YAML timestamp takes). Keys nest by mapping,
//! and an issue names the dotted path where the document departed. Anything a
//! layout does not name is not the layout's business.
//!
//! A type the layout does not know is refused when it is bound, not when a
//! Stream arrives (ADR-0042).

use contract::{ContractError, ValidationIssue};
use yaml_rust2::{Yaml, YamlLoader};

/// The seven types a layout may ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    String,
    Integer,
    Float,
    Boolean,
    Table,
    Array,
    Datetime,
}

impl Kind {
    /// The type named in a layout, if the name is one of the seven.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "string" => Self::String,
            "integer" => Self::Integer,
            "float" => Self::Float,
            "boolean" => Self::Boolean,
            "table" => Self::Table,
            "array" => Self::Array,
            "datetime" => Self::Datetime,
            _ => return None,
        })
    }

    /// Whether `value` is of this kind.
    #[must_use]
    pub fn holds(self, value: &Yaml) -> bool {
        match (self, value) {
            (Self::String, Yaml::String(_))
            | (Self::Integer, Yaml::Integer(_))
            | (Self::Float, Yaml::Real(_))
            | (Self::Boolean, Yaml::Boolean(_))
            | (Self::Table, Yaml::Hash(_))
            | (Self::Array, Yaml::Array(_)) => true,
            (Self::Datetime, Yaml::String(text)) => is_timestamp(text),
            _ => false,
        }
    }

    /// The name a layout would use for `value`.
    #[must_use]
    pub fn of(value: &Yaml) -> &'static str {
        match value {
            Yaml::String(text) if is_timestamp(text) => "datetime",
            Yaml::String(_) => "string",
            Yaml::Integer(_) => "integer",
            Yaml::Real(_) => "float",
            Yaml::Boolean(_) => "boolean",
            Yaml::Hash(_) => "table",
            Yaml::Array(_) => "array",
            Yaml::Null => "null",
            Yaml::Alias(_) => "alias",
            Yaml::BadValue => "nothing",
        }
    }
}

/// `YYYY-MM-DD`, alone or followed by a time — the shape a YAML timestamp
/// takes; the digits are not checked against the calendar.
fn is_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    digits(0..4)
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && bytes.get(7) == Some(&b'-')
        && digits(8..10)
        && matches!(bytes.get(10), None | Some(b'T' | b't' | b' '))
}

/// One key the layout requires: its dotted path and its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Required {
    pub path: String,
    pub kind: Kind,
}

/// The keys a bound layout requires, in the order the layout names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    required: Vec<Required>,
}

impl Layout {
    /// Read a layout document.
    ///
    /// # Errors
    /// The layout is not YAML, is not a mapping, a leaf is not a type name,
    /// or a name is not one of the seven types.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let documents = YamlLoader::load_from_str(text).map_err(|error| ContractError {
            message: format!("the layout is not YAML: {error}"),
        })?;
        let mut required = Vec::new();
        match documents.first() {
            Some(Yaml::Hash(_)) => collect(&documents[0], "", &mut required)?,
            Some(other) => {
                return Err(ContractError {
                    message: format!("the layout is {}; a layout is a mapping", Kind::of(other)),
                });
            }
            None => {}
        }
        Ok(Self { required })
    }

    /// The keys required, in layout order.
    #[must_use]
    pub fn required(&self) -> &[Required] {
        &self.required
    }

    /// Every way `document` departs from the layout: a required key that is
    /// missing, or one of another type than the layout names.
    #[must_use]
    pub fn check(&self, document: &Yaml) -> Vec<ValidationIssue> {
        self.required
            .iter()
            .filter_map(|required| match lookup(document, &required.path) {
                None => Some(ValidationIssue {
                    code: "required".to_string(),
                    message: format!("{} is required", required.path),
                    path: Some(required.path.clone()),
                }),
                Some(value) if !required.kind.holds(value) => Some(ValidationIssue {
                    code: "type".to_string(),
                    message: format!(
                        "{} is {}, the layout asks for {:?}",
                        required.path,
                        Kind::of(value),
                        required.kind
                    )
                    .to_lowercase(),
                    path: Some(required.path.clone()),
                }),
                Some(_) => None,
            })
            .collect()
    }
}

fn collect(mapping: &Yaml, prefix: &str, into: &mut Vec<Required>) -> Result<(), ContractError> {
    let Yaml::Hash(entries) = mapping else {
        return Ok(());
    };
    for (key, value) in entries {
        let key = key
            .as_str()
            .map_or_else(|| format!("{key:?}"), str::to_string);
        let path = if prefix.is_empty() {
            key
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Yaml::Hash(_) => collect(value, &path, into)?,
            Yaml::String(name) => match Kind::named(name) {
                Some(kind) => into.push(Required { path, kind }),
                None => {
                    return Err(ContractError {
                        message: format!(
                            "{path} asks for {name:?}; a layout type is string, integer, \
                             float, boolean, table, array or datetime"
                        ),
                    });
                }
            },
            other => {
                return Err(ContractError {
                    message: format!(
                        "{path} is {}; a layout leaf names a type as a string",
                        Kind::of(other)
                    ),
                });
            }
        }
    }
    Ok(())
}

fn lookup<'a>(document: &'a Yaml, path: &str) -> Option<&'a Yaml> {
    let mut current = document;
    for segment in path.split('.') {
        let Yaml::Hash(entries) = current else {
            return None;
        };
        current = entries.get(&Yaml::String(segment.to_string()))?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(text: &str) -> Yaml {
        YamlLoader::load_from_str(text)
            .expect("yaml")
            .into_iter()
            .next()
            .expect("a document")
    }

    #[test]
    fn a_layout_lists_its_keys_by_dotted_path() {
        let layout = Layout::parse("service:\n  name: string\n  port: integer\nmodules: array")
            .expect("a layout");
        let paths: Vec<(&str, Kind)> = layout
            .required()
            .iter()
            .map(|r| (r.path.as_str(), r.kind))
            .collect();
        assert_eq!(
            paths,
            vec![
                ("service.name", Kind::String),
                ("service.port", Kind::Integer),
                ("modules", Kind::Array)
            ]
        );
        assert!(Layout::parse("").expect("empty").required().is_empty());
    }

    #[test]
    fn an_unknown_type_a_leaf_that_is_not_a_name_or_a_non_mapping_is_refused() {
        let unknown = Layout::parse("port: number").expect_err("refused");
        assert!(unknown.message.contains("port asks for \"number\""));
        let not_a_name = Layout::parse("port: 80").expect_err("refused");
        assert!(not_a_name.message.contains("port is integer"));
        let sequence = Layout::parse("- a\n- b").expect_err("refused");
        assert!(sequence.message.contains("a layout is a mapping"));
        assert!(Layout::parse("a: [1, 2").is_err());
    }

    #[test]
    fn a_missing_key_and_a_key_of_the_wrong_type_are_named_by_their_path() {
        let layout = Layout::parse(
            "service:\n  name: string\n  port: integer\nstarted: datetime\nratio: float",
        )
        .expect("a layout");
        let issues = layout.check(&document("service:\n  name: 1\n  port: 80\nratio: 0.5"));
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert_eq!(issues[0].code, "type");
        assert_eq!(issues[0].path.as_deref(), Some("service.name"));
        assert!(issues[0].message.contains("is integer"));
        assert_eq!(issues[1].code, "required");
        assert_eq!(issues[1].path.as_deref(), Some("started"));
        let held = document(
            "service:\n  name: edge\n  port: 80\nstarted: 2026-09-10T08:00:00Z\nratio: 1.5",
        );
        assert!(layout.check(&held).is_empty(), "{:?}", layout.check(&held));
        let not_a_date = document("service: {name: a, port: 1}\nstarted: soon\nratio: 1.0");
        let issues = layout.check(&not_a_date);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path.as_deref(), Some("started"));
    }
}
