use std::{
    collections::hash_map::DefaultHasher,
    error::Error,
    hash::Hasher,
    io::Write,
    iter::zip,
    ops::Range,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread::{sleep, JoinHandle},
    time::{Duration, Instant},
};

use ansi_term::Colour;

use itertools::EitherOrBoth::{Both, Left, Right};
use itertools::Itertools;

use crate::options::Options;
use crate::ranges::RangeParser;

type LineMap = std::collections::HashMap<(u64, u64), Vec<i64>>;

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

type WriterBox = Box<
    dyn Fn(&mut dyn Write, (&i64, &i64), Duration) -> std::io::Result<()> + Send + Sync + 'static,
>;

lazy_static! {
    static ref WRITERS: Vec<WriterBox> = vec![
        Box::new(
            |out: &mut dyn Write, num: (&i64, &i64), _: Duration| -> std::io::Result<()> {
                write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))
            }
        ),
        Box::new(
            |out: &mut dyn Write, num: (&i64, &i64), _: Duration| -> std::io::Result<()> {
                write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))?;
                if num.1 != &0 {
                    write!(out, "_{}", Colour::Red.paint(format!("{}", num.1)))?;
                }
                Ok(())
            }
        ),
        Box::new(
            |out: &mut dyn Write, num: (&i64, &i64), _: Duration| -> std::io::Result<()> {
                write!(out, "{}", Colour::Red.bold().paint(format!("{}", num.1)))
            }
        ),
        Box::new(
            |out: &mut dyn Write, num: (&i64, &i64), interval: Duration| -> std::io::Result<()> {
                if *num.1 != 0 {
                    let delta = *num.1 as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Red
                            .bold()
                            .paint(format_number(delta, false).to_string())
                    )
                } else {
                    write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))
                }
            }
        ),
        Box::new(
            |out: &mut dyn Write, num: (&i64, &i64), interval: Duration| -> std::io::Result<()> {
                if *num.1 != 0 {
                    let delta = (*num.1 * 8) as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "{}",
                        Colour::Green
                            .bold()
                            .paint(format_number(delta, true).to_string())
                    )
                } else {
                    write!(out, "{}", Colour::Blue.paint(format!("{}", num.0)))
                }
            }
        ),
    ];
}

pub fn run(
    opt: Options,
    term: Arc<AtomicBool>,
    delta: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let interval = Duration::from_secs(opt.interval.unwrap_or(1));

    print!("{}", ansi_escapes::ClearScreen);

    let now = Instant::now();
    let end = now + Duration::from_secs(opt.seconds.unwrap_or(9999999999));
    let mut next = now + interval;
    let mut line_map = LineMap::new();
    let mut last_run = Instant::now();

    let opt = Arc::new(opt);

    while Instant::now() < end {
        if term.load(Ordering::Relaxed) {
            println!("SIGTERM");
            break;
        }

        let mut outputs: Vec<JoinHandle<_>> = Vec::with_capacity(opt.commands.len());

        for cmd in &opt.commands {
            let cmd = cmd.clone();
            let opt = Arc::clone(&opt);
            let output = std::thread::spawn(move || run_command(&cmd, opt).unwrap());
            outputs.push(output);
        }

        print!("{}", ansi_escapes::CursorTo::TopLeft);

        if !opt.no_banner {
            println!(
                "Every {} ms, delta:{}: {}\n",
                interval.as_millis(),
                delta.load(Ordering::Relaxed) % WRITERS.len(),
                opt.commands.join(" | ")
            );
        }

        let mut lineno = 0u64;
        let writer_idx = delta.load(Ordering::Relaxed) % WRITERS.len();

        for output in outputs {
            let output = output.join().map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("Thread Join error: {:?}", e).into()
            })?;

            let interval = Instant::now() - last_run;
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

        last_run = Instant::now();

        let nap = next - last_run;
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
) -> std::io::Result<()> {
    let rp = RangeParser::new(|c| c.is_ascii_whitespace() || ".,:;()[]{}<>'`\"|".contains(c));

    let ranges = rp.get_numeric_ranges(line);
    let strings = parse_strings(line, &ranges);
    let absolutes = parse_numbers(line, &ranges);
    let last_key = (lineno, chunks_fingerprint(&strings));

    let last_absolutes = lmap.entry(last_key).or_insert(absolutes.clone());

    let deltas = {
        if absolutes.len() == last_absolutes.len() {
            let mut deltas = Vec::with_capacity(absolutes.len());

            for (a, b) in absolutes.iter().zip(last_absolutes.iter()) {
                deltas.push(a - b);
            }
            *last_absolutes = absolutes.clone();
            deltas
        } else {
            *last_absolutes = absolutes.clone();
            vec![0; absolutes.len()]
        }
    };

    writeln_data(
        out, writer_idx, &strings, &absolutes, &deltas, &ranges, interval,
    )
}

fn writeln_data(
    out: &mut dyn Write,
    writer_idx: usize,
    strings: &[&str],
    absolutes: &[i64],
    deltas: &[i64],
    ranges: &[Range<usize>],
    interval: Duration,
) -> std::io::Result<()> {
    let s = strings.iter();
    let n = zip(absolutes, deltas);

    let first_is_number = !ranges.is_empty() && ranges[0].start == 0;

    for chunk in n.zip_longest(s) {
        match chunk {
            Both(number, string) => {
                if first_is_number {
                    write_number(out, writer_idx, number, interval)?;
                    write!(out, "{}", string)?;
                } else {
                    write!(out, "{}", string)?;
                    write_number(out, writer_idx, number, interval)?;
                }
            }
            Left(number) => {
                write_number(out, writer_idx, number, interval)?;
            }
            Right(string) => {
                write!(out, "{}", string)?;
            }
        }
    }

    writeln!(out, "{}", ansi_escapes::EraseEndLine)
}

fn write_number(
    out: &mut dyn Write,
    writer_idx: usize,
    num: (&i64, &i64),
    interval: Duration,
) -> std::io::Result<()> {
    WRITERS[writer_idx](out, num, interval)
}

fn run_command(
    cmd: &str,
    _opt: Arc<Options>,
) -> Result<String, Box<(dyn Error + Send + Sync + 'static)>> {
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
