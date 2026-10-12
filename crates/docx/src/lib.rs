//! WordCraft DOCX reader and writer (ECMA-376 / ISO/IEC 29500 WordprocessingML).
//!
//! [`read`] turns a `.docx` package into a [`wordcraft_doc::Document`]; [`write`] does the
//! reverse. Both are written from the public specification. The reader is lenient (unknown
//! markup is skipped, bad numbers fall back to defaults) and bounded (zip entry sizes, total
//! decompressed size, XML depth and element counts are capped), so hostile input yields an
//! error or a best-effort document, never a panic.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod chart_mapping;
mod chart_spec;
mod crypt;
mod custom;
mod custom_xml;
mod opaque;
mod package;
mod preservation;
mod preserved_xml;
mod read;
mod smart_art;
mod units;
mod wordml2003;
mod write;
mod xml;

/// Validated, atomic edits to supported embedded chart data and formatting subsets.
///
/// This API returns new package bytes and never mutates its input. Callers that integrate it into
/// a document editor must treat those bytes as one document edit and retain them with Undo and
/// recovery state.
pub use chart_mapping::{
    AxisBoundChange, AxisChanges, ChartAxisSnapshot, ChartDataChanges, ChartDataSnapshot, ChartEditPlan, ChartSeriesSnapshot, DataLabelChanges,
    DataLabelSnapshot, ErrorBarSnapshot, SeriesChanges, apply_chart_edit, document_has_chart, inspect_chart_data, prepare_chart_edit,
};
/// Render an in-memory chart model through the same chart drawing engine as imported charts.
pub use chart_spec::{chart_items, chart_xml};

pub use crypt::{
    DEFAULT_SPIN_COUNT, MAX_PASSWORD_CHARS, PasswordCheck, check_password, check_protection_password, decrypt, encrypt, encrypt_with_spin_count,
    is_encrypted, protection_hash,
};
pub use flat::{XmlKind, sniff_xml, write as write_flat};
pub use read::read;
pub use smart_art::smart_art_items;
pub use write::{write, write_as};

/// Largest `.xml` file read as a Word XML document.
pub const MAX_XML_FILE: usize = 1 << 30;

/// Read a Word XML file (`.xml`): a Word XML Document (Flat OPC, the whole package in one XML
/// file) or a Word 2003 XML Document (WordprocessingML 2003), told apart by the root element.
/// Any other XML gives [`DocxError::NotWordXml`].
pub fn read_xml(bytes: &[u8]) -> Result<wordcraft_doc::Document, DocxError> {
    if bytes.len() > MAX_XML_FILE {
        return Err(DocxError::Limit(format!("XML file of {} bytes", bytes.len())));
    }
    match sniff_xml(bytes) {
        XmlKind::FlatOpc => flat::read(bytes),
        XmlKind::WordMl2003 => wordml2003::read(bytes),
        XmlKind::Other => Err(DocxError::NotWordXml),
    }
}

/// What [`DocxError::PasswordRequired`] says.
pub const PASSWORD_REQUIRED: &str = "this document is protected with a password";
/// What [`DocxError::WrongPassword`] says.
pub const WRONG_PASSWORD: &str = "the password is incorrect";

/// Drawing Canvas extension carrying canvas rotation and flips.
pub(crate) const CANVAS_SPIN_EXT: &str = "urn:wordcraft:canvas-xfrm";

/// Read a package that may be password-protected. Encrypted packages require a password.
pub fn read_with_password(bytes: &[u8], password: Option<&str>) -> Result<wordcraft_doc::Document, DocxError> {
    if is_encrypted(bytes) {
        let package = zeroize::Zeroizing::new(decrypt(bytes, password)?);
        return read::read_package(&package);
    }
    read(bytes)
}

/// Whether automatic metadata changes must be deferred to keep unsupported source XML intact.
pub fn has_preserved_package(doc: &wordcraft_doc::Document) -> bool {
    preservation::is_guarded(doc)
}

/// Known preservation-only features; this is not a complete compatibility certification.
pub fn compatibility_warnings(doc: &wordcraft_doc::Document) -> Vec<String> {
    let mut warnings = opaque::warnings(doc);
    warnings.extend(preservation::warnings(doc));
    warnings
}

/// Which kind of WordprocessingML package to write. The main part's content type differs per
/// kind, and Word refuses a file whose content type doesn't match its extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flavor {
    /// `.docx`
    #[default]
    Document,
    /// `.docm`: may carry a VBA project.
    MacroDocument,
    /// `.dotx`
    Template,
    /// `.dotm`: may carry a VBA project.
    MacroTemplate,
}

impl Flavor {
    /// The flavour a file extension (without the dot, any case) names.
    pub fn from_ext(ext: &str) -> Option<Flavor> {
        match ext.to_ascii_lowercase().as_str() {
            "docx" => Some(Flavor::Document),
            "docm" => Some(Flavor::MacroDocument),
            "dotx" => Some(Flavor::Template),
            "dotm" => Some(Flavor::MacroTemplate),
            _ => None,
        }
    }

    /// Content type of the main document part.
    pub fn main_content_type(self) -> &'static str {
        match self {
            Flavor::Document => "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            Flavor::MacroDocument => "application/vnd.ms-word.document.macroEnabled.main+xml",
            Flavor::Template => "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
            Flavor::MacroTemplate => "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
        }
    }

    /// Can this package hold macros?
    pub fn macros(self) -> bool {
        matches!(self, Flavor::MacroDocument | Flavor::MacroTemplate)
    }
}

/// Errors from reading or writing a DOCX package.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum DocxError {
    /// The bytes are not a readable zip archive.
    #[error("not a valid zip package: {0}")]
    Zip(String),
    /// A required part is absent.
    #[error("missing part: {0}")]
    MissingPart(String),
    /// A part is not well-formed XML.
    #[error("malformed XML: {0}")]
    Xml(String),
    /// The input exceeds a safety limit (size, nesting, element count).
    #[error("limit exceeded: {0}")]
    Limit(String),
    /// Imported content cannot be exported safely without losing data.
    #[error("cannot safely preserve DOCX content: {0}")]
    Preservation(String),
    /// The package is readable but isn't a WordprocessingML document.
    #[error("not a Word document: {0}")]
    NotWord(String),
    /// An `.xml` file that is neither a Word XML Document (Flat OPC) nor Word 2003 XML.
    #[error("not a Word XML document")]
    NotWordXml,
    /// The package is encrypted and no password was given.
    #[error("{}", PASSWORD_REQUIRED)]
    PasswordRequired,
    /// The password doesn't open the package.
    #[error("{}", WRONG_PASSWORD)]
    WrongPassword,
    /// The package is encrypted in a way WordCraft can't read, or its encryption data is damaged
    /// (or encrypting failed).
    #[error("encrypted document: {0}")]
    Encryption(String),
}
