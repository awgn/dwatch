use owo_colors::OwoColorize;
use anyhow::Result;
use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Display,
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        LazyLock,
    },
    time::Duration,
};

pub static FOCUS_STYLE_MAP: LazyLock<DashMap<usize, AtomicUsize>> = LazyLock::new(DashMap::new);
pub static FOCUS_INDEX: Mutex<Option<usize>> = Mutex::new(None);
pub static GLOBAL_STYLE: AtomicUsize = AtomicUsize::new(0);
pub static FOCUS_LIFETIME: AtomicUsize = AtomicUsize::new(0);
pub static TOTAL_FOCUSABLE_ITEMS: AtomicUsize = AtomicUsize::new(0);

const FOCUS_LIFETIME_LIMIT: usize = 5;

pub fn load_style_map(cmd: &[String]) -> Result<()> {
    let key = cmd.join(" ").trim().to_owned();
    let config_path = get_config_path()?;

    if !config_path.exists() {
        return Ok(()); // No config file exists yet
    }

    let content = fs::read_to_string(&config_path)?;
    if content.trim().is_empty() {
        return Ok(()); // Empty file
    }

    // Parse NDJSON format
    let mut command_styles: HashMap<String, HashMap<usize, usize>> = HashMap::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: CommandStyleEntry = serde_json::from_str(line)?;
        command_styles.insert(entry.command, entry.styles);
    }

    // Load styles for the specific command
    if let Some(styles) = command_styles.get(&key) {
        FOCUS_STYLE_MAP.clear();
        for (key, value) in styles {
            FOCUS_STYLE_MAP.insert(*key, AtomicUsize::new(*value));
        }
    }

    Ok(())
}

pub fn save_style_map(cmd: &[String]) -> Result<()> {
    let key = cmd.join(" ").trim().to_owned();
    let config_path = get_config_path()?;

    // Ensure config directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read existing entries
    let mut command_styles: HashMap<String, HashMap<usize, usize>> = HashMap::new();
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: CommandStyleEntry = serde_json::from_str(line)?;
            command_styles.insert(entry.command, entry.styles);
        }
    }

    // Update with current STYLE_MAP for this command
    let current_styles: HashMap<usize, usize> = FOCUS_STYLE_MAP
        .iter()
        .map(|entry| {
            (
                *entry.key(),
                entry.value().load(std::sync::atomic::Ordering::Relaxed),
            )
        })
        .collect();

    command_styles.insert(key, current_styles);

    // Write back as NDJSON
    let mut file = fs::File::create(&config_path)?;
    for (command, styles) in command_styles {
        let entry = CommandStyleEntry { command, styles };
        writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
struct CommandStyleEntry {
    command: String,
    styles: HashMap<usize, usize>,
}

fn get_config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("Could not determine home directory"))?;

    let mut path = PathBuf::from(home);
    path.push(".config");
    path.push("dwatch");
    path.push("styles.json");
    Ok(path)
}

#[derive(Debug, Copy, Clone)]
pub struct Styles {
    focus: Focus,
}

impl Styles {
    pub fn new() -> Self {
        Styles {
            focus: Focus::new(),
        }
    }

    pub fn current(&self, index: usize) -> usize {
        FOCUS_STYLE_MAP
            .get(&index)
            .map(|atomic| atomic.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or_else(|| GLOBAL_STYLE.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn focus_or_global(&self) -> usize {
        let global_style = || GLOBAL_STYLE.load(std::sync::atomic::Ordering::Relaxed);
        match self.focus.index() {
            Some(focus_index) => FOCUS_STYLE_MAP
                .get(&focus_index)
                .map(|atomic| atomic.load(std::sync::atomic::Ordering::Relaxed))
                .unwrap_or_else(global_style),
            None => global_style(),
        }
    }

    #[inline]
    pub fn focus(&self) -> Focus {
        self.focus
    }

    #[inline]
    pub fn is_focus(&self, index: usize) -> bool {
        self.focus.index().map(|idx| idx == index).unwrap_or(false)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Focus(Option<usize>);

impl Focus {
    pub fn new() -> Self {
        let mut focus = FOCUS_INDEX.lock();
        let value = *focus;
        if FOCUS_LIFETIME.fetch_add(1, Ordering::Acquire) > FOCUS_LIFETIME_LIMIT {
            *focus = None;
            Focus(None)
        } else {
            Focus(value)
        }
    }

    pub fn index(&self) -> Option<usize> {
        self.0
    }
}

impl Display for Focus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = self.0 {
            write!(f, "(focus:{value})")
        } else {
            Ok(())
        }
    }
}

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
                write!(out, "{}", apply_style(format!("{}", num.0), "blue", focus))?;
                Ok(())
            },
        ),
        WriterBox::new(
            "number+(events per interval)",
            |out: &mut dyn Write, num: &(i64, i64), _: Duration, focus: bool| -> Result<()> {
                write!(out, "{}", apply_style(format!("{}", num.0), "red", focus))?;
                if num.1 != 0 {
                    write!(out, "⟶{}/i", apply_style(format!("{}", num.1), "red", focus))?;
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
                write!(out, "{}", apply_style(format!("{}", num.0), "red", focus))?;
                if num.1 != 0 {
                    let rate = num.1 as f64 / interval.as_secs_f64();
                    write!(out, "⟶{}/s", apply_style(format!("{rate}"), "red", focus))?;
                }
                Ok(())
            },
        ),
        WriterBox::new(
            "events per interval",
            |out: &mut dyn Write, num: &(i64, i64), _: Duration, focus: bool| -> Result<()> {
                write!(out, "{}/i", apply_style(format!("{}", num.1), "red", focus))?;
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
                let rate = num.1 as f64 / interval.as_secs_f64();
                write!(out, "{}/s", apply_style(format!("{rate}"), "red", focus))?;
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
                write!(out, "{}", apply_style(format!("{}", num.0), "purple", focus))?;
                if num.1 != 0 {
                    let rate = num.1 as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "⟶{}/s",
                        apply_style(format_number(rate, false).to_string(), "purple", focus)
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
                write!(out, "{}", apply_style(format!("{}", num.0), "green", focus))?;
                if num.1 != 0 {
                    let bit_rate = (num.1 * 8) as f64 / interval.as_secs_f64();
                    write!(
                        out,
                        "⟶{}/s",
                        apply_style(format_number(bit_rate, true).to_string(), "green", focus)
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
            v => format!("{v:.2}bps"),
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
fn apply_style(text: String, color: &str, focus: bool) -> String {
    let colored = match color {
        "blue" => text.blue().to_string(),
        "red" => text.red().to_string(),
        "purple" => text.magenta().to_string(),
        "green" => text.green().to_string(),
        _ => text.blue().to_string(),
    };
    
    let bold_text = colored.bold().to_string();
    
    if focus {
        bold_text.reversed().to_string()
    } else {
        bold_text
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
        assert_eq!(format_number(1500.0, true), "1.50Kbps");
        assert_eq!(format_number(1_500_000.0, true), "1.50Mbps");
        assert_eq!(format_number(1_500_000_000.0, true), "1.50Gbps");
    }
}
