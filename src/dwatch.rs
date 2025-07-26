use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    io::Write,
    ops::Range,
    sync::{atomic::Ordering, Arc, LazyLock},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use ansi_term::Colour;

use anyhow::{anyhow, Result};
use itertools::Itertools;
use itertools::{
    izip, multizip,
    EitherOrBoth::{Both, Left, Right},
};

use crate::options::Options;
use crate::ranges::RangeParser;
use crate::{STYLE, TERM, WAIT};

const AVERAGE_SECONDS_IN_YEAR: u64 = 31_556_952;

/// Tracks numeric values from a line of text over time, computing deltas and statistics
#[derive(Debug, Clone)]
struct LineNumbers {
    /// Current numeric values extracted from the line
    values: Vec<i64>,
    /// Change from previous values (current - previous)
    delta: Vec<i64>,
    /// Minimum delta observed for each position
    min: Vec<i64>,
    /// Maximum delta observed for each position
    max: Vec<i64>,
}

impl LineNumbers {
    /// Creates a new LineNumbers instance with initial values
    fn new(numbers: Vec<i64>) -> Self {
        let len = numbers.len();
        Self {
            values: numbers.clone(),
            delta: numbers,
            min: vec![0; len],
            max: vec![0; len],
        }
    }
}

type LineMap = std::collections::HashMap<(u64, u64), LineNumbers>;

/// Formats a numeric value with appropriate unit suffixes
///
/// # Arguments
/// * `v` - The value to format
/// * `bit` - If true, formats as bits per second (bps), otherwise as raw count
fn format_number<T: Into<f64>>(v: T, bit: bool) -> String {
    let value = v.into();

    const GIGA: f64 = 1_000_000_000.0;
    const MEGA: f64 = 1_000_000.0;
    const KILO: f64 = 1_000.0;

    if bit {
        match value {
            v if v > GIGA => format!("{:.2}_Gbps", v / GIGA),
            v if v > MEGA => format!("{:.2}_Mbps", v / MEGA),
            v if v > KILO => format!("{:.2}_Kbps", v / KILO),
            v => format!("{v:.2}_bps"),
        }
    } else {
        match value {
            v if v > GIGA => format!("{:.2}G", v / GIGA),
            v if v > MEGA => format!("{:.2}M", v / MEGA),
            v if v > KILO => format!("{:.2}K", v / KILO),
            v => format!("{v:.2}"),
        }
    }
}

type WriterFn =
    dyn Fn(&mut dyn Write, (i64, i64, i64, i64), Duration) -> Result<()> + Send + Sync + 'static;

pub struct WriterBox {
    write: Box<WriterFn>,
    pub style: String,
}

impl WriterBox {
    fn new<F>(style: &str, fun: F) -> Self
    where
        F: Fn(&mut dyn Write, (i64, i64, i64, i64), Duration) -> Result<()> + Send + Sync + 'static,
    {
        Self {
            write: Box::new(fun),
            style: style.to_owned(),
        }
    }

    pub fn index(s: &str) -> Option<usize> {
        WRITERS.iter().position(|w| w.style == s)
    }
}

