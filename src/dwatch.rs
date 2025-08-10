use std::{
    cell::RefCell,
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    io::Write,
    ops::Range,
    sync::{atomic::Ordering, Arc},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use itertools::Itertools;
use itertools::{
    izip,
    EitherOrBoth::{Both, Left, Right},
};

use crate::{
    options::Options,
    styles::{Styles, TOTAL_FOCUSABLE_ITEMS},
};
use crate::{ranges::RangeParser, styles::WRITERS};
use crate::{TERM, WAIT};
use wait_timeout::ChildExt;

const AVERAGE_SECONDS_IN_YEAR: u64 = 31_556_952;

/// Tracks numeric values from a line of text over time, computing deltas and statistics
#[derive(Debug, Clone)]
struct LineNumbers {
    /// Current numeric values extracted from the line
    values: Vec<i64>,
    /// Change from previous values (current - previous)
    delta: Vec<i64>,
}

impl LineNumbers {
    /// Creates a new LineNumbers instance with initial values
    fn new(numbers: Vec<i64>) -> Self {
        Self {
            values: numbers.clone(),
            delta: numbers,
        }
    }
}

type LineMap = std::collections::HashMap<(usize, u64), LineNumbers>;

/// Main state container for the dwatch application
pub struct Dwatch {
    /// Parser for extracting numeric ranges from text
    range_parser: RangeParser,
    /// Maps line identifiers to their numeric statistics
    line_map: RefCell<LineMap>,
    /// Interval between consecutive runs
    interval: Duration,
}

impl Dwatch {
    pub fn new(interval: Duration) -> Self {
        Self {
            range_parser: RangeParser::new(|c| {
                c.is_ascii_whitespace() || ".,:;()[]{}<>'`\"|=".contains(c)
            }),
            line_map: RefCell::new(LineMap::new()),
            interval,
        }
    }

    pub fn run(self, opt: Options) -> Result<()> {
        let opt = Arc::new(opt);
        let mutex = parking_lot::Mutex::new(());

        let (mut next, end) = {
            let now = Instant::now();
            (
                now + self.interval,
                now + Duration::from_secs(opt.seconds.unwrap_or(AVERAGE_SECONDS_IN_YEAR * 100)),
            )
        };

        // Pre-allocate thread handles vector
        let mut thread_handles: Vec<JoinHandle<_>> = Vec::with_capacity(opt.commands.len());

        while Instant::now() < end {
            let styles = Styles::new();

            print!(
                "{}{}",
                ansi_escapes::ClearScreen,
                ansi_escapes::CursorTo::TopLeft
            );

            if !opt.no_banner {
                println!(
                    "Every {} ms, style '{}': {}{} {}\n",
                    self.interval.as_millis(),
                    WRITERS[styles.focus_or_global() % WRITERS.len()].style,
                    opt.commands.join(" | "),
                    ansi_escapes::EraseEndLine,
                    styles.focus()
                );
            }

            let (mut line_no, mut num_no): (usize, usize) = (0, 0);

            for cmd in &opt.commands {
                let opt = Arc::clone(&opt);
                let cmd = cmd.clone();
                thread_handles.push(std::thread::spawn(move || {
                    run_command(&cmd, opt, self.interval).unwrap_or_else(|e| format!("{e}"))
                }));
            }

            for th in thread_handles.drain(..) {
                let output = th
                    .join()
                    .map_err(|e| -> anyhow::Error { anyhow!("Thread Join error: {:?}", e) })?;

                // transform and print the output, line by line
                for line in output.lines() {
                    num_no +=
                        self.writeln_line(&mut std::io::stdout(), (line, line_no, num_no), styles)?;
                    line_no += 1;
                }
            }

            write!(&mut std::io::stdout(), "{}", ansi_escapes::EraseDown)?;
            std::io::stdout().flush()?;

            TOTAL_FOCUSABLE_ITEMS.store(num_no, Ordering::Relaxed);

            if TERM.load(Ordering::Relaxed) {
                eprintln!("SIGTERM");
                break;
            }

            let mut guard = mutex.lock();
            let timeo_res = WAIT.wait_until(&mut guard, next);
            if timeo_res.timed_out() {
                next += self.interval;
            }
        }

        Ok(())
    }

    fn writeln_line(
        &self,
        out: &mut dyn Write,
        line: (&str, usize, usize),
        styles: Styles,
    ) -> Result<usize> {
        let ranges = self.range_parser.get_numeric_ranges(line.0);
        let strings = parse_strings(line.0, &ranges);
        let numbers = parse_numbers(line.0, &ranges)?;
        let key = (line.1, chunks_fingerprint(&strings));

        let mut line_map = self.line_map.borrow_mut();

        let line_stat = line_map
            .entry(key)
            .or_insert(LineNumbers::new(numbers.clone()));

        let total_numbers_in_line = numbers.len();

        let line_stat = {
            if total_numbers_in_line == line_stat.values.len() {
                let mut deltas = Vec::with_capacity(numbers.len());

                for (a, b) in numbers.iter().zip(line_stat.values.iter()) {
                    deltas.push(a - b);
                }
                line_stat.values = numbers.clone();
                line_stat.delta = deltas;
                line_stat
            } else {
                line_stat.values = numbers.clone();
                line_stat.delta = vec![0; numbers.len()];
                line_stat
            }
        };

        self.writeln_data(out, &strings, line_stat, &ranges, styles, line.2)?;
        Ok(total_numbers_in_line)
    }

    fn writeln_data(
        &self,
        out: &mut dyn Write,
        strings: &[&str],
        line_stat: &LineNumbers,
        ranges: &[Range<usize>],
        styles: Styles,
        initial_idx: usize,
    ) -> Result<()> {
        let first_is_number = !ranges.is_empty() && ranges[0].start == 0;

        for (idx, chunk) in izip!(
            line_stat.values.iter().copied(),
            line_stat.delta.iter().copied(),
        )
        .zip_longest(strings.iter())
        .enumerate()
        {
            let absolute_idx = initial_idx + idx;
            match chunk {
                Both(number, string) => {
                    if first_is_number {
                        self.write_number(out, &number, styles, absolute_idx)?;
                        write!(out, "{string}")?;
                    } else {
                        write!(out, "{string}")?;
                        self.write_number(out, &number, styles, absolute_idx)?;
                    }
                }
                Left(number) => {
                    self.write_number(out, &number, styles, absolute_idx)?;
                }
                Right(string) => {
                    write!(out, "{string}")?;
                }
            }
        }

        writeln!(out, "{}", ansi_escapes::EraseEndLine)?;
        Ok(())
    }

    #[inline]
    fn write_number(
        &self,
        out: &mut dyn Write,
        numbers: &(i64, i64),
        styles: Styles,
        idx: usize,
    ) -> Result<()> {
        (WRITERS[styles.current(idx) % WRITERS.len()].write)(
            out,
            numbers,
            self.interval,
            styles.is_focus(idx),
        )
    }
}

fn run_command(cmd: &str, _opt: Arc<Options>, timeout: Duration) -> Result<String> {
    // Spawn the child process, but keep it mutable to kill it later if needed
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn command '{}': {}", cmd, e))?;

    // Wait for the process with a timeout
    match child.wait_timeout(timeout)? {
        // The process finished within the time limit
        Some(status) => {
            // Since it finished, we can now safely collect its full output
            let output = child.wait_with_output()?;

            if !status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.is_empty() {
                    return Err(anyhow!(
                        "Command '{}' failed with stderr: {}",
                        cmd,
                        stderr.trim()
                    ));
                }
                return Err(anyhow!(
                    "Command '{}' failed with exit code: {:?}",
                    cmd,
                    output.status.code()
                ));
            }

            // Avoid unnecessary allocation if output is already valid UTF-8
            match String::from_utf8(output.stdout) {
                Ok(s) => Ok(s),
                Err(e) => Ok(String::from_utf8_lossy(e.as_bytes()).into_owned()),
            }
        }
        // The timeout was reached, the process is still running
        None => {
            // Kill the process to prevent it from running forever
            child.kill()?;
            // Wait for the now-killed process to be cleaned up by the OS
            child.wait()?;

            Err(anyhow!(
                "Command '{}' timed out after {} seconds and was killed",
                cmd,
                timeout.as_secs()
            ))
        }
    }
}

