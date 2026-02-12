use clap::Parser;

use crate::{buckal_error, buckal_log, buckal_note, config::Config, utils::UnwrapOrExit};

#[derive(Parser, Debug)]
pub struct LogoutArgs {
    /// Registry to use
    #[arg(long)]
    pub registry: Option<String>,
}

pub fn execute(args: &LogoutArgs) {
    let mut config = Config::load();

    let registry_name = args
        .registry
        .as_deref()
        .unwrap_or_else(|| config.default_registry())
        .to_string();

    if let Some(registry) = config.registries.get_mut(&registry_name) {
        if registry.token.is_none() {
            buckal_log!(
                "Logout",
                format!("not currently logged in to `{}`", registry_name)
            );
        } else {
            registry.token = None;
            config.save().unwrap_or_exit();
            buckal_log!(
                "Logout",
                format!(
                    "token for `{}` has been removed from local storage",
                    registry_name
                )
            );
            buckal_note!(
                "This does not invalidate the token, you may want to revoke it on the registry website if you no longer need it."
            );
        }
    } else {
        buckal_error!("registry `{}` not found in configuration", registry_name);
        std::process::exit(1);
    }
}
