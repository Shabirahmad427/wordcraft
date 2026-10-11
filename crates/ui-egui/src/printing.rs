//! Shared print preview state and platform print-service contract.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Duplex {
    #[default]
    Simplex,
    LongEdge,
    ShortEdge,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Printer {
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrinterCapabilities {
    pub media: Vec<String>,
    pub duplex: Vec<Duplex>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativePrintOptions {
    pub printer: String,
    pub copies: u16,
    pub collated: bool,
    pub duplex: Duplex,
    pub media: Option<String>,
    pub landscape: bool,
    pub fit_to_page: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrintJob {
    pub id: Option<String>,
    pub receipt: String,
}

/// Implemented by each desktop host. Calls are passed as data, never interpolated into a shell.
pub trait PrintBackend {
    fn printers(&self) -> Result<Vec<Printer>, String>;
    fn capabilities(&self, printer: &str) -> Result<PrinterCapabilities, String>;
    fn submit(&self, pdf: &[u8], options: &NativePrintOptions) -> Result<PrintJob, String>;
    fn cancel(&self, job_id: &str) -> Result<(), String>;
}

/// Compatibility name for the existing desktop service field.
pub use PrintBackend as NativePrintBackend;

/// Validate settings shared across platform backends before touching a printer or spooler.
pub fn validate_options(options: &NativePrintOptions) -> Result<(), String> {
    if options.printer.trim().is_empty() {
        return Err("Select a printer before printing.".into());
    }
    if !(1..=999).contains(&options.copies) {
        return Err("Copies must be between 1 and 999.".into());
    }
    Ok(())
}

/// Return a usable name for a discovered media option, including drivers that omit display names.
pub fn media_label(display_name: Option<&str>, width_microns: u32, height_microns: u32) -> String {
    display_name
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} × {} mm", width_microns / 1000, height_microns / 1000))
}

pub struct PrintPreviewState {
    pub loaded: bool,
    pub printers: Vec<Printer>,
    pub selected_printer: String,
    pub capabilities: PrinterCapabilities,
    pub page_range: String,
    pub copies: String,
    pub collated: bool,
    pub duplex: Duplex,
    pub media: String,
    pub landscape: bool,
    pub fit_to_page: bool,
    pub message: Option<String>,
    pub error: Option<String>,
    pub last_job: Option<String>,
}

impl PrintPreviewState {
    pub fn reset_if_printer_changed(&mut self, printer: &str, capabilities: PrinterCapabilities) {
        self.selected_printer = printer.to_string();
        self.media.clear();
        self.capabilities = capabilities;
        self.duplex = Duplex::Simplex;
    }
}

impl Default for PrintPreviewState {
    fn default() -> Self {
        Self {
            loaded: false,
            printers: Vec::new(),
            selected_printer: String::new(),
            capabilities: PrinterCapabilities::default(),
            page_range: String::new(),
            copies: "1".into(),
            collated: true,
            duplex: Duplex::Simplex,
            media: String::new(),
            landscape: false,
            fit_to_page: false,
            message: None,
            error: None,
            last_job: None,
        }
    }
}

/// Parse a human page range into zero-based page indexes. Empty selects all pages.
pub fn parse_page_range(input: &str, page_count: usize) -> Result<Vec<usize>, String> {
    if page_count == 0 {
        return Err("The document has no printable pages.".into());
    }
    let input = input.trim();
    if input.is_empty() {
        return Ok((0..page_count).collect());
    }
    if input.len() > 4096 {
        return Err("The page range is too long.".into());
    }
    let mut pages = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for range in input.split(',') {
        let range = range.trim();
        if range.is_empty() {
            return Err("Enter pages like 1-3,5.".into());
        }
        let (start, end) = match range.split_once('-') {
            Some((start, end)) => (parse_page_number(start, page_count)?, parse_page_number(end, page_count)?),
            None => {
                let page = parse_page_number(range, page_count)?;
                (page, page)
            }
        };
        if start > end {
            return Err("A page range must start before it ends.".into());
        }
        for page in start..=end {
            if !seen.insert(page) {
                return Err("A page appears more than once in the range.".into());
            }
            pages.push(page);
        }
    }
    Ok(pages)
}

fn parse_page_number(input: &str, page_count: usize) -> Result<usize, String> {
    let page = input.trim().parse::<usize>().map_err(|_| "Enter pages like 1-3,5.".to_string())?;
    if page == 0 || page > page_count {
        return Err(format!("Page {page} is outside this document (1–{page_count})."));
    }
    Ok(page - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges_are_one_based_bounded_and_ordered() {
        assert_eq!(parse_page_range("", 5).unwrap(), [0, 1, 2, 3, 4]);
        assert_eq!(parse_page_range(" 1-3, 5 ", 5).unwrap(), [0, 1, 2, 4]);
        assert!(parse_page_range("0", 5).is_err());
        assert!(parse_page_range("1-6", 5).is_err());
        assert!(parse_page_range("3-2", 5).is_err());
        assert!(parse_page_range("2,2", 5).is_err());
        assert!(parse_page_range("1,,3", 5).is_err());
    }

    #[test]
    fn selecting_another_printer_resets_unsupported_settings() {
        let mut state = PrintPreviewState { duplex: Duplex::LongEdge, ..Default::default() };
        state.reset_if_printer_changed("printer", PrinterCapabilities { media: vec!["A4".into()], duplex: vec![Duplex::Simplex] });
        assert_eq!(state.duplex, Duplex::Simplex);
        assert!(state.media.is_empty(), "the new printer's default media remains selected");
    }

    #[test]
    fn shared_print_options_reject_missing_printers_and_bad_copy_counts() {
        let mut options = NativePrintOptions { copies: 1, ..Default::default() };
        assert!(validate_options(&options).is_err());
        options.printer = "queue".into();
        assert!(validate_options(&options).is_ok());
        options.copies = 0;
        assert!(validate_options(&options).is_err());
        options.copies = 1000;
        assert!(validate_options(&options).is_err());
    }

    #[test]
    fn media_without_a_driver_label_still_has_a_stable_size_label() {
        assert_eq!(media_label(Some("Letter"), 215_900, 279_400), "Letter");
        assert_eq!(media_label(None, 210_000, 297_000), "210 × 297 mm");
        assert_eq!(media_label(Some(" "), 0, 0), "0 × 0 mm");
    }
}
