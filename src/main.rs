mod dwatch;
mod options;
mod ranges;

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
use std::sync::Condvar;

#[macro_use]
extern crate lazy_static;

static TERM : AtomicBool = AtomicBool::new(false);
static STYLE : AtomicUsize = AtomicUsize::new(0);

lazy_static! {
    static ref WAIT: Condvar = Condvar::new();
}

fn main() -> Result<()> {
    let mut opts = Options::parse();
    if opts.commands.is_empty() {
        return Ok(());
    }

    STYLE.store(
        opts.style
            .as_ref()
            .and_then(|name| dwatch::WriterBox::index(name))
            .unwrap_or(0), Ordering::Relaxed
    );

    std::thread::spawn(move || {
        let mut sigs = vec![SIGTSTP, SIGWINCH];

        sigs.extend(TERM_SIGNALS);
        let mut signals = SignalsInfo::<SignalOnly>::new(&sigs).unwrap();

        for info in &mut signals {
            match info {
                SIGTERM | SIGINT | SIGTSTP => {
                    TERM.store(true, Ordering::Relaxed);
                    break;
                }
                SIGQUIT => {
                    STYLE.fetch_add(1, Ordering::Relaxed);
                    WAIT.notify_one();
                }
                _ => {}
            }
        }
    });

    if !opts.multiple_commands {
        opts.commands = vec![opts.commands.join(" ")];
    }

    dwatch::run(opts)
}
