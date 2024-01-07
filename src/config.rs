use crate::json::JsonOpt;

use aopt::Error;
use cote::*;
use std::path::PathBuf;

const CONFIG: &str = include_str!("../config.json");

pub fn try_to_load_configuration2(
    config_directories: &[Option<std::path::PathBuf>],
    name: &str,
) -> Result<(PathBuf, JsonOpt), Error> {
    let cfg_name = format!("{}.json", name);
    let mut config = PathBuf::from(name);

    // search in config directories
    for path in config_directories.iter().flatten() {
        let handler = path.join(&cfg_name);

        if handler.is_file() {
            config = handler;
            break;
        }
    }
    // if argument is a valid path
    if config.is_file() {
        let context = std::fs::read_to_string(&config)
            .map_err(|e| Error::raise_error(format!("Can not read from {:?}: {:?}", &config, e)))?;

        Ok((
            config,
            serde_json::from_str(&context).map_err(|e| {
                Error::raise_error(format!("Invalid configuration format: {:?}", e))
            })?,
        ))
    } else {
        let mut error_message = String::from("Can not find configuration file in ");

        for path in config_directories.iter().flatten() {
            error_message += "'";
            error_message += path.to_str().unwrap_or("None");
            error_message += "' ";
        }
        Err(Error::raise_error(error_message))
    }
}

pub fn default_json_configuration() -> &'static str {
    CONFIG
}

pub fn get_configuration_directories() -> Vec<Option<std::path::PathBuf>> {
    vec![
        // find configuration in exe directory
        std::env::current_exe().ok().map(|mut v| {
            v.pop();
            v
        }),
        std::env::current_exe().ok().and_then(|mut v| {
            v.pop();
            if let Some(env_compile_dir) = option_env!("FS_BUILD_CONFIG_DIR") {
                v.push(
                    // find configuration in given directory(compile time)
                    env_compile_dir,
                );
                Some(v)
            } else {
                None
            }
        }),
        // find configuration in working directory
        std::env::current_dir().ok(),
        // find directory in given directory(runtime)
        std::env::var("FS_CONFIG_DIR")
            .ok()
            .map(std::path::PathBuf::from),
    ]
}
