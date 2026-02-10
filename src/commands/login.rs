use clap::Parser;
use inquire::{Text, validator::Validation};

use crate::{buckal_error, buckal_log, config::Config, utils::UnwrapOrExit};

#[derive(Parser, Debug)]
pub struct LoginArgs {
    /// Registry to use
    #[arg(long)]
    pub registry: Option<String>,
}

pub fn execute(args: &LoginArgs) {
    let mut config = Config::load();

    let validator = |input: &str| {
        if input.chars().count() > 0 {
            Ok(Validation::Valid)
        } else {
            Ok(Validation::Invalid("Token cannot be empty.".into()))
        }
    };

    let registry_name = args
        .registry
        .as_deref()
        .unwrap_or(config.registry.default.as_deref().unwrap_or("buck2hub"));

    if let Some(registry) = config.registries.get_mut(registry_name) {
        let token = Text::new(
            format!(
                "Please paste the token found on {}/me/settings below\n ",
                registry.base
            )
            .as_str(),
        )
        .with_placeholder("Token")
        .with_validator(validator)
        .prompt()
        .unwrap_or_exit();
        registry.token = Some(token);
        config.save().unwrap_or_exit();
        buckal_log!("Login", format!("token for `{}` saved", registry_name));
    } else {
        buckal_error!("Registry `{}` not found in configuration", registry_name);
        std::process::exit(1);
    }
}
