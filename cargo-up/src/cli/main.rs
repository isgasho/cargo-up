use cargo_metadata::{CargoOpt, MetadataCommand};
use clap::{AppSettings, Clap};

use std::process::exit;

mod dep;
mod utils;

use utils::term::{TERM_ERR, TERM_OUT};

#[derive(Debug, Clap)]
enum Subcommand {
    Dep(dep::Dep),
}

#[derive(Debug, Clap)]
#[clap(version, global_setting(AppSettings::VersionlessSubcommands))]
struct Opt {
    /// Path to workspace Cargo.toml
    #[clap(long, value_name = "path")]
    manifest_path: Option<String>,

    #[clap(subcommand)]
    subcommand: Subcommand,
}

#[derive(Debug, Clap)]
#[clap(
    name = "cargo-up",
    bin_name = "cargo",
    version,
    global_setting(AppSettings::ColoredHelp)
)]
enum Cargo {
    Up(Opt),
}

fn main() {
    let Cargo::Up(opt) = Cargo::parse();

    let mut cmd = MetadataCommand::new();

    cmd.features(CargoOpt::AllFeatures);

    if let Some(path) = opt.manifest_path {
        cmd.manifest_path(path);
    }

    let metadata = match cmd.exec() {
        Ok(x) => x,
        Err(err) => {
            eprintln!("{}", err.to_string());
            exit(1);
        }
    };

    let err = match opt.subcommand {
        Subcommand::Dep(x) => x.run(metadata),
    }
    .err();

    let code = if let Some(e) = err {
        e.print_err().unwrap();
        1
    } else {
        0
    };

    TERM_ERR.flush().unwrap();
    TERM_OUT.flush().unwrap();

    exit(code)
}
