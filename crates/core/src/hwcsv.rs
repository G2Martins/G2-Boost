//! "Gravar em CSV" of the hardware monitor: one snapshot of the sensors (value, minimum,
//! maximum) written to the user's Documents folder as `G2 Boost - sensores - <data>.csv`,
//! UTF-8 with BOM so Excel reads the accents, never overwriting an existing file.
//!
//! The UI hands over text and numbers (hostile input): the row count is bounded, control
//! characters are dropped, cells are cut at 200 characters and spreadsheet formulas
//! (`=`, `+`, `-`, `@` at the start of a text cell) are neutralised before anything is written.
//! The file name and folder are ours; nothing path-like comes from the caller.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{FOLDERID_Documents, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};

pub const MAX_ROWS: usize = 5000;
const MAX_CELL: usize = 200;
/// Device, group, sensor, value, minimum, maximum, unit.
pub const COLUMNS: usize = 7;

#[derive(Debug, Clone, Deserialize)]
pub struct Row {
    pub device: String,
    pub group: String,
    pub sensor: String,
    pub value: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    /// `pt-BR` writes `;` and decimal commas (what Excel in Portuguese opens by double click).
    pub locale: String,
    /// The seven column titles, in the UI's language.
    pub header: Vec<String>,
    pub rows: Vec<Row>,
}

#[derive(Debug, thiserror::Error)]
pub enum CsvError {
    #[error("empty")]
    Empty,
    #[error("too_many_rows")]
    TooManyRows,
    #[error("bad_header")]
    BadHeader,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// (separator, decimal mark) for the UI language.
pub fn format_for(locale: &str) -> (char, char) {
    if locale.to_ascii_lowercase().starts_with("pt") { (';', ',') } else { (',', '.') }
}

/// One text cell: no control characters, at most 200 characters, a leading apostrophe when a
/// spreadsheet would read it as a formula, quoted when it holds the separator or a quote.
pub fn escape(cell: &str, sep: char) -> String {
    let mut s: String = cell.chars().filter(|c| !c.is_control()).take(MAX_CELL).collect();
    if s.starts_with(['=', '+', '-', '@']) {
        s.insert(0, '\'');
    }
    if s.contains(sep) || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// A number with up to three decimals and no trailing zeros; empty when missing or not finite.
pub fn number(v: Option<f64>, decimal: char) -> String {
    let Some(v) = v.filter(|v| v.is_finite()) else { return String::new() };
    let mut s = format!("{v:.3}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        s = "0".into();
    }
    if decimal == '.' { s } else { s.replace('.', &decimal.to_string()) }
}

/// The whole file: BOM, header, one line per row, CRLF line ends.
pub fn render(header: &[String], rows: &[Row], sep: char, decimal: char) -> String {
    let mut out = String::from('\u{FEFF}');
    let sep_s = sep.to_string();
    let mut line = |cells: Vec<String>| {
        out.push_str(&cells.join(&sep_s));
        out.push_str("\r\n");
    };
    line(header.iter().map(|h| escape(h, sep)).collect());
    for r in rows {
        line(vec![
            escape(&r.device, sep),
            escape(&r.group, sep),
            escape(&r.sensor, sep),
            number(r.value, decimal),
            number(r.min, decimal),
            number(r.max, decimal),
            escape(&r.unit, sep),
        ]);
    }
    out
}

/// Validate the request and render it.
pub fn build_text(req: &Request) -> Result<String, CsvError> {
    if req.rows.is_empty() {
        return Err(CsvError::Empty);
    }
    if req.rows.len() > MAX_ROWS {
        return Err(CsvError::TooManyRows);
    }
    if req.header.len() != COLUMNS {
        return Err(CsvError::BadHeader);
    }
    let (sep, decimal) = format_for(&req.locale);
    Ok(render(&req.header, &req.rows, sep, decimal))
}

/// `G2 Boost - sensores - 2026-10-25 14-03-22.csv` (no colons: file names cannot have them).
pub fn file_name(y: u16, mo: u16, d: u16, h: u16, mi: u16, s: u16) -> String {
    format!("G2 Boost - sensores - {y:04}-{mo:02}-{d:02} {h:02}-{mi:02}-{s:02}.csv")
}

/// Create `name` in `dir` without overwriting: if it exists, `name (2).csv`, `name (3).csv`…
pub fn save_in(dir: &Path, name: &str, text: &str) -> std::io::Result<PathBuf> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, "csv"));
    for n in 1..=99 {
        let path = if n == 1 { dir.join(name) } else { dir.join(format!("{stem} ({n}).{ext}")) };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "too many files with this name"))
}

