use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, NetPort, RestrictionStatus, Ruleset,
    RulesetAttr, RulesetCreatedAttr, RulesetError, Scope, path_beneath_rules,
};
use log::{info, warn};

use crate::config::Args;

pub fn init_landlock(args: &Args) -> Result<(), RulesetError> {
    let handle_success = |name: &str| {
        info!("Landlock {} ruleset created.", name);
    };
    let handle_error = |name: &str| {
        warn!(
            "Landlock {} ruleset could not be created. This can be due to an old kernel.",
            name
        )
    };

    let handle_result = |result: Result<RestrictionStatus, RulesetError>, name: &str| match result {
        Ok(status) => match status.ruleset {
            landlock::RulesetStatus::FullyEnforced => {
                handle_success(name);
            }
            _ => {
                handle_error(name);
            }
        },
        Err(_) => {
            handle_error(name);
        }
    };

    handle_result(
        Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(ABI::V1))?
            .create()?
            .add_rules(path_beneath_rules(
                [&args.bees_work_dir],
                AccessFs::from_read(ABI::V1),
            ))?
            .restrict_self(),
        "filesystem",
    );

    handle_result(
        Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessNet::from_all(ABI::V4))?
            .create()?
            .add_rule(NetPort::new(args.port, AccessNet::BindTcp))?
            .restrict_self(),
        "network",
    );

    handle_result(
        Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .scope(Scope::from_all(ABI::V6))?
            .create()?
            .restrict_self(),
        "scope",
    );

    Ok(())
}
