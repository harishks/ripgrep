use std::path::Path;

use regex::bytes::Regex;
use term::{Attr, Terminal};
use term::color;

use types::FileTypeDef;

/// Printer encapsulates all output logic for searching.
///
/// Note that we currently ignore all write errors. It's probably worthwhile
/// to fix this, but printers are only ever used for writes to stdout or
/// writes to memory, neither of which commonly fail.
pub struct Printer<W> {
    /// The underlying writer.
    wtr: W,
    /// Whether anything has been printed to wtr yet.
    has_printed: bool,
    /// Whether to show column numbers for the first match or not.
    column: bool,
    /// The string to use to separate non-contiguous runs of context lines.
    context_separator: Vec<u8>,
    /// The end-of-line terminator used by the printer. In general, eols are
    /// printed via the match directly, but occasionally we need to insert them
    /// ourselves (for example, to print a context separator).
    eol: u8,
    /// Whether to show file name as a heading or not.
    ///
    /// N.B. If with_filename is false, then this setting has no effect.
    heading: bool,
    /// Whether to show every match on its own line.
    line_per_match: bool,
    /// Whether to suppress all output.
    quiet: bool,
    /// A string to use as a replacement of each match in a matching line.
    replace: Option<Vec<u8>>,
    /// Whether to prefix each match with the corresponding file name.
    with_filename: bool,
}

impl<W: Terminal + Send> Printer<W> {
    /// Create a new printer that writes to wtr.
    pub fn new(wtr: W) -> Printer<W> {
        Printer {
            wtr: wtr,
            has_printed: false,
            column: false,
            context_separator: "--".to_string().into_bytes(),
            eol: b'\n',
            heading: false,
            line_per_match: false,
            quiet: false,
            replace: None,
            with_filename: false,
        }
    }

    /// When set, column numbers will be printed for the first match on each
    /// line.
    pub fn column(mut self, yes: bool) -> Printer<W> {
        self.column = yes;
        self
    }

    /// Set the context separator. The default is `--`.
    pub fn context_separator(mut self, sep: Vec<u8>) -> Printer<W> {
        self.context_separator = sep;
        self
    }

    /// Set the end-of-line terminator. The default is `\n`.
    pub fn eol(mut self, eol: u8) -> Printer<W> {
        self.eol = eol;
        self
    }

    /// Whether to show file name as a heading or not.
    ///
    /// N.B. If with_filename is false, then this setting has no effect.
    pub fn heading(mut self, yes: bool) -> Printer<W> {
        self.heading = yes;
        self
    }

    /// Whether to show every match on its own line.
    pub fn line_per_match(mut self, yes: bool) -> Printer<W> {
        self.line_per_match = yes;
        self
    }

    /// When set, all output is suppressed.
    pub fn quiet(mut self, yes: bool) -> Printer<W> {
        self.quiet = yes;
        self
    }

    /// Replace every match in each matching line with the replacement string
    /// given.
    ///
    /// The replacement string syntax is documented here:
    /// https://doc.rust-lang.org/regex/regex/bytes/struct.Captures.html#method.expand
    pub fn replace(mut self, replacement: Vec<u8>) -> Printer<W> {
        self.replace = Some(replacement);
        self
    }

    /// When set, each match is prefixed with the file name that it came from.
    pub fn with_filename(mut self, yes: bool) -> Printer<W> {
        self.with_filename = yes;
        self
    }

    /// Returns true if and only if something has been printed.
    pub fn has_printed(&self) -> bool {
        self.has_printed
    }

    /// Flushes the underlying writer and returns it.
    pub fn into_inner(mut self) -> W {
        let _ = self.wtr.flush();
        self.wtr
    }

    /// Prints a type definition.
    pub fn type_def(&mut self, def: &FileTypeDef) {
        self.write(def.name().as_bytes());
        self.write(b": ");
        let mut first = true;
        for pat in def.patterns() {
            if !first {
                self.write(b", ");
            }
            self.write(pat.as_bytes());
            first = false;
        }
        self.write_eol();
    }

    /// Prints the given path.
    pub fn path<P: AsRef<Path>>(&mut self, path: P) {
        self.write(path.as_ref().to_string_lossy().as_bytes());
        self.write_eol();
    }

    /// Prints the given path and a count of the number of matches found.
    pub fn path_count<P: AsRef<Path>>(&mut self, path: P, count: u64) {
        if self.with_filename {
            self.write(path.as_ref().to_string_lossy().as_bytes());
            self.write(b":");
        }
        self.write(count.to_string().as_bytes());
        self.write_eol();
    }

