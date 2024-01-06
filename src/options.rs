use clap::Parser;

#[derive(Parser, Default, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Options {
    #[clap(short, long, help = "Exit after the specified number of seconds")]
    pub seconds: Option<u64>,

    #[clap(short, long, help = "Suppress the banner")]
    pub no_banner: bool,

    #[clap(short, long, help = "Interpret arguments as multiple commands")]
    pub multiple_commands: bool,

    #[clap(short, long, help = "Set the update interval in seconds")]
    pub interval: Option<u64>,

    #[clap(long, help = "Style (one of: default, abs-delta, delta, fancy, fancy-net)")]
    pub style: Option<String>,

    pub commands: Vec<String>,
}
