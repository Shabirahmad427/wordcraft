//! Desktop print services. Linux and macOS use the CUPS command interface to enumerate printers,
//! query advertised options, submit the preview's PDF and cancel queued jobs. Other platforms
//! retain the PDF-viewer flow through [`Services::print`].
//!
//! The viewer fallback writes a private, unique `wordcraft-print-….pdf` in the OS temp directory
//! and opens it without waiting: Preview on macOS, or the default PDF app elsewhere. Viewer-owned
//! files are kept for an hour because the viewer may open them after the app returns.
//!
//! The viewer reads the file after we return, so it can't be removed right away: files older than
//! [`KEEP`] are cleaned up off the UI thread when the app starts and after each print.
//!
//! [`Services::print`]: wordcraft_ui_egui::Services::print

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use wordcraft_ui_egui::printing::{Duplex, NativePrintBackend, NativePrintOptions, PrintJob, Printer, PrinterCapabilities};
#[cfg(target_os = "windows")]
use wordcraft_ui_egui::printing::{Duplex, NativePrintBackend, NativePrintOptions, PrintJob, Printer, PrinterCapabilities};

/// Temporary print files start with this and end with `.pdf`.
const PREFIX: &str = "wordcraft-print-";
/// How long a print file is kept for its viewer before the clean-up removes it.
const KEEP: Duration = Duration::from_secs(60 * 60);

/// The `Services::print` hook.
pub type Hook = Box<dyn Fn(&[u8]) -> Result<(), String>>;

/// CUPS/IPPs system spooler used on Linux and macOS. Windows keeps the PDF-viewer flow.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Default)]
pub struct CupsBackend;

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl NativePrintBackend for CupsBackend {
    fn printers(&self) -> Result<Vec<Printer>, String> {
        let output = std::process::Command::new("lpstat").arg("-e").output().map_err(|e| format!("CUPS is unavailable: {e}"))?;
        if !output.status.success() {
            return Err(command_error("lpstat", &output.stderr));
        }
        let defaults = std::process::Command::new("lpstat").arg("-d").output().ok();
        let default = defaults
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8_lossy(&out.stdout).split_once(':').map(|(_, name)| name.trim().to_string()))
            .unwrap_or_default();
        let names: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|name| {
                !name.is_empty() && !name.starts_with('-') && !name.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '#'))
            })
            .map(str::to_string)
            .collect();
        Ok(names.into_iter().map(|name| Printer { is_default: name == default, name }).collect())
    }

    fn capabilities(&self, printer: &str) -> Result<PrinterCapabilities, String> {
        if !self.printers()?.iter().any(|p| p.name == printer) {
            return Err("The selected printer is no longer available.".into());
        }
        let output = std::process::Command::new("lpoptions")
            .args(["-p", printer, "-l"])
            .output()
            .map_err(|e| format!("couldn't read printer options: {e}"))?;
        if !output.status.success() {
            return Err(command_error("lpoptions", &output.stderr));
        }
        Ok(parse_capabilities(&String::from_utf8_lossy(&output.stdout)))
    }

    fn submit(&self, pdf: &[u8], options: &NativePrintOptions) -> Result<PrintJob, String> {
        wordcraft_ui_egui::printing::validate_options(options)?;
        if !self.printers()?.iter().any(|p| p.name == options.printer) {
            return Err("The selected printer is no longer available.".into());
        }
        let caps = self.capabilities(&options.printer)?;
        if options.duplex != Duplex::Simplex && !caps.duplex.contains(&options.duplex) {
            return Err("The selected printer does not support that duplex mode.".into());
        }
        if options.media.as_ref().is_some_and(|media| !caps.media.contains(media)) {
            return Err("The selected paper size is no longer available.".into());
        }
        if !(1..=999).contains(&options.copies) {
            return Err("Copies must be between 1 and 999.".into());
        }
        let path = write_temp(&std::env::temp_dir(), pdf).map_err(|e| format!("couldn't prepare the print job: {e}"))?;
        let output = std::process::Command::new("lp").args(lp_arguments(&path, options)).output();
        let _ = std::fs::remove_file(&path);
        let output = output.map_err(|e| format!("CUPS could not submit the print job: {e}"))?;
        if !output.status.success() {
            return Err(command_error("lp", &output.stderr));
        }
        let receipt = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let id = parse_job_id(&receipt);
        Ok(PrintJob { id, receipt: if receipt.is_empty() { "Print job submitted.".into() } else { receipt } })
    }

    fn cancel(&self, job_id: &str) -> Result<(), String> {
        if job_id.is_empty() || !job_id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')) {
            return Err("The print queue returned an invalid job identifier.".into());
        }
        let output = std::process::Command::new("cancel").arg(job_id).output().map_err(|e| format!("couldn't cancel print job: {e}"))?;
        if output.status.success() { Ok(()) } else { Err(command_error("cancel", &output.stderr)) }
    }
}

