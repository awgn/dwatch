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

#[derive(Debug, Clone)]
struct LineNumbers {
    values: Vec<i64>,
    delta: Vec<i64>,
    min: Vec<i64>,
    max: Vec<i64>,
}

impl LineNumbers {
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

fn format_number<T: Into<f64>>(v: T, bit: bool) -> String {
    let value = v.into();

    if bit {
        if value > 1_000_000_000.0 {
            format!("{:.2}_Gbps", value / 1_000_000_000.0)
        } else if value > 1_000_000.0 {
            format!("{:.2}_Mbps", value / 1_000_000.0)
        } else if value > 1_000.0 {
            format!("{:.2}_Kbps", value / 1_000.0)
        } else {
            format!("{value:.2}_bps")
        }
    } else if value > 1_000_000_000.0 {
        format!("{:.2}G", value / 1_000_000_000.0)
    } else if value > 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value > 1_000.0 {
        format!("{:.2}K", value / 1_000.0)
    } else {
        format!("{value:.2}")
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

pub struct DwatchState {
    range_parser: RangeParser,
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

    while Instant::now() < end {
        if TERM.load(Ordering::Relaxed) {
            eprintln!("SIGTERM");
            break;
        }

        let mut thread_handles: Vec<JoinHandle<_>> = Vec::with_capacity(opt.commands.len());

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

        for th in thread_handles {
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
        .expect("failed to execute process");

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
}
