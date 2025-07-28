mod dwatch;
mod options;
mod ranges;
mod styles;

use anyhow::Result;
use clap::Parser;
use dashmap::DashMap;
use options::Options;
use parking_lot::Mutex;
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

static TERM: AtomicBool = AtomicBool::new(false);
static STYLE: AtomicUsize = AtomicUsize::new(0);
static STYLE_MAP: LazyLock<DashMap<usize, AtomicUsize>> = LazyLock::new(DashMap::new);
static WAIT: LazyLock<parking_lot::Condvar> = LazyLock::new(parking_lot::Condvar::new);
static FOCUS: Mutex<Option<usize>> = Mutex::new(None);
static FOCUS_RUN: AtomicUsize = AtomicUsize::new(0);
static FOCUS_TOTAL: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<()> {
    let mut opts = Options::parse();
    if opts.commands.is_empty() {
        return Ok(());
    }

    STYLE.store(
        opts.style
            .as_ref()
            .and_then(|name| styles::WriterBox::index(name))
            .unwrap_or(0),
        Ordering::Relaxed,
    );

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
                    break;
                }
                SIGTSTP => {
                    if let Some(mut focus) = FOCUS.try_lock() {
                        match focus.as_mut() {
                            Some(f) => {
                                if (*f + 1) >= FOCUS_TOTAL.load(Ordering::Relaxed) {
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
                    FOCUS_RUN.store(0, Ordering::Release);
                    WAIT.notify_one();
                }
                SIGQUIT => {
                    if let Some(focus) = FOCUS.try_lock() {
                        if let Some(idx) = *focus {
                            STYLE_MAP
                                .entry(idx)
                                .and_modify(|counter| {
                                    counter.fetch_add(1, Ordering::Relaxed);
                                })
                                .or_insert_with(|| AtomicUsize::new(1));
                            FOCUS_RUN.store(0, Ordering::Release);
                        } else {
                            STYLE.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    WAIT.notify_one();
                }
                _ => {}
            }
        }
    });

    if !opts.multiple_commands {
        opts.commands = vec![opts.commands.join(" ")];
    }

    let dwatch = Dwatch::new(Duration::from_secs(opts.interval.unwrap_or(1)));
    dwatch.run(opts)
}
