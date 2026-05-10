use crate::{
    i18n::TelegramUiCatalog,
    telegram_api::{TelegramBotCommand, TelegramSetMyCommandsRequest},
};

pub const TELEGRAM_COCKPIT_COMMANDS: [TelegramCommandSpec; 16] = [
    TelegramCommandSpec::new(TelegramCockpitCommand::Start, "start"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Help, "help"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Status, "status"),
    TelegramCommandSpec::new(TelegramCockpitCommand::New, "new"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Switch, "switch"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Sessions, "sessions"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Profiles, "profiles"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Continue, "continue"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Abort, "abort"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Queue, "queue"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Approvals, "approvals"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Processes, "processes"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Process, "process"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Manifest, "manifest"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Subagent, "subagent"),
    TelegramCommandSpec::new(TelegramCockpitCommand::Settings, "settings"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramCockpitCommand {
    Start,
    Help,
    Status,
    New,
    Switch,
    Sessions,
    Profiles,
    Continue,
    Abort,
    Queue,
    Approvals,
    Processes,
    Process,
    Manifest,
    Subagent,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelegramCommandSpec {
    pub id: TelegramCockpitCommand,
    pub name: &'static str,
}

impl TelegramCommandSpec {
    pub const fn new(id: TelegramCockpitCommand, name: &'static str) -> Self {
        Self { id, name }
    }
}

impl TelegramCockpitCommand {
    pub fn from_name(name: &str) -> Option<Self> {
        TELEGRAM_COCKPIT_COMMANDS
            .iter()
            .find(|command| command.name == name)
            .map(|command| command.id)
    }

    pub fn name(self) -> &'static str {
        TELEGRAM_COCKPIT_COMMANDS
            .iter()
            .find(|command| command.id == self)
            .expect("every cockpit command has a command spec")
            .name
    }
}

pub fn telegram_command_menu_request(catalog: TelegramUiCatalog) -> TelegramSetMyCommandsRequest {
    TelegramSetMyCommandsRequest {
        commands: telegram_bot_commands(catalog),
        language_code: None,
    }
}

pub fn telegram_bot_commands(catalog: TelegramUiCatalog) -> Vec<TelegramBotCommand> {
    TELEGRAM_COCKPIT_COMMANDS
        .into_iter()
        .map(|command| TelegramBotCommand {
            command: command.name.into(),
            description: catalog.command_description(command.id).into(),
        })
        .collect()
}

pub fn render_command_help(catalog: TelegramUiCatalog) -> String {
    let mut text = catalog.command_help_title().to_owned();
    for command in TELEGRAM_COCKPIT_COMMANDS {
        text.push('\n');
        text.push_str(&catalog.command_help_item(command.id));
    }
    text
}

pub fn render_unknown_command_help(name: &str, catalog: TelegramUiCatalog) -> String {
    format!(
        "{}\n\n{}",
        catalog.unknown_command(name),
        render_command_help(catalog)
    )
}

#[cfg(test)]
mod tests {
    use super::{TELEGRAM_COCKPIT_COMMANDS, TelegramCockpitCommand, telegram_bot_commands};
    use crate::i18n::TelegramUiCatalog;
    use noloong_agent::Locale;

    #[test]
    fn commands_include_task_ten_surface() {
        let names = TELEGRAM_COCKPIT_COMMANDS
            .into_iter()
            .map(|command| command.name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "start",
                "help",
                "status",
                "new",
                "switch",
                "sessions",
                "profiles",
                "continue",
                "abort",
                "queue",
                "approvals",
                "processes",
                "process",
                "manifest",
                "subagent",
                "settings"
            ]
        );
    }

    #[test]
    fn command_from_name_accepts_normalized_names() {
        assert_eq!(
            TelegramCockpitCommand::from_name("status"),
            Some(TelegramCockpitCommand::Status)
        );
    }

    #[test]
    fn bot_commands_are_localized_and_valid_for_telegram() {
        let commands = telegram_bot_commands(TelegramUiCatalog::new(Locale::Zh));

        assert_eq!(commands.len(), TELEGRAM_COCKPIT_COMMANDS.len());
        assert!(
            commands
                .iter()
                .any(|command| command.command == "approvals")
        );
        assert!(
            commands
                .iter()
                .any(|command| command.description == "列出待处理审批")
        );
        assert!(commands.iter().all(|command| {
            !command.command.starts_with('/')
                && command.command.len() <= 32
                && (3..=256).contains(&command.description.chars().count())
        }));
    }
}
