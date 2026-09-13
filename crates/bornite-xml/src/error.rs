//! Why a document was refused.

use bornite_core::CoreError;

/// One thing wrong with a document's content.
///
/// Content issues are collected across the whole document and reported together, so
/// a document with three bad attributes is fixed in one round, not three.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "issue", rename_all = "snake_case")]
#[non_exhaustive]
pub enum XmlIssueV1 {
    /// A required attribute is absent.
    #[error("<{element}> is missing the {attribute} attribute")]
    MissingAttribute {
        /// The element.
        element: &'static str,
        /// The attribute.
        attribute: &'static str,
    },
    /// An attribute is present that this element type does not use.
    ///
    /// Bornite does not ignore attributes it does not need: a `weight` on a `type="none"`
    /// quorum is most likely a mistake about what the rule does.
    #[error("<{element}> has a {attribute} attribute, which is not used when {reason}")]
    UnusedAttribute {
        /// The element.
        element: &'static str,
        /// The attribute.
        attribute: &'static str,
        /// Why it is unused.
        reason: &'static str,
    },
    /// An attribute holds a value outside its allowed set or form.
    #[error("<{element} {attribute}=\"{value}\"> is not valid: {reason}")]
    InvalidValue {
        /// The element.
        element: &'static str,
        /// The attribute.
        attribute: &'static str,
        /// The value found.
        value: String,
        /// What was expected.
        reason: String,
    },
    /// A required child element is absent.
    #[error("<{parent}> is missing its <{element}> child")]
    MissingElement {
        /// The parent.
        parent: &'static str,
        /// The missing child.
        element: &'static str,
    },
    /// A child element appears more than once where one is allowed.
    #[error("<{parent}> has more than one <{element}> child")]
    RepeatedElement {
        /// The parent.
        parent: &'static str,
        /// The repeated child.
        element: &'static str,
    },
    /// A core value could not be built from an attribute.
    #[error("<{element}>: {error}")]
    Core {
        /// The element.
        element: &'static str,
        /// The construction error.
        error: CoreError,
    },
}

/// Why a document was refused.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
#[non_exhaustive]
pub enum XmlError {
    /// The document is not well-formed, or has an element where none is allowed.
    ///
    /// Structural problems stop parsing where they are found, because nothing after an
    /// unexpected element can be trusted to mean what it appears to.
    #[error("malformed document at byte {position}: {detail}")]
    Malformed {
        /// Byte offset where parsing stopped.
        position: u64,
        /// What was wrong.
        detail: String,
    },
    /// The document's content is wrong in one or more places.
    #[error("{} issue(s): {}", .issues.len(), render(.issues))]
    Invalid {
        /// Every issue, sorted.
        issues: Vec<XmlIssueV1>,
    },
    /// The document declares a version this build does not read.
    #[error("document version {found:?} is not supported; this build reads \"1.0\"")]
    UnsupportedVersion {
        /// The version found.
        found: String,
    },
    /// The input is larger than the reader will accept.
    #[error("document is {found} bytes, over the {limit} byte limit")]
    TooLarge {
        /// Bytes supplied.
        found: u64,
        /// The limit.
        limit: u64,
    },
}

fn render(issues: &[XmlIssueV1]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
