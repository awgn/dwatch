use ansi_term::{Colour, Style};
use anyhow::Result;
use std::{io::Write, sync::LazyLock, time::Duration};

type WriterFn =
    dyn Fn(&mut dyn Write, &(i64, i64), Duration, bool) -> Result<()> + Send + Sync + 'static;

pub struct WriterBox {
    pub write: Box<WriterFn>,
    pub style: String,
}

impl WriterBox {
    pub fn new<F>(style: &str, fun: F) -> Self
    where
        F: Fn(&mut dyn Write, &(i64, i64), Duration, bool) -> Result<()> + Send + Sync + 'static,
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

pub static WRITERS: LazyLock<Vec<WriterBox>> = LazyLock::new(|| {
    vec![
        WriterBox::new(
            "default",
            |out: &mut dyn Write, num: &(i64, i64), _: Duration, focus: bool| -> Result<()> {
                let style = build_style(Colour::Blue, focus);
                write!(out, "{}", style.paint(format!("{}", num.0)))?;
                Ok(())
            },
        ),
        WriterBox::new(
            "number+(events per interval)",
            |out: &mut dyn Write, num: &(i64, i64), _: Duration, focus: bool| -> Result<()> {
                let style = build_style(Colour::Red, focus);
                write!(out, "{}", style.paint(format!("{}", num.0)))?;
                if num.1 != 0 {
                    write!(out, "⟶{}/i", style.paint(format!("{}", num.1)))?;
                }
                Ok(())
            },
        ),
        WriterBox::new(
            "number+(events per second)",
            |out: &mut dyn Write,
             num: &(i64, i64),
             interval: Duration,
             focus: bool|
             -> Result<()> {
                let style = build_style(Colour::Red, focus);
                write!(out, "{}", style.paint(format!("{}", num.0)))?;
                if num.1 != 0 {
                    let rate = num.1 as f64 / interval.as_secs_f64();
                    write!(out, "⟶{}/s", style.paint(format!("{rate}")))?;
                }
                Ok(())
            },
        ),
        WriterBox::new(
            "events per interval",
            |out: &mut dyn Write, num: &(i64, i64), _: Duration, focus: bool| -> Result<()> {
                let style = build_style(Colour::Red, focus);
                write!(out, "{}/i", style.paint(format!("{}", num.1)))?;
                Ok(())
            },
        ),
        WriterBox::new(
            "events per second",
            |out: &mut dyn Write,
             num: &(i64, i64),
             interval: Duration,
             focus: bool|
             -> Result<()> {
                let style = build_style(Colour::Red, focus);
                let rate = num.1 as f64 / interval.as_secs_f64();
                write!(out, "{}/s", style.paint(format!("{rate}")))?;
                Ok(())
            },
        ),
        WriterBox::new(
            "engineering",
            |out: &mut dyn Write,
             num: &(i64, i64),
             interval: Duration,
             focus: bool|
             -> Result<()> {
                let style = build_style(Colour::Purple, focus);
                write!(out, "{}", style.paint(format!("{}", num.0)))?;
                if num.1 != 0 {
                    let rate = num.1 as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "⟶{}/s",
                        style.paint(format_number(rate, false).to_string())
                    )?;
                }
                Ok(())
            },
        ),
        WriterBox::new(
            "networking",
            |out: &mut dyn Write,
             num: &(i64, i64),
             interval: Duration,
             focus: bool|
             -> Result<()> {
                let style = build_style(Colour::Green, focus);
                write!(out, "{}", style.paint(format!("{}", num.0)))?;
                if num.1 != 0 {
                    let bit_rate = (num.1 * 8) as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "⟶{}/s",
                        style.paint(format_number(bit_rate, true).to_string())
                    )?;
                }
                Ok(())
            },
        ),
    ]
});

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
            v if v > GIGA => format!("{:.2}Gbps", v / GIGA),
            v if v > MEGA => format!("{:.2}Mbps", v / MEGA),
            v if v > KILO => format!("{:.2}Kbps", v / KILO),
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

#[inline]
fn build_style(c: Colour, focus: bool) -> Style {
    if focus {
        c.bold().reverse()
    } else {
        c.bold()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
