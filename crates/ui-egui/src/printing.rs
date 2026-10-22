//! Shared print preview state and platform print-service contract.

use craft_ui::printing::Labels;
pub use craft_ui::printing::{Discover, Duplex, Feedback, Printer, Settings, State};
pub type Submit =
    Box<dyn Fn(Vec<u8>, craft_ui::printing::Settings, bool, Vec<(String, String)>) -> std::sync::mpsc::Receiver<Result<String, String>>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeDuplex {
    #[default]
    Simplex,
    LongEdge,
    ShortEdge,
}

/// Metadata for the app-owned commands. Inspecting it never discovers printers or submits.
pub fn commands(app: &crate::WordApp) -> Vec<serde_json::Value> {
    use serde_json::json;
    let enabled = (app.services.print_native.is_some() && app.printing.ready()) || app.services.print.is_some();
    let submit_params = "range?: all|current|selection|custom; pages?: string (1-3,5); parity?: all|odd|even; copies?: integer 1..999; collate?: bool; orientation?: auto|portrait|landscape; paper?: none|Letter|Legal|A4|… . Submits only when explicitly executed.";
    vec![
        json!({"id":"ui.printSettings", "label":"Print settings", "location":"File › Print", "shortcut":null, "params":"printer, copies, duplex, grayscale, refresh, status, detail", "enabled":true}),
        json!({"id":"ui.printSubmit", "label":"Submit print job", "location":"File › Print", "shortcut":null, "params":submit_params, "enabled":enabled}),
        json!({"id":"ui.print", "label":"Print (alias of ui.printSubmit)", "location":"File › Print", "shortcut":null, "params":submit_params, "enabled":enabled}),
    ]
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativePrinter {
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrinterCapabilities {
    pub media: Vec<String>,
    pub duplex: Vec<NativeDuplex>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativePrintOptions {
    pub printer: String,
    pub copies: u16,
    pub collated: bool,
    pub duplex: NativeDuplex,
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
    fn printers(&self) -> Result<Vec<NativePrinter>, String>;
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

/// IPP/PWG 5101.1 media name for a standard paper size, accepted by common print spoolers.
pub fn pwg_media(paper: &str) -> Option<&'static str> {
    Some(match paper {
        "Letter" => "na_letter_8.5x11in",
        "Legal" => "na_legal_8.5x14in",
        "Executive" => "na_executive_7.25x10.5in",
        "A3" => "iso_a3_297x420mm",
        "A4" => "iso_a4_210x297mm",
        "A5" => "iso_a5_148x210mm",
        "B4 (JIS)" => "jis_b4_257x364mm",
        "B5 (JIS)" => "jis_b5_182x257mm",
        "Tabloid" => "na_ledger_11x17in",
        "Statement" => "na_invoice_5.5x8.5in",
        "Envelope #10" => "na_number-10_4.125x9.5in",
        "Envelope DL" => "iso_dl_110x220mm",
        _ => return None,
    })
}

/// Compatibility bridge for the upstream print-settings widget. The fork's native print preview
/// uses `PrintPreviewState`; this bridge keeps upstream keyboard/control integrations available.
pub fn show(state: &mut State, ui: &mut egui::Ui, discover: &Discover, translate: impl Fn(&str) -> String) {
    let labels = Labels {
        printer: &translate("Printer"),
        select_printer: &translate("Select a printer"),
        refresh: &translate("Refresh printers"),
        copies: &translate("Copies"),
        duplex: &translate("Two-sided printing"),
        printer_default: &translate("Printer default"),
        one_sided: &translate("One-sided"),
        long_edge: &translate("Two-sided: long edge"),
        short_edge: &translate("Two-sided: short edge"),
        grayscale: &translate("Grayscale"),
    };
    state.show(ui, egui::Id::new("wordcraft_print_settings"), discover, &labels);
    if let Some(message) = &state.message {
        ui.label(feedback(message, translate));
    }
}

pub fn settings(state: &mut State, params: &serde_json::Value, discover: Option<&Discover>) -> Result<serde_json::Value, String> {
    use craft_ui::printing::{Duplex as UiDuplex, Settings};
    use serde_json::{Value, json};
    let object = params.as_object().ok_or("print settings must be an object")?;
    if object.keys().any(|key| !["printer", "copies", "duplex", "grayscale", "refresh"].contains(&key.as_str())) {
        return Err("unknown print setting; use printer, copies, duplex, grayscale or refresh".into());
    }
    if state.busy() && !object.is_empty() {
        return Err("wait for the active print submission before changing settings".into());
    }
    let mut next = state.settings.clone();
    if let Some(value) = object.get("printer") {
        let printer = value.as_str().ok_or("printer must be a name from the discovered list")?;
        if !state.printers.iter().any(|available| available.name == printer) {
            return Err("printer must be a name from the discovered list".into());
        }
        next.printer = printer.to_owned();
    }
    if let Some(value) = object.get("copies") {
        next.copies = value.as_u64().filter(|copies| (1..=999).contains(copies)).ok_or("copies must be an integer from 1 to 999")? as u32;
    }
    if let Some(value) = object.get("duplex") {
        next.duplex = match value.as_str() {
            Some("printerDefault") => None,
            Some("off") => Some(UiDuplex::Off),
            Some("longEdge") => Some(UiDuplex::LongEdge),
            Some("shortEdge") => Some(UiDuplex::ShortEdge),
            None if value.is_null() => None,
            _ => return Err("duplex must be printerDefault, off, longEdge or shortEdge".into()),
        };
    }
    if let Some(value) = object.get("grayscale") {
        next.grayscale = value.as_bool().ok_or("grayscale must be a boolean")?;
    }
    let refresh = object.get("refresh").map(|value| value.as_bool().ok_or("refresh must be a boolean")).transpose()?.unwrap_or(false);
    if refresh && !state.refresh(discover.ok_or("native printer discovery is unavailable here")?) {
        return Err("printer discovery or submission is already active".into());
    }
    state.settings = next;
    let (status, detail) = match state.message.as_ref() {
        None => ("idle", ""),
        Some(Feedback::FindingPrinters) => ("findingPrinters", ""),
        Some(Feedback::NoPrinters) => ("noPrinters", ""),
        Some(Feedback::DiscoveryFailed) => ("discoveryFailed", ""),
        Some(Feedback::Submitting) => ("submitting", ""),
        Some(Feedback::Submitted(receipt)) => ("submitted", receipt.as_str()),
        Some(Feedback::Failed(error)) => ("failed", error.as_str()),
        Some(Feedback::WorkerStopped) => ("workerStopped", ""),
    };
    let _ = std::marker::PhantomData::<Settings>;
    Ok(json!({"printer":state.settings.printer,"copies":state.settings.copies,
        "duplex":match state.settings.duplex { None=>"printerDefault",Some(UiDuplex::Off)=>"off",Some(UiDuplex::LongEdge)=>"longEdge",Some(UiDuplex::ShortEdge)=>"shortEdge" },
        "grayscale":state.settings.grayscale,"printers":state.printers.iter().map(|printer|json!({"name":printer.name,"default":printer.default})).collect::<Vec<Value>>(),
        "busy":state.busy(),"discovering":state.discovering(),"ready":state.ready(),"status":status,"detail":detail}))
}

pub fn feedback(message: &Feedback, translate: impl Fn(&str) -> String) -> String {
    match message {
        Feedback::FindingPrinters => translate("Finding printers…"),
        Feedback::NoPrinters => translate("No printers found. Configure a system printer or export a PDF."),
        Feedback::DiscoveryFailed => translate("Printer discovery failed."),
        Feedback::Submitting => translate("Submitting print job…"),
        Feedback::Submitted(receipt) if receipt.trim().is_empty() => translate("Print job submitted."),
        Feedback::Submitted(receipt) => translate("Print job submitted: {receipt}").replace("{receipt}", receipt),
        Feedback::Failed(error) => translate("Print failed: {error}").replace("{error}", error),
        Feedback::WorkerStopped => translate("Print failed: the printing worker stopped."),
    }
}

pub fn submit(app: &mut crate::WordApp, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    if app.services.print.is_none() && app.services.print_native.is_none() {
        return Err("printing to the system print dialog isn't available here; export a PDF instead".into());
    }
    if app.services.print_native.is_some() && !app.printing.ready() {
        return Err("Select an available printer in File > Print before submitting.".into());
    }
    let native = app.services.print_native.is_some();
    let mut settings = app.printing.settings.clone();
    let mut pdf_params = params.clone();
    if native {
        let object = pdf_params.as_object_mut().ok_or("print settings must be an object")?;
        if let Some(copies) = object.get("copies") {
            settings.copies = copies
                .as_u64()
                .filter(|copies| (1..=wordcraft_engine::print::MAX_COPIES as u64).contains(copies))
                .ok_or_else(|| format!("copies must be an integer from 1 to {}", wordcraft_engine::print::MAX_COPIES))?
                as u32;
        }
        // Copies are performed by the native spooler, so they must not be embedded in the PDF.
        object.insert("copies".into(), json!(1));
    } else if let Some(copies) = params.get("copies")
        && !copies.as_u64().is_some_and(|n| (1..=wordcraft_engine::print::MAX_COPIES as u64).contains(&n))
    {
        return Err(format!("copies must be an integer from 1 to {}", wordcraft_engine::print::MAX_COPIES));
    }
    if let Some(collate) = params.get("collate")
        && !collate.is_boolean()
    {
        return Err("collate must be a boolean".into());
    }
    let (pdf, plan) = app.session.print_pdf(&pdf_params).map_err(|error| error.to_string())?;
    let byte_count = pdf.len();
    if let Some(submit) = &app.services.print_native {
        if plan.chosen.checked_mul(settings.copies as usize).is_none_or(|pages| pages > wordcraft_engine::print::MAX_PRINT_PAGES) {
            return Err("the selected pages and copies exceed the print-job page limit".into());
        }
        let collated = params.get("collate").and_then(serde_json::Value::as_bool).unwrap_or(app.ui.print.collate);
        let orientation = params.get("orientation").and_then(serde_json::Value::as_str).unwrap_or(&app.ui.print.orientation);
        let landscape = match orientation {
            "landscape" => true,
            "portrait" => false,
            "auto" => app.session.export_layout().pages.first().is_some_and(|page| page.w > page.h),
            _ => return Err("orientation must be auto, portrait or landscape".into()),
        };
        let options = if landscape {
            vec![("orientation-requested".into(), "4".into()), ("landscape".into(), "true".into())]
        } else {
            vec![("orientation-requested".into(), "3".into())]
        };
        let mut options = options;
        if let Some(media) = wordcraft_engine::print::paper(params).ok().flatten().and_then(|(name, _, _)| pwg_media(name)) {
            options.push(("media".into(), media.into()));
        }
        let receiver = submit(pdf, settings, collated, options);
        app.printing.begin(receiver).map_err(|_| "a print job is already in progress".to_string())?;
        app.print_feedback_seen = None;
        return Ok(json!({"printing":true,"bytes":byte_count,"pending":true}));
    }
    if let Some(print) = &app.services.print {
        print(&pdf)?;
    }
    Ok(json!({"printing":true,"bytes":byte_count,"pending":false}))
}

pub struct PrintPreviewState {
    pub loaded: bool,
    pub printers: Vec<NativePrinter>,
    pub selected_printer: String,
    pub capabilities: PrinterCapabilities,
    pub page_range: String,
    pub copies: String,
    pub collated: bool,
    pub duplex: NativeDuplex,
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
        self.duplex = NativeDuplex::Simplex;
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
            duplex: NativeDuplex::Simplex,
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
        let mut state = PrintPreviewState { duplex: NativeDuplex::LongEdge, ..Default::default() };
        state.reset_if_printer_changed("printer", PrinterCapabilities { media: vec!["A4".into()], duplex: vec![NativeDuplex::Simplex] });
        assert_eq!(state.duplex, NativeDuplex::Simplex);
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

    /// Every Layout › Size paper has a printer media name (#2108) the spooler accepts as a keyword.
    #[test]
    fn every_paper_size_has_a_media_name() {
        for (name, _, _) in wordcraft_geom::PAPER_SIZES {
            let media = super::pwg_media(name).unwrap_or_else(|| panic!("{name} has no media name"));
            assert!(media.len() <= 64 && media.bytes().all(|b| b.is_ascii_graphic() && !matches!(b, b'/' | b':' | b'=' | b'\"')), "{media}");
        }
        assert_eq!(super::pwg_media("Quarto"), None);
    }
}