    /// Prints the context separator.
    pub fn context_separate(&mut self) {
        // N.B. We can't use `write` here because of borrowing restrictions.
        if self.quiet {
            return;
        }
        if self.context_separator.is_empty() {
            return;
        }
        self.has_printed = true;
        let _ = self.wtr.write_all(&self.context_separator);
        let _ = self.wtr.write_all(&[self.eol]);
    }

    pub fn matched<P: AsRef<Path>>(
        &mut self,
        re: &Regex,
        path: P,
        buf: &[u8],
        start: usize,
        end: usize,
        line_number: Option<u64>,
    ) {
        if !self.line_per_match {
            let column =
                if self.column {
                    Some(re.find(&buf[start..end])
                            .map(|(s, _)| s + 1).unwrap_or(0) as u64)
                } else {
                    None
                };
            return self.write_match(
                re, path, buf, start, end, line_number, column);
        }
        for (s, _) in re.find_iter(&buf[start..end]) {
            let column = if self.column { Some(s as u64) } else { None };
            self.write_match(
                re, path.as_ref(), buf, start, end, line_number, column);
        }
    }

    fn write_match<P: AsRef<Path>>(
        &mut self,
        re: &Regex,
        path: P,
        buf: &[u8],
        start: usize,
        end: usize,
        line_number: Option<u64>,
        column: Option<u64>,
    ) {
        if self.heading && self.with_filename && !self.has_printed {
            self.write_heading(path.as_ref());
        } else if !self.heading && self.with_filename {
            self.write(path.as_ref().to_string_lossy().as_bytes());
            self.write(b":");
        }
        if let Some(line_number) = line_number {
            self.line_number(line_number, b':');
        }
        if let Some(c) = column {
            self.write((c + 1).to_string().as_bytes());
            self.write(b":");
        }
        if self.replace.is_some() {
            let line = re.replace_all(
                &buf[start..end], &**self.replace.as_ref().unwrap());
            self.write(&line);
        } else {
            self.write_matched_line(re, &buf[start..end]);
        }
        if buf[start..end].last() != Some(&self.eol) {
            self.write_eol();
        }
    }

    fn write_matched_line(&mut self, re: &Regex, buf: &[u8]) {
        if !self.wtr.supports_color() {
            self.write(buf);
            return;
        }
        let mut last_written = 0;
        for (s, e) in re.find_iter(buf) {
            self.write(&buf[last_written..s]);
            let _ = self.wtr.fg(color::BRIGHT_RED);
            let _ = self.wtr.attr(Attr::Bold);
            self.write(&buf[s..e]);
            let _ = self.wtr.reset();
            last_written = e;
        }
        self.write(&buf[last_written..]);
    }

    pub fn context<P: AsRef<Path>>(
        &mut self,
        path: P,
        buf: &[u8],
        start: usize,
        end: usize,
        line_number: Option<u64>,
    ) {
        if self.heading && self.with_filename && !self.has_printed {
            self.write_heading(path.as_ref());
        } else if !self.heading && self.with_filename {
            self.write(path.as_ref().to_string_lossy().as_bytes());
            self.write(b"-");
        }
        if let Some(line_number) = line_number {
            self.line_number(line_number, b'-');
        }
        self.write(&buf[start..end]);
        if buf[start..end].last() != Some(&self.eol) {
            self.write_eol();
        }
    }

    fn write_heading<P: AsRef<Path>>(&mut self, path: P) {
        if self.wtr.supports_color() {
            let _ = self.wtr.fg(color::BRIGHT_GREEN);
            let _ = self.wtr.attr(Attr::Bold);
        }
        self.write(path.as_ref().to_string_lossy().as_bytes());
        self.write_eol();
        if self.wtr.supports_color() {
            let _ = self.wtr.reset();
        }
    }

    fn line_number(&mut self, n: u64, sep: u8) {
        if self.wtr.supports_color() {
            let _ = self.wtr.fg(color::BRIGHT_BLUE);
            let _ = self.wtr.attr(Attr::Bold);
        }
        self.write(n.to_string().as_bytes());
        if self.wtr.supports_color() {
            let _ = self.wtr.reset();
        }
        self.write(&[sep]);
    }

    fn write(&mut self, buf: &[u8]) {
        if self.quiet {
            return;
        }
        self.has_printed = true;
        let _ = self.wtr.write_all(buf);
    }

    fn write_eol(&mut self) {
        let eol = self.eol;
        self.write(&[eol]);
    }
}
