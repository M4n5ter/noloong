use crate::{cli::CliError, schema};
use clap::{Args, Subcommand};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) fn run_profile_config_schema(
    options: ProfileConfigSchemaOptions,
) -> Result<(), CliError> {
    if options.output.is_some() && options.check.is_some() {
        return Err(CliError::Schema(
            "--output cannot be used together with --check".into(),
        ));
    }
    if let Some(check_path) = options.check {
        return check_profile_config_schema(&check_path);
    }
    let schema = schema::profile_config_schema_json();
    if let Some(output_path) = options.output {
        return write_profile_config_schema(&output_path, &schema);
    }
    io::stdout().lock().write_all(schema.as_bytes())?;
    Ok(())
}

fn check_profile_config_schema(path: &Path) -> Result<(), CliError> {
    let current = fs::read_to_string(path)?;
    let expected = schema::profile_config_schema_json();
    if current == expected {
        return Ok(());
    }
    Err(CliError::Schema(format!(
        "profile config schema is out of date: {}; regenerate it with `noloong profile-config schema --output {}`",
        path.display(),
        path.display()
    )))
}

fn write_profile_config_schema(path: &Path, schema: &str) -> Result<(), CliError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, schema)?;
    Ok(())
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(crate) struct ProfileConfigSchemaOptions {
    #[arg(long = "output", conflicts_with = "check")]
    pub(crate) output: Option<PathBuf>,
    #[arg(long = "check", conflicts_with = "output")]
    pub(crate) check: Option<PathBuf>,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct ProfileConfigCommand {
    #[command(subcommand)]
    pub(crate) command: ProfileConfigSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub(crate) enum ProfileConfigSubcommand {
    Schema(ProfileConfigSchemaOptions),
}
