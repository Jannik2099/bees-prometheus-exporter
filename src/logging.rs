use anyhow::Result;
use log::LevelFilter;
use log4rs::{
    Config,
    append::console::ConsoleAppender,
    config::{Appender, Root},
};
use std::str::FromStr;

pub fn init_logger(level: &str) -> Result<()> {
    let stdout = ConsoleAppender::builder().build();
    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .build(
            Root::builder()
                .appender("stdout")
                .build(LevelFilter::from_str(level)?),
        )?;

    log4rs::init_config(config)?;
    Ok(())
}