/// The user's Documents folder (OneDrive-redirected or not), else `%USERPROFILE%\Documents`.
fn documents_dir() -> Option<PathBuf> {
    // SAFETY: the API allocates the string; it is copied, then freed with CoTaskMemFree.
    let known = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Documents, KNOWN_FOLDER_FLAG(0), None).ok().and_then(|p| {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
            s
        })
    };
    known.map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")))
}

/// Write the file and return its path (the caller reveals it in Explorer).
pub fn save(req: &Request) -> Result<PathBuf, CsvError> {
    let text = build_text(req)?;
    let dir = documents_dir().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no Documents folder"))?;
    // SAFETY: plain call, returns the struct by value.
    let t = unsafe { GetLocalTime() };
    Ok(save_in(&dir, &file_name(t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond), &text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(device: &str, sensor: &str, v: Option<f64>) -> Row {
        Row { device: device.into(), group: "Temperatures".into(), sensor: sensor.into(), value: v, min: v, max: v, unit: "°C".into() }
    }

    /// A tiny RFC 4180 reader: rows of cells, quotes undoubled, newlines allowed inside quotes.
    fn parse(text: &str, sep: char) -> Vec<Vec<String>> {
        let (mut rows, mut cur, mut cell, mut quoted) = (Vec::new(), Vec::new(), String::new(), false);
        let mut it = text.trim_start_matches('\u{FEFF}').chars().peekable();
        while let Some(c) = it.next() {
            match (c, quoted) {
                ('"', true) if it.peek() == Some(&'"') => {
                    cell.push('"');
                    it.next();
                }
                ('"', _) => quoted = !quoted,
                (c, false) if c == sep => cur.push(std::mem::take(&mut cell)),
                ('\r', false) => {}
                ('\n', false) => {
                    cur.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut cur));
                }
                (c, _) => cell.push(c),
            }
        }
        rows
    }

    #[test]
    fn escape_quotes_separators_and_quotes_inside() {
        assert_eq!(escape("CPU Package", ';'), "CPU Package");
        assert_eq!(escape("a;b", ';'), "\"a;b\"");
        assert_eq!(escape("a;b", ','), "a;b", "only the active separator forces quotes");
        assert_eq!(escape("a,b", ','), "\"a,b\"");
        assert_eq!(escape("say \"hi\"", ';'), "\"say \"\"hi\"\"\"");
        assert_eq!(escape("", ';'), "");
        assert_eq!(escape("Núcleo 3 °C", ';'), "Núcleo 3 °C");
    }

    #[test]
    fn escape_neutralises_formulas_control_characters_and_length() {
        for evil in ["=cmd|' /C calc'!A0", "+1+1", "-2+3", "@SUM(A1)"] {
            assert!(escape(evil, ';').starts_with('\''), "{evil}");
        }
        assert_eq!(escape("-12V", ';'), "'-12V", "a rail called -12V pays the price of the apostrophe");
        assert_eq!(escape("12V", ';'), "12V");
        assert_eq!(escape("a\r\nb\tc\u{7}d", ';'), "abcd", "no control characters, so no broken rows");
        assert_eq!(escape("\n=1+1", ';'), "'=1+1", "a formula hidden behind a newline is still caught");
        assert_eq!(escape(&"x".repeat(1000), ';').chars().count(), MAX_CELL);
    }

    #[test]
    fn numbers_use_the_locale_mark_and_never_print_garbage() {
        assert_eq!(number(Some(45.3), ','), "45,3");
        assert_eq!(number(Some(45.3), '.'), "45.3");
        assert_eq!(number(Some(1200.0), ','), "1200");
        assert_eq!(number(Some(12.096), '.'), "12.096");
        assert_eq!(number(Some(0.1 + 0.2), '.'), "0.3");
        assert_eq!(number(Some(-0.0004), '.'), "0");
        assert_eq!(number(Some(-3.5), ','), "-3,5");
        assert_eq!(number(Some(1.0e12), '.'), "1000000000000");
        for none in [None, Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY)] {
            assert_eq!(number(none, '.'), "");
        }
        assert_eq!(format_for("pt-BR"), (';', ','));
        assert_eq!(format_for("PT"), (';', ','));
        assert_eq!(format_for("en"), (',', '.'));
        assert_eq!(format_for(""), (',', '.'));
    }

    #[test]
    fn rendered_file_round_trips_hostile_cells_with_the_same_column_count() {
        let header: Vec<String> = ["Dispositivo", "Grupo", "Sensor", "Valor", "Mínimo", "Máximo", "Unidade"].map(String::from).to_vec();
        let rows = vec![
            row("CPU [#0]: i5; \"10400\"", "Core 0", Some(61.5)),
            row("Board", "=HYPERLINK(\"http://x\")", Some(-3.0)),
            row("Multi\nline", "VIN", None),
        ];
        for (sep, dec) in [(';', ','), (',', '.')] {
            let text = render(&header, &rows, sep, dec);
            assert!(text.starts_with('\u{FEFF}'), "BOM for Excel");
            assert!(text.contains("\r\n") && !text.contains("\n\n"));
            let table = parse(&text, sep);
            assert_eq!(table.len(), 4);
            assert!(table.iter().all(|r| r.len() == COLUMNS), "{table:?}");
            assert_eq!(table[1][0], "CPU [#0]: i5; \"10400\"");
            assert_eq!(table[1][3], if dec == ',' { "61,5" } else { "61.5" });
            assert_eq!(table[2][2], "'=HYPERLINK(\"http://x\")");
            assert_eq!(table[2][3], "-3");
            assert_eq!(table[3][0], "Multiline");
            assert_eq!(table[3][3], "", "a sensor that vanished has an empty value");
        }
    }

    #[test]
    fn request_is_validated_before_anything_is_written() {
        let header: Vec<String> = (0..COLUMNS).map(|i| format!("h{i}")).collect();
        let ok = Request { locale: "en".into(), header: header.clone(), rows: vec![row("d", "s", Some(1.0))] };
        assert!(build_text(&ok).is_ok());
        let none = Request { rows: vec![], ..ok.clone() };
        assert!(matches!(build_text(&none), Err(CsvError::Empty)));
        let huge = Request { rows: (0..=MAX_ROWS).map(|_| row("d", "s", Some(1.0))).collect(), ..ok.clone() };
        assert!(matches!(build_text(&huge), Err(CsvError::TooManyRows)));
        let at_cap = Request { rows: (0..MAX_ROWS).map(|_| row("d", "s", Some(1.0))).collect(), ..ok.clone() };
        assert!(build_text(&at_cap).is_ok());
        let short = Request { header: header[..3].to_vec(), ..ok.clone() };
        assert!(matches!(build_text(&short), Err(CsvError::BadHeader)));
        let long = Request { header: (0..20).map(|i| i.to_string()).collect(), ..ok };
        assert!(matches!(build_text(&long), Err(CsvError::BadHeader)));
    }

    #[test]
    fn file_name_follows_the_documented_pattern() {
        assert_eq!(file_name(2026, 10, 5, 9, 3, 7), "G2 Boost - sensores - 2026-10-05 09-03-07.csv");
        assert!(!file_name(2026, 10, 25, 14, 30, 0).contains(':'));
    }

    #[test]
    fn saving_never_overwrites_an_existing_file() {
        let dir = std::env::temp_dir().join(format!("g2boost-hwcsv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = "G2 Boost - sensores - test.csv";
        let a = save_in(&dir, name, "\u{FEFF}first").unwrap();
        let b = save_in(&dir, name, "second").unwrap();
        let c = save_in(&dir, name, "third").unwrap();
        assert_eq!(a.file_name().unwrap(), name);
        assert_eq!(b.file_name().unwrap(), "G2 Boost - sensores - test (2).csv");
        assert_eq!(c.file_name().unwrap(), "G2 Boost - sensores - test (3).csv");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "\u{FEFF}first", "the first file is untouched");
        assert_eq!(std::fs::read(&a).unwrap()[..3], [0xEF, 0xBB, 0xBF], "UTF-8 BOM bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn documents_folder_resolves() {
        assert!(documents_dir().is_some_and(|d| d.is_absolute()));
    }

    /// Live: writes a real file in this user's Documents folder, reads it back and deletes it.
    #[test]
    #[ignore = "live: writes (then removes) a file in the real Documents folder"]
    fn live_save_writes_a_csv_in_documents() {
        let header: Vec<String> = (0..COLUMNS).map(|i| format!("h{i}")).collect();
        let req = Request { locale: "pt-BR".into(), header, rows: vec![row("CPU", "Core 0", Some(61.5))] };
        let path = save(&req).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(path.file_name().unwrap().to_string_lossy().starts_with("G2 Boost - sensores - "));
        assert!(path.extension().is_some_and(|e| e == "csv"));
        assert_eq!(bytes[..3], [0xEF, 0xBB, 0xBF]);
        assert!(String::from_utf8(bytes).unwrap().contains("61,5"));
    }
}
