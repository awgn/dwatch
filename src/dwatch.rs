use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    io::Write,
    ops::Range,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread::{sleep, JoinHandle},
    time::{Duration, Instant},
};

use ansi_term::Colour;

use anyhow::{anyhow, Result};
use itertools::{EitherOrBoth::{Both, Left, Right}, izip, multizip};
use itertools::Itertools;

use crate::options::Options;
use crate::ranges::RangeParser;

#[derive(Debug, Clone)]
struct LineNumbers {
    num: Vec<i64>,
    delta: Vec<i64>,
    min: Vec<i64>,
    max: Vec<i64>,
}

impl LineNumbers {
    fn new(numbers: Vec<i64>) -> Self {
        let len = numbers.len();
        Self {
            num: numbers.clone(),
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
            format!("{:.2}_bps", value)
        }
    } else if value > 1_000_000_000.0 {
        format!("{:.2}G", value / 1_000_000_000.0)
    } else if value > 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value > 1_000.0 {
        format!("{:.2}K", value / 1_000.0)
    } else {
        format!("{:.2}", value)
    }
}

pub struct WriterBox {
    write:
        Box<dyn Fn(&mut dyn Write, (&i64, &i64, &i64, &i64), Duration) -> Result<()> + Send + Sync + 'static>,
    pub style: String,
}

impl WriterBox {
    fn new<F>(style: String, fun: F) -> Self
    where
        F: Fn(&mut dyn Write, (&i64, &i64, &i64, &i64), Duration) -> Result<()> + Send + Sync + 'static,
    {
        Self {
            write: Box::new(fun),
            style,
        }
    }

    pub fn index(s: &str) -> Option<usize> {
        WRITERS.iter().position(|w| w.style == s)
    }
}

lazy_static! {
    static ref WRITERS: Vec<WriterBox> = vec![
        WriterBox::new(
            "default".into(),
            |out: &mut dyn Write, num: (&i64, &i64, &i64, &i64), _: Duration| -> Result<()> {
                write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))?;
                Ok(())
            }
        ),
        WriterBox::new(
            "abs-delta".into(),
            |out: &mut dyn Write, num: (&i64, &i64, &i64, &i64), _: Duration| -> Result<()> {
                write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))?;
                if num.1 != &0 {
                    write!(out, "_{}", Colour::Red.paint(format!("{}", num.1)))?;
                }
                Ok(())
            }
        ),
        WriterBox::new(
            "delta".into(),
            |out: &mut dyn Write, num: (&i64, &i64, &i64, &i64), _: Duration| -> Result<()> {
                write!(out, "{}", Colour::Red.bold().paint(format!("{}", num.1)))?;
                Ok(())
            }
        ),
        WriterBox::new(
            "fancy".into(),
            |out: &mut dyn Write, num: (&i64, &i64, &i64, &i64), interval: Duration| -> Result<()> {
                if *num.1 != 0 {
                    let delta = *num.1 as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Red
                            .bold()
                            .paint(format_number(delta, false).to_string())
                    )?;
                    Ok(())
                } else {
                    write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))?;
                    Ok(())
                }
            }
        ),
        WriterBox::new(
            "fancy-net".into(),
            |out: &mut dyn Write, num: (&i64, &i64, &i64, &i64), interval: Duration| -> Result<()> {
                if *num.1 != 0 {
                    let delta = (*num.1 * 8) as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Green
                            .bold()
                            .paint(format_number(delta, true).to_string())
                    )?;
                    Ok(())
                } else {
                    write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))?;
                    Ok(())
                }
            }
        ),
        WriterBox::new(
            "stats".into(),
            |out: &mut dyn Write, num: (&i64, &i64, &i64, &i64), _: Duration| -> Result<()> {
                write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))?;
                if num.1 != &0 {
                    write!(out, "_{}", Colour::Red.paint(format!("{}", num.1)))?;
                    write!(out, "_{}", Colour::Black.bold().paint(format!("{}/{}", num.2, num.3)))?;
                }
                Ok(())
            }
        ),
        WriterBox::new(
            "stats-net".into(),
            |out: &mut dyn Write, num: (&i64, &i64, &i64, &i64), interval: Duration| -> Result<()> {
                if *num.1 != 0 {
                    let delta = *num.1 as f64 * 8.0 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Green
                            .bold()
                            .paint(format_number(delta, true).to_string())
                    )?;
                    write!(out, "_{}", Colour::Black.bold().paint(format!("{}/{}",
                        format_number(*num.2 as f64 * 8.0 / interval.as_secs_f64(), true),
                        format_number(*num.3 as f64 * 8.0 / interval.as_secs_f64(), true))))?;
                    Ok(())
                } else {
                    write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))?;
                    Ok(())
                }
            }
        ),
    ];
}

