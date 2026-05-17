use crate::{build_info, cli::CliError};
use clap::{Args, Subcommand};
use std::{
    io::{self, Write},
    path::PathBuf,
};

pub(crate) fn run_build_info(command: BuildInfoCommand) -> Result<(), CliError> {
    match command.command {
        BuildInfoSubcommand::Manifest => {
            io::stdout()
                .lock()
                .write_all(build_info::manifest_json().as_bytes())?;
        }
        BuildInfoSubcommand::Command => {
            writeln!(io::stdout().lock(), "{}", build_info::build_command()?)?;
        }
        BuildInfoSubcommand::Source(command) => run_build_info_source(command)?,
    }
    Ok(())
}

fn run_build_info_source(command: BuildInfoSourceCommand) -> Result<(), CliError> {
    match command.command {
        BuildInfoSourceSubcommand::List => {
            let mut stdout = io::stdout().lock();
            for path in build_info::source_paths()? {
                writeln!(stdout, "{path}")?;
            }
        }
        BuildInfoSourceSubcommand::Cat(options) => {
            io::stdout()
                .lock()
                .write_all(&build_info::source_file(&options.path)?)?;
        }
        BuildInfoSourceSubcommand::Extract(options) => {
            build_info::extract_source(&options.output_dir, options.force)?;
        }
        BuildInfoSourceSubcommand::Archive(options) => {
            build_info::write_archive(&options.output)?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct BuildInfoCommand {
    #[command(subcommand)]
    pub(crate) command: BuildInfoSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub(crate) enum BuildInfoSubcommand {
    Manifest,
    Command,
    Source(BuildInfoSourceCommand),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct BuildInfoSourceCommand {
    #[command(subcommand)]
    pub(crate) command: BuildInfoSourceSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub(crate) enum BuildInfoSourceSubcommand {
    List,
    Cat(BuildInfoSourceCatOptions),
    Extract(BuildInfoSourceExtractOptions),
    Archive(BuildInfoSourceArchiveOptions),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct BuildInfoSourceCatOptions {
    pub(crate) path: String,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct BuildInfoSourceExtractOptions {
    #[arg(long = "output-dir")]
    pub(crate) output_dir: PathBuf,
    #[arg(long)]
    pub(crate) force: bool,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct BuildInfoSourceArchiveOptions {
    #[arg(long)]
    pub(crate) output: PathBuf,
}