/// Windows printer discovery, PrintTicket configuration and PDF submission through WinRT's
/// Windows.Data.Pdf renderer and the system XPS print pipeline. The WordCraft Print page supplies
/// the printer-selection and options dialog.
#[cfg(target_os = "windows")]
#[derive(Default)]
pub struct WindowsPrintBackend;

#[cfg(target_os = "windows")]
impl NativePrintBackend for WindowsPrintBackend {
    fn printers(&self) -> Result<Vec<Printer>, String> {
        use winprint::printer::PrinterDevice;

        let devices = PrinterDevice::all().map_err(|error| format!("Windows printer discovery failed: {error}"))?;
        const PRINTER_ATTRIBUTE_DEFAULT: u32 = 0x0000_0004;
        Ok(devices
            .into_iter()
            .map(|device| Printer { name: device.name().to_string(), is_default: device.os_attributes() & PRINTER_ATTRIBUTE_DEFAULT != 0 })
            .collect())
    }

    fn capabilities(&self, printer: &str) -> Result<PrinterCapabilities, String> {
        use winprint::ticket::{FeatureOptionPack, FeatureOptionPackWithPredefined, PredefinedDuplexType, PrintCapabilities};

        let device = windows_device(printer)?;
        let caps = PrintCapabilities::fetch(&device).map_err(|error| format!("Could not read printer capabilities: {error}"))?;
        let mut capabilities = PrinterCapabilities::default();
        for media in caps.page_media_sizes() {
            let size = media.size();
            let label = wordcraft_ui_egui::printing::media_label(media.display_name(), size.width_in_micron(), size.height_in_micron());
            if !label.is_empty() {
                capabilities.media.push(label);
            }
        }
        capabilities.media.sort();
        capabilities.media.dedup();
        for duplex in caps.duplexes() {
            match duplex.as_predefined_name() {
                Some(PredefinedDuplexType::TwoSidedLongEdge) => capabilities.duplex.push(Duplex::LongEdge),
                Some(PredefinedDuplexType::TwoSidedShortEdge) => capabilities.duplex.push(Duplex::ShortEdge),
                _ => {}
            }
        }
        capabilities.duplex.sort_by_key(|duplex| match duplex {
            Duplex::Simplex => 0,
            Duplex::LongEdge => 1,
            Duplex::ShortEdge => 2,
        });
        capabilities.duplex.dedup();
        Ok(capabilities)
    }

    fn submit(&self, pdf: &[u8], options: &NativePrintOptions) -> Result<PrintJob, String> {
        use winprint::{
            printer::{FilePrinter, WinPdfPrinter},
            ticket::{
                Copies, FeatureOptionPack, FeatureOptionPackWithPredefined, PredefinedDuplexType, PredefinedPageOrientation, PrintCapabilities,
                PrintTicket, PrintTicketBuilder,
            },
        };

        wordcraft_ui_egui::printing::validate_options(options)?;
        let device = windows_device(&options.printer)?;
        let caps = PrintCapabilities::fetch(&device).map_err(|error| format!("Could not read printer capabilities: {error}"))?;
        let mut ticket = PrintTicketBuilder::new(&device).map_err(|error| format!("Could not prepare printer settings: {error}"))?;
        ticket.merge(Copies(options.copies)).map_err(|error| format!("The printer rejected the copy count: {error}"))?;

        let collation = if options.collated { "Collated" } else { "Uncollated" };
        let collate_ticket = PrintTicket::from_xml(format!(
            r#"<psf:PrintTicket xmlns:psf="http://schemas.microsoft.com/windows/2003/08/printing/printschemaframework" xmlns:psk="http://schemas.microsoft.com/windows/2003/08/printing/printschemakeywords" version="1"><psf:Feature name="psk:JobCollateAllDocuments"><psf:Option name="psk:{collation}"/></psf:Feature></psf:PrintTicket>"#
        ));
        ticket.merge(collate_ticket).map_err(|error| format!("The printer does not accept the requested collation setting: {error}"))?;

        if let Some(media_name) = options.media.as_deref().filter(|media| !media.is_empty()) {
            let media = caps
                .page_media_sizes()
                .find(|media| {
                    let size = media.size();
                    wordcraft_ui_egui::printing::media_label(media.display_name(), size.width_in_micron(), size.height_in_micron()) == media_name
                })
                .ok_or_else(|| "The selected paper size is no longer available.".to_string())?;
            ticket.merge(media).map_err(|error| format!("The printer rejected the selected paper size: {error}"))?;
        }
        match options.duplex {
            Duplex::Simplex => {
                if let Some(simplex) = caps.duplexes().find(|duplex| duplex.as_predefined_name() == Some(PredefinedDuplexType::OneSided)) {
                    ticket.merge(simplex).map_err(|error| format!("The printer rejected one-sided printing: {error}"))?;
                }
            }
            Duplex::LongEdge | Duplex::ShortEdge => {
                let requested =
                    if options.duplex == Duplex::LongEdge { PredefinedDuplexType::TwoSidedLongEdge } else { PredefinedDuplexType::TwoSidedShortEdge };
                let duplex = caps
                    .duplexes()
                    .find(|duplex| duplex.as_predefined_name() == Some(requested))
                    .ok_or_else(|| "The selected duplex mode is no longer available.".to_string())?;
                ticket.merge(duplex).map_err(|error| format!("The printer rejected the selected duplex mode: {error}"))?;
            }
        }
        if options.landscape {
            let orientation = caps
                .page_orientations()
                .find(|orientation| orientation.as_predefined_name() == Some(PredefinedPageOrientation::Landscape))
                .ok_or_else(|| "This printer does not advertise landscape orientation.".to_string())?;
            ticket.merge(orientation).map_err(|error| format!("The printer rejected landscape orientation: {error}"))?;
        }
        if options.fit_to_page {
            let scale_ticket = PrintTicket::from_xml(
                r#"<psf:PrintTicket xmlns:psf="http://schemas.microsoft.com/windows/2003/08/printing/printschemaframework" xmlns:psk="http://schemas.microsoft.com/windows/2003/08/printing/printschemakeywords" version="1"><psf:Feature name="psk:PageScaling"><psf:Option name="psk:FitApplicationMediaSizeToPageImageableSize"/></psf:Feature></psf:PrintTicket>"#,
            );
            ticket.merge(scale_ticket).map_err(|error| format!("The printer does not accept fit-to-page scaling: {error}"))?;
        }

        let path = write_temp(&std::env::temp_dir(), pdf).map_err(|error| format!("Could not prepare the print job: {error}"))?;
        let result =
            WinPdfPrinter::new(device).print(&path, ticket.build().map_err(|error| format!("Could not finalize printer settings: {error}"))?);
        let _ = std::fs::remove_file(&path);
        result.map_err(|error| format!("Windows could not submit the print job: {error}"))?;
        Ok(PrintJob { id: None, receipt: "Print job submitted to Windows. Use the system print queue for job status or cancellation.".into() })
    }