pub fn run(opt: Options, term: Arc<AtomicBool>, style_index: Arc<AtomicUsize>) -> Result<()> {
    let interval = Duration::from_secs(opt.interval.unwrap_or(1));

    print!("{}", ansi_escapes::ClearScreen);

    let now = Instant::now();
    let end = now + Duration::from_secs(opt.seconds.unwrap_or(9999999999));
    let mut next = now + interval;
    let mut line_map = LineMap::new();

    let opt = Arc::new(opt);

    while Instant::now() < end {
        if term.load(Ordering::Relaxed) {
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

        if !opt.no_banner {
            println!(
                "Every {} ms, delta[{}]: {}{}\n",
                interval.as_millis(),
                WRITERS[style_index.load(Ordering::Relaxed) % WRITERS.len()].style,
                opt.commands.join(" | "),
                ansi_escapes::EraseEndLine
            );
        }

        let mut lineno = 0u64;
        let writer_idx = style_index.load(Ordering::Relaxed) % WRITERS.len();

        for th in thread_handles {
            let output = th
                .join()
                .map_err(|e| -> anyhow::Error { anyhow!("Thread Join error: {:?}", e) })?;

            // transform and print the output, line by line
            for line in output.lines() {
                writeln_line(
                    &mut std::io::stdout(),
                    writer_idx,
                    line,
                    lineno,
                    &mut line_map,
                    interval,
                )?;
                lineno += 1;
            }
        }

        write!(&mut std::io::stdout(), "{}", ansi_escapes::EraseDown)?;

        let nap = next - Instant::now();
        next += interval;
        sleep(nap);
    }

    Ok(())
}

fn writeln_line(
    out: &mut dyn Write,
    writer_idx: usize,
    line: &str,
    lineno: u64,
    lmap: &mut LineMap,
    interval: Duration,
) -> Result<()> {
    let rp = RangeParser::new(|c| c.is_ascii_whitespace() || ".,:;()[]{}<>'`\"|".contains(c));

    let ranges = rp.get_numeric_ranges(line);
    let strings = parse_strings(line, &ranges);
    let numbers = parse_numbers(line, &ranges);
    let key = (lineno, chunks_fingerprint(&strings));

    let line_stat = lmap.entry(key).or_insert(LineNumbers::new(numbers.clone()));

    let stat = {
        if numbers.len() == line_stat.num.len() {
            let mut deltas = Vec::with_capacity(numbers.len());

            for (a, b) in numbers.iter().zip(line_stat.num.iter()) {
                deltas.push(a - b);
            }
            line_stat.num = numbers.clone();
            line_stat.delta = deltas;

            for (min, max, value) in multizip((
                &mut line_stat.min,
                &mut line_stat.max,
                &line_stat.delta)
            ) {
                *min = std::cmp::min(*min, *value);
                *max = std::cmp::max(*max, *value);
            }

            line_stat.clone()
        } else {
            line_stat.num = numbers.clone();
            line_stat.delta = vec![0; numbers.len()];
            line_stat.min = vec![0; numbers.len()];
            line_stat.max = vec![0; numbers.len()];
            line_stat.clone()
        }
    };

    writeln_data(
        out, writer_idx, &strings, &stat, &ranges, interval,
    )
}

fn writeln_data(
    out: &mut dyn Write,
    writer_idx: usize,
    strings: &[&str],
    stat: &LineNumbers,
    ranges: &[Range<usize>],
    interval: Duration,
) -> Result<()> {
    let s = strings.iter();
    let first_is_number = !ranges.is_empty() && ranges[0].start == 0;

    for chunk in izip!(&stat.num, &stat.delta, &stat.min, &stat.max).zip_longest(s) {
        match chunk {
            Both(numbers, string) => {
                if first_is_number {
                    write_number(out, writer_idx, numbers, interval)?;
                    write!(out, "{}", string)?;
                } else {
                    write!(out, "{}", string)?;
                    write_number(out, writer_idx, numbers, interval)?;
                }
            }
            Left(numbers) => {
                write_number(out, writer_idx, numbers, interval)?;
            }
            Right(string) => {
                write!(out, "{}", string)?;
            }
        }
    }

    writeln!(out, "{}", ansi_escapes::EraseEndLine)?;
    Ok(())
}

fn write_number(
    out: &mut dyn Write,
    writer_idx: usize,
    numbers: (&i64, &i64, &i64, &i64),
    interval: Duration,
) -> Result<()> {
    (WRITERS[writer_idx].write)(out, numbers, interval)
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
pub fn parse_numbers(line: &str, ranges: &[Range<usize>]) -> Vec<i64> {
    ranges
        .iter()
        .map(|r| line[r.clone()].parse::<i64>().unwrap())
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
    fn test_mutable_numbers() {
        let rp = RangeParser::new(|c| c.is_ascii_whitespace());
        let ranges = rp.get_numeric_ranges("1234 hello 5678 world");
        let numbers = parse_numbers("1234 hello 5678 world", &ranges);
        assert_eq!(numbers.len(), 2);
        assert_eq!(numbers[0], 1234);
        assert_eq!(numbers[1], 5678);
    }
}