#[inline]
pub fn parse_numbers(line: &str, ranges: &[Range<usize>]) -> Result<Vec<i64>> {
    ranges
        .iter()
        .map(|r| {
            line.get(r.clone())
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or_else(|| anyhow!("failed to parse number in range {r:?}"))
        })
        .collect()
}

#[inline]
pub fn parse_strings<'a>(line: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
    complement_ranges(ranges, line.len())
        .iter()
        .map(|r| &line[r.clone()])
        .collect()
}

pub fn complement_ranges(xs: &[Range<usize>], size: usize) -> Vec<Range<usize>> {
    let mut compvec = Vec::with_capacity(xs.len() + 1);
    let mut first = 0;

    for x in xs {
        compvec.push(Range {
            start: first,
            end: x.start,
        });
        first = x.end;
    }

    compvec.push(Range {
        start: first,
        end: size,
    });

    compvec.retain(|r| r.start != r.end);
    compvec
}

#[inline]
fn chunks_fingerprint(chunks: &[&str]) -> u64 {
    let mut h = DefaultHasher::new();
    chunks.iter().for_each(|c| h.write(c.as_bytes()));
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_immutable_strings() {
        let rp = RangeParser::new(|c| c.is_ascii_whitespace());
        let ranges = rp.get_numeric_ranges("1234 hello 5678 world");
        let strings = parse_strings("1234 hello 5678 world", &ranges);
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0], " hello ");
        assert_eq!(strings[1], " world");
    }

    #[test]
    fn test_mutable_numbers() -> Result<()> {
        let rp = RangeParser::new(|c| c.is_ascii_whitespace());
        let ranges = rp.get_numeric_ranges("1234 hello 5678 world");
        let numbers = parse_numbers("1234 hello 5678 world", &ranges)?;
        assert_eq!(numbers.len(), 2);
        assert_eq!(numbers[0], 1234);
        assert_eq!(numbers[1], 5678);
        Ok(())
    }

    #[test]
    fn test_complement_ranges() {
        let ranges = vec![Range { start: 0, end: 4 }, Range { start: 10, end: 14 }];
        let complement = complement_ranges(&ranges, 20);

        assert_eq!(complement.len(), 2);
        assert_eq!(complement[0], Range { start: 4, end: 10 });
        assert_eq!(complement[1], Range { start: 14, end: 20 });

        // Edge case: no ranges
        let complement = complement_ranges(&[], 10);
        assert_eq!(complement.len(), 1);
        assert_eq!(complement[0], Range { start: 0, end: 10 });

        // Edge case: ranges cover entire string
        let ranges = vec![Range { start: 0, end: 10 }];
        let complement = complement_ranges(&ranges, 10);
        assert_eq!(complement.len(), 0);
    }

    #[test]
    fn test_chunks_fingerprint() {
        let chunks1 = vec!["hello", " ", "world"];
        let chunks2 = vec!["hello", " ", "world"];
        let chunks3 = vec!["hello", "", "world"];
        let chunks4 = vec!["goodbye", " ", "world"];

        let fp1 = chunks_fingerprint(&chunks1);
        let fp2 = chunks_fingerprint(&chunks2);
        let fp3 = chunks_fingerprint(&chunks3);
        let fp4 = chunks_fingerprint(&chunks4);

        // Same chunks should produce same fingerprint
        assert_eq!(fp1, fp2);

        // Different chunks should produce different fingerprints
        assert_ne!(fp1, fp3);
        assert_ne!(fp1, fp4);
        assert_ne!(fp3, fp4);
    }
}