static WRITERS: LazyLock<Vec<WriterBox>> = LazyLock::new(|| {
    vec![
        WriterBox::new(
            "default",
            |out: &mut dyn Write, num: (i64, i64, i64, i64), _: Duration| -> Result<()> {
                write!(out, "{}", Colour::Blue.bold().paint(format!("{}", num.0)))?;
                Ok(())
            },
        ),
        WriterBox::new(
            "number+delta",
            |out: &mut dyn Write, num: (i64, i64, i64, i64), _: Duration| -> Result<()> {
                write!(out, "{}", Colour::Red.bold().paint(format!("{}", num.0)))?;
                if num.1 != 0 {
                    write!(out, ":_{}", Colour::Red.paint(format!("{}", num.1)))?;
                }
                Ok(())
            },
        ),
        WriterBox::new(
            "delta",
            |out: &mut dyn Write, num: (i64, i64, i64, i64), _: Duration| -> Result<()> {
                write!(out, ":{}", Colour::Red.bold().paint(format!("{}", num.1)))?;
                Ok(())
            },
        ),
        WriterBox::new(
            "fancy",
            |out: &mut dyn Write, num: (i64, i64, i64, i64), interval: Duration| -> Result<()> {
                if num.1 != 0 {
                    let delta = num.1 as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Purple
                            .bold()
                            .paint(format_number(delta, false).to_string())
                    )?;
                    Ok(())
                } else {
                    write!(out, "{}", Colour::Purple.bold().paint(format!("{}", num.0)))?;
                    Ok(())
                }
            },
        ),
        WriterBox::new(
            "fancy-network",
            |out: &mut dyn Write, num: (i64, i64, i64, i64), interval: Duration| -> Result<()> {
                if num.1 != 0 {
                    let delta = (num.1 * 8) as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Green
                            .bold()
                            .paint(format_number(delta, true).to_string())
                    )?;
                    Ok(())
                } else {
                    write!(out, "{}", Colour::Green.bold().paint(format!("{}", num.0)))?;
                    Ok(())
                }
            },
        ),
        WriterBox::new(
            "stats",
            |out: &mut dyn Write, num: (i64, i64, i64, i64), _: Duration| -> Result<()> {
                write!(out, "{}", Colour::Cyan.bold().paint(format!("{}", num.0)))?;
                if num.1 != 0 {
                    write!(out, "_{}", Colour::Cyan.paint(format!("{}", num.1)))?;
                    write!(
                        out,
                        "_{}",
                        Colour::Cyan.bold().paint(format!("{}/{}", num.2, num.3))
                    )?;
                }
                Ok(())
            },
        ),
        WriterBox::new(
            "stats-network",
            |out: &mut dyn Write, num: (i64, i64, i64, i64), interval: Duration| -> Result<()> {
                if num.1 != 0 {
                    let delta = num.1 as f64 * 8.0 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Green
                            .bold()
                            .paint(format_number(delta, true).to_string())
                    )?;
                    write!(
                        out,
                        "_{}",
                        Colour::Green.bold().paint(format!(
                            "{}/{}",
                            format_number(num.2 as f64 * 8.0 / interval.as_secs_f64(), true),
                            format_number(num.3 as f64 * 8.0 / interval.as_secs_f64(), true)
                        ))
                    )?;
                    Ok(())
                } else {
                    write!(out, "{}", Colour::Green.bold().paint(format!("{}", num.0)))?;
                    Ok(())
                }
            },
        ),
    ]
});

/// Main state container for the dwatch application
pub struct DwatchState {
    /// Parser for extracting numeric ranges from text
    range_parser: RangeParser,
    /// Maps line identifiers to their numeric statistics
    line_map: LineMap,
}

impl DwatchState {
    pub fn new() -> Self {
        Self {
            range_parser: RangeParser::new(|c| {
                c.is_ascii_whitespace() || ".,:;()[]{}<>'`\"|=".contains(c)
            }),
            line_map: LineMap::new(),
        }
    }
}

pub fn run(opt: Options) -> Result<()> {
    let interval = Duration::from_secs(opt.interval.unwrap_or(1));
    let opt = Arc::new(opt);
    let mutex = parking_lot::Mutex::new(());
    let mut state = DwatchState::new();

    print!("{}", ansi_escapes::ClearScreen);

    let (mut next, end) = {
        let now = Instant::now();
        (
            now + interval,
            now + Duration::from_secs(opt.seconds.unwrap_or(AVERAGE_SECONDS_IN_YEAR * 100)),
        )
    };

    // Pre-allocate thread handles vector
    let mut thread_handles: Vec<JoinHandle<_>> = Vec::with_capacity(opt.commands.len());

    while Instant::now() < end {
        if TERM.load(Ordering::Relaxed) {
            eprintln!("SIGTERM");
            break;
        }

        for cmd in &opt.commands {
            let cmd = cmd.clone();
            let opt = Arc::clone(&opt);
            thread_handles.push(std::thread::spawn(move || run_command(&cmd, opt).unwrap()));
        }

        print!("{}", ansi_escapes::CursorTo::TopLeft);

        let widx = STYLE.load(Ordering::Relaxed) % WRITERS.len();

        if !opt.no_banner {
            println!(
                "Every {} ms, style {}: {}{}\n",
                interval.as_millis(),
                WRITERS[widx].style,
                opt.commands.join(" | "),
                ansi_escapes::EraseEndLine
            );
        }

        let mut lineno = 0u64;

        for th in thread_handles.drain(..) {
            let output = th
                .join()
                .map_err(|e| -> anyhow::Error { anyhow!("Thread Join error: {:?}", e) })?;

            // transform and print the output, line by line
            for line in output.lines() {
                writeln_line(
                    &mut std::io::stdout(),
                    widx,
                    (line, lineno),
                    &mut state,
                    interval,
                )?;
                lineno += 1;
            }
        }

        write!(&mut std::io::stdout(), "{}", ansi_escapes::EraseDown)?;
        std::io::stdout().flush()?;

        let mut guard = mutex.lock();
        let timeo_res = WAIT.wait_until(&mut guard, next);
        if timeo_res.timed_out() {
            next += interval;
        }
    }

    Ok(())
}