    fn cancel(&self, _job_id: &str) -> Result<(), String> {
        Err("Windows did not provide a cancellable job handle; use the system print queue to cancel this job.".into())
    }
}

#[cfg(target_os = "windows")]
fn windows_device(printer: &str) -> Result<winprint::printer::PrinterDevice, String> {
    winprint::printer::PrinterDevice::all()
        .map_err(|error| format!("Windows printer discovery failed: {error}"))?
        .into_iter()
        .find(|device| device.name() == printer)
        .ok_or_else(|| "The selected printer is no longer available.".into())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn parse_job_id(receipt: &str) -> Option<String> {
    let id = receipt.split_once("request id is ")?.1.split_whitespace().next()?.trim_matches(|c: char| matches!(c, '(' | ')'));
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))).then(|| id.to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn lp_arguments(path: &Path, options: &NativePrintOptions) -> Vec<std::ffi::OsString> {
    let mut args = vec!["-d".into(), options.printer.clone().into(), "-n".into(), options.copies.to_string().into()];
    if options.collated {
        args.extend(["-o".into(), "collate=true".into()]);
    }
    let duplex = match options.duplex {
        Duplex::Simplex => Some("sides=one-sided"),
        Duplex::LongEdge => Some("sides=two-sided-long-edge"),
        Duplex::ShortEdge => Some("sides=two-sided-short-edge"),
    };
    if let Some(value) = duplex {
        args.extend(["-o".into(), value.into()]);
    }
    if let Some(media) = options.media.as_deref().filter(|s| !s.is_empty()) {
        args.extend(["-o".into(), format!("media={media}").into()]);
    }
    if options.landscape {
        args.extend(["-o".into(), "orientation-requested=4".into()]);
    }
    if options.fit_to_page {
        args.extend(["-o".into(), "fit-to-page".into()]);
    }
    args.extend(["--".into(), path.as_os_str().to_owned()]);
    args
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn parse_capabilities(text: &str) -> PrinterCapabilities {
    let mut result = PrinterCapabilities::default();
    for line in text.lines().take(256) {
        let Some((property, values)) = line.split_once(':') else { continue };
        let key = property.split('/').next().unwrap_or("").trim().to_ascii_lowercase();
        let choices: Vec<&str> = values.split_whitespace().map(|s| s.trim_start_matches('*')).filter(|s| !s.is_empty()).take(128).collect();
        if matches!(key.as_str(), "pagesize" | "media") {
            result.media.extend(
                choices.iter().filter(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))).map(|s| (*s).to_string()),
            );
        }
        if key == "duplex" || key == "sides" {
            if choices.iter().any(|s| matches!(*s, "DuplexNoTumble" | "two-sided-long-edge")) {
                result.duplex.push(Duplex::LongEdge);
            }
            if choices.iter().any(|s| matches!(*s, "DuplexTumble" | "two-sided-short-edge")) {
                result.duplex.push(Duplex::ShortEdge);
            }
        }
    }
    result.media.sort();
    result.media.dedup();
    result.duplex.sort_by_key(|d| match d {
        Duplex::Simplex => 0,
        Duplex::LongEdge => 1,
        Duplex::ShortEdge => 2,
    });
    result.duplex.dedup();
    result
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn command_error(command: &str, stderr: &[u8]) -> String {
    let reason = String::from_utf8_lossy(stderr).trim().to_string();
    if reason.is_empty() { format!("{command} failed.") } else { format!("{command}: {reason}") }
}

/// The desktop print hook; also clears print files left over from earlier runs.
pub fn hook() -> Hook {
    let dir = std::env::temp_dir();
    spawn_cleanup(dir.clone());
    Box::new(move |bytes| {
        let r = print_with(&dir, bytes, open_viewer);
        spawn_cleanup(dir.clone());
        r
    })
}

// Keep submission workers reachable until normal app shutdown; dropping a JoinHandle would
// let process exit kill PowerShell/CUPS before it hands the job to the spooler.
static JOBS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(Vec::new());

pub fn finish_pending_jobs() {
    let workers = std::mem::take(&mut *JOBS.lock().unwrap_or_else(|poison| poison.into_inner()));
    for worker in workers {
        if worker.join().is_err() {
            log::error!("the print worker stopped unexpectedly during shutdown");
        }
    }
}

pub fn submit() -> Submit {
    Box::new(|bytes, settings: Settings, collate| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new().name("print-submit".into()).spawn(move || {
            let job = craft_print::Job {
                printer: Some(settings.printer),
                copies: settings.copies.clamp(1, 999),
                collate,
                duplex: settings.duplex.map(|value| match value {
                    Duplex::LongEdge => craft_print::Duplex::LongEdge,
                    Duplex::ShortEdge => craft_print::Duplex::ShortEdge,
                    Duplex::Off => craft_print::Duplex::Off,
                }),
                grayscale: settings.grayscale,
                scaling: craft_print::Scaling::ShrinkToFit,
                title: "WordCraft document".into(),
                ..Default::default()
            };
            let _ = sender.send(craft_print::submit(&bytes, &job).map_err(|error| error.to_string()));
        });
        if let Ok(worker) = worker {
            let mut jobs = JOBS.lock().unwrap_or_else(|poison| poison.into_inner());
            jobs.retain(|job| !job.is_finished());
            jobs.push(worker);
        }
        receiver
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_waits_for_pending_submission_workers() {
        let (release, wait) = std::sync::mpsc::channel();
        let (completed, completion) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            completed.send(()).unwrap();
        });
        JOBS.lock().unwrap().push(worker);
        let shutdown = std::thread::spawn(finish_pending_jobs);
        assert!(completion.try_recv().is_err(), "the print worker is still pending");
        release.send(()).unwrap();
        shutdown.join().unwrap();
        completion.recv().unwrap();
        assert!(JOBS.lock().unwrap().is_empty(), "workers were joined before shutdown returned");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn cups_capabilities_and_options_map_only_supported_values() {
        let caps = parse_capabilities(
            "PageSize/Media Size: *Letter A4 Legal\nDuplex/2-Sided Printing: None *DuplexNoTumble DuplexTumble\nResolution/Quality: 300dpi 600dpi\n",
        );
        assert_eq!(caps.media, ["A4", "Legal", "Letter"]);
        assert_eq!(caps.duplex, [Duplex::LongEdge, Duplex::ShortEdge]);

        let args = lp_arguments(
            Path::new("/tmp/document with spaces.pdf"),
            &NativePrintOptions {
                printer: "OfficeQueue".into(),
                copies: 3,
                collated: true,
                duplex: Duplex::ShortEdge,
                media: Some("A4".into()),
                landscape: true,
                fit_to_page: true,
            },
        );
        let args: Vec<String> = args.into_iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            [
                "-d",
                "OfficeQueue",
                "-n",
                "3",
                "-o",
                "collate=true",
                "-o",
                "sides=two-sided-short-edge",
                "-o",
                "media=A4",
                "-o",
                "orientation-requested=4",
                "-o",
                "fit-to-page",
                "--",
                "/tmp/document with spaces.pdf"
            ]
        );
        assert_eq!(parse_job_id("request id is OfficeQueue-41 (1 file(s))"), Some("OfficeQueue-41".into()));
        assert_eq!(parse_job_id("unexpected lp output"), None);
    }
}
