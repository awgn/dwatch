mod dwatch;
mod options;
mod ranges;
mod styles;

use anyhow::Result;
use clap::Parser;
use options::Options;
use signal_hook::consts::signal::*;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::iterator::exfiltrator::SignalOnly;
use signal_hook::iterator::SignalsInfo;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::LazyLock;
use std::time::Duration;

use crate::dwatch::Dwatch;
use crate::styles::{
    load_style_map, save_style_map, FOCUS_INDEX, FOCUS_LIFETIME, FOCUS_STYLE_MAP, GLOBAL_STYLE,
    TOTAL_FOCUSABLE_ITEMS,
};

static WAIT: LazyLock<parking_lot::Condvar> = LazyLock::new(parking_lot::Condvar::new);

static TERM: AtomicBool = AtomicBool::new(false);

fn normalize_cmds<I, S>(strings: I) -> impl Iterator<Item = String>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    strings.map(|s| s.as_ref().split_whitespace().collect::<Vec<_>>().join(" "))
}

fn main() -> Result<()> {
    let mut opts = Options::parse();
    if opts.commands.is_empty() {
        return Ok(());
    }

    GLOBAL_STYLE.store(
        opts.style
            .as_ref()
            .and_then(|name| styles::WriterBox::index(name))
            .unwrap_or(0),
        Ordering::Relaxed,
    );

    opts.commands = normalize_cmds(opts.commands.iter()).collect();
    if !opts.multiple_commands {
        opts.commands = vec![opts.commands.join(" ")];
    }

    load_style_map(&opts.commands)?;

    let cmds = opts.commands.clone();

    std::thread::spawn(move || {
        let mut sigs = vec![SIGTSTP, SIGWINCH];

        sigs.extend(TERM_SIGNALS);
        let mut signals =
            SignalsInfo::<SignalOnly>::new(&sigs).expect("failed to build SignalsInfo");

        for info in &mut signals {
            match info {
                SIGTERM | SIGINT => {
                    TERM.store(true, Ordering::Relaxed);
                    WAIT.notify_one();
                    if let Err(e) = save_style_map(&cmds) {
                        eprintln!("Failed to save style map: {}", e);
                    };
                    break;
                }
                SIGTSTP => {
                    println!("SIGTSTP received");
                    if let Some(mut focus) = FOCUS_INDEX.try_lock() {
                        match focus.as_mut() {
                            Some(f) => {
                                if (*f + 1) >= TOTAL_FOCUSABLE_ITEMS.load(Ordering::Relaxed) {
                                    *f = 0;
                                } else {
                                    *f += 1;
                                }
                            }
                            None => {
                                *focus = Some(0);
                            }
                        }
                    }
                    FOCUS_LIFETIME.store(0, Ordering::Release);
                    WAIT.notify_one();
                }
                SIGQUIT => {
                    if let Some(focus) = FOCUS_INDEX.try_lock() {
                        if let Some(idx) = *focus {
                            FOCUS_STYLE_MAP
                                .entry(idx)
                                .and_modify(|counter| {
                                    counter.fetch_add(1, Ordering::Relaxed);
                                })
                                .or_insert_with(|| AtomicUsize::new(1));
                            FOCUS_LIFETIME.store(0, Ordering::Release);
                        } else {
                            GLOBAL_STYLE.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    WAIT.notify_one();
                }
                _ => {}
            }
        }
    });

    let dwatch = Dwatch::new(Duration::from_secs(opts.interval.unwrap_or(1)));
    dwatch.run(opts)
}
