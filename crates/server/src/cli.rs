// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The operator commands the binary answers besides serving.

use thiserror::Error;

use crate::identity::{self, FlagChange};
use crate::store::{Store, StoreError};

/// What the operator typed after the binary name.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Nothing: serve.
    Serve,
    /// `instance-admin grant <login>` or `instance-admin revoke <login>`.
    InstanceAdmin { grant: bool, login: String },
}

/// What the operator reads on stderr when a command does not run.
#[derive(Debug, Error)]
pub enum CliError {
    #[error("usage: huliho [instance-admin grant <login> | instance-admin revoke <login>]")]
    Usage,
    #[error("no user signs in as {0}")]
    UnknownLogin(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Reads the arguments after the binary name.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for anything but the known shapes.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, CliError> {
    let args: Vec<String> = args.into_iter().collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => Ok(Command::Serve),
        ["instance-admin", "grant", login] => Ok(Command::InstanceAdmin {
            grant: true,
            login: (*login).to_owned(),
        }),
        ["instance-admin", "revoke", login] => Ok(Command::InstanceAdmin {
            grant: false,
            login: (*login).to_owned(),
        }),
        _ => Err(CliError::Usage),
    }
}

/// Grants or revokes the instance-admin flag and says what changed.
///
/// # Errors
///
/// Returns [`CliError::UnknownLogin`] for a sign-in name nobody uses;
/// database failures pass through.
pub fn instance_admin(store: &Store, grant: bool, login: &str) -> Result<String, CliError> {
    let change = if grant {
        identity::grant_instance_admin(store, login)
    } else {
        identity::revoke_instance_admin(store, login)
    };
    let change = match change {
        Ok(change) => change,
        Err(StoreError::NotFound) => return Err(CliError::UnknownLogin(login.to_owned())),
        Err(other) => return Err(other.into()),
    };
    Ok(match (grant, change) {
        (true, FlagChange::Changed) => format!("instance admin granted to {login}"),
        (true, FlagChange::Unchanged) => format!("{login} already is an instance admin"),
        (false, FlagChange::Changed) => format!("instance admin revoked for {login}"),
        (false, FlagChange::Unchanged) => format!("{login} is not an instance admin"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::create_personal_user;
    use crate::scope;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn no_argument_serves_and_the_two_subcommands_parse() {
        assert_eq!(parse(Vec::new()).unwrap(), Command::Serve);
        assert_eq!(
            parse(words("instance-admin grant mira")).unwrap(),
            Command::InstanceAdmin {
                grant: true,
                login: "mira".to_owned(),
            }
        );
        assert_eq!(
            parse(words("instance-admin revoke mira")).unwrap(),
            Command::InstanceAdmin {
                grant: false,
                login: "mira".to_owned(),
            }
        );
    }

    #[test]
    fn anything_else_is_a_usage_error() {
        for text in [
            "serve",
            "instance-admin",
            "instance-admin grant",
            "instance-admin grant mira extra",
            "instance-admin promote mira",
            "rollback",
        ] {
            assert!(matches!(parse(words(text)), Err(CliError::Usage)), "{text}");
        }
    }

    #[test]
    fn granting_and_revoking_say_what_changed_and_land_on_the_scope() {
        let store = Store::in_memory().unwrap();
        let (_, user) = create_personal_user(&store, "mira@example.com").unwrap();
        assert_eq!(
            instance_admin(&store, true, "mira@example.com").unwrap(),
            "instance admin granted to mira@example.com"
        );
        assert_eq!(
            instance_admin(&store, true, "mira@example.com").unwrap(),
            "mira@example.com already is an instance admin"
        );
        assert!(
            scope::resolve(&store, &user.id, None)
                .unwrap()
                .instance_admin()
        );
        assert_eq!(
            instance_admin(&store, false, "mira@example.com").unwrap(),
            "instance admin revoked for mira@example.com"
        );
        assert_eq!(
            instance_admin(&store, false, "mira@example.com").unwrap(),
            "mira@example.com is not an instance admin"
        );
        assert!(
            !scope::resolve(&store, &user.id, None)
                .unwrap()
                .instance_admin()
        );
    }

    #[test]
    fn an_unknown_login_is_named_back() {
        let store = Store::in_memory().unwrap();
        let result = instance_admin(&store, true, "ghost");
        assert!(matches!(result, Err(CliError::UnknownLogin(login)) if login == "ghost"));
    }
}