fn writeln_line(
    out: &mut dyn Write,
    widx: usize,
    line: (&str, u64),
    state: &mut DwatchState,
    interval: Duration,
) -> Result<()> {
    let ranges = state.range_parser.get_numeric_ranges(line.0);
    let strings = parse_strings(line.0, &ranges);
    let numbers = parse_numbers(line.0, &ranges)?;
    let key = (line.1, chunks_fingerprint(&strings));

    let line_stat = state
        .line_map
        .entry(key)
        .or_insert(LineNumbers::new(numbers.clone()));

    let line_stat = {
        if numbers.len() == line_stat.values.len() {
            let mut deltas = Vec::with_capacity(numbers.len());

            for (a, b) in numbers.iter().zip(line_stat.values.iter()) {
                deltas.push(a - b);
            }
            line_stat.values = numbers.clone();
            line_stat.delta = deltas;

            for (min, max, value) in
                multizip((&mut line_stat.min, &mut line_stat.max, &line_stat.delta))
            {
                *min = std::cmp::min(*min, *value);
                *max = std::cmp::max(*max, *value);
            }

            line_stat
        } else {
            line_stat.values = numbers.clone();
            line_stat.delta = vec![0; numbers.len()];
            line_stat.min = vec![0; numbers.len()];
            line_stat.max = vec![0; numbers.len()];
            line_stat
        }
    };

    writeln_data(out, widx, &strings, line_stat, &ranges, interval)
}

fn writeln_data(
    out: &mut dyn Write,
    widx: usize,
    strings: &[&str],
    stat: &LineNumbers,
    ranges: &[Range<usize>],
    interval: Duration,
) -> Result<()> {
    let first_is_number = !ranges.is_empty() && ranges[0].start == 0;

    for chunk in izip!(
        stat.values.iter().copied(),
        stat.delta.iter().copied(),
        stat.min.iter().copied(),
        stat.max.iter().copied(),
    )
    .zip_longest(strings.iter())
    {
        match chunk {
            Both(numbers, string) => {
                if first_is_number {
                    write_number(out, widx, numbers, interval)?;
                    write!(out, "{string}")?;
                } else {
                    write!(out, "{string}")?;
                    write_number(out, widx, numbers, interval)?;
                }
            }
            Left(numbers) => {
                write_number(out, widx, numbers, interval)?;
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
    out: &mut dyn Write,
    widx: usize,
    numbers: (i64, i64, i64, i64),
    interval: Duration,
) -> Result<()> {
    (WRITERS[widx].write)(out, numbers, interval)
}

fn run_command(cmd: &str, _opt: Arc<Options>) -> Result<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map_err(|e| anyhow!("Failed to execute command '{}': {}", cmd, e))?;

    if !output.status.success() {
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
    fn test_format_number() {
        // Test without bit formatting
        assert_eq!(format_number(500.0, false), "500.00");
        assert_eq!(format_number(1500.0, false), "1.50K");
        assert_eq!(format_number(1_500_000.0, false), "1.50M");
        assert_eq!(format_number(1_500_000_000.0, false), "1.50G");

        // Test with bit formatting
        assert_eq!(format_number(500.0, true), "500.00_bps");
        assert_eq!(format_number(1500.0, true), "1.50_Kbps");
        assert_eq!(format_number(1_500_000.0, true), "1.50_Mbps");
        assert_eq!(format_number(1_500_000_000.0, true), "1.50_Gbps");
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
