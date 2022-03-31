use std::collections::HashSet;

use aopt::{err::create_error, prelude::*};
use aopt_help::prelude::*;
use async_std::{
    channel::{unbounded, Sender},
    path::PathBuf,
    prelude::StreamExt,
};

#[async_std::main]
async fn main() -> color_eyre::Result<()> {
    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    color_eyre::install()?;
    let mut loader = DynParser::<SimpleSet, DefaultService>::new_policy(PrePolicy::default());
    let config_directories = get_configuration_directories();

    loader
        .add_opt_cb(
            "--load=a",
            simple_opt_mut_cb!(move |uid, set: &mut SimpleSet, value| {
                let mut ret = vec![];

                if let Some(inner_value) = set[uid].get_value_mut().take_vec() {
                    ret = inner_value;
                }
                if let Some(config_names) = value.as_slice() {
                    for config_name in config_names {
                        let config_name = format!("{}.json", config_name);
                        let json_config =
                            try_to_load_configuration(&config_directories, &config_name).map_err(
                                |e| {
                                    create_error(format!(
                                        "Unknow configuration name {}: {:?}",
                                        config_name, e
                                    ))
                                },
                            )?;
                        let config: opt_serde::JsonOpt = serde_json::from_str(&json_config)
                            .map_err(|e| {
                                create_error(format!(
                                    "Unknow configuration format of {}: {:?}",
                                    config_name, e
                                ))
                            })?;

                        for opt in config.opts {
                            ret.push(opt.get_opt().to_string());
                            opt.add_self_to(set)?;
                        }
                    }
                }
                Ok(Some(OptValue::from(ret)))
            }),
        )?
        .add_alias("-l")?
        .set_help("Load option setting from configuration")
        .commit()?;
    loader
        .add_opt("--debug=b")?
        .add_alias("-d")?
        .set_help("Print debug message")
        .commit()?;
    loader
        .add_opt("--help=b")?
        .add_alias("-?")?
        .set_help("Print help message")
        .commit()?;

    getoptd!(&mut std::env::args().skip(1), loader)?;

    let mut finder = loader;
    let noa = finder.get_service().get_noa();
    let args: Vec<String> = noa.iter().map(|v| v.to_string()).collect();
    let debug = value_of(&finder, "--debug", false);
    let display_help = value_of(&finder, "--help", false);
    let loaded = finder["--load"].get_value_mut().take_vec();

    finder.set_policy(ForwardPolicy::default());
    finder.reset();
    finder
        .add_opt("--whole=a")?
        .add_alias("-w")?
        .set_help("Extension category: match whole filename")
        .commit()?;
    finder
        .add_opt("--Whole=a")?
        .add_alias("-W")?
        .set_help("Exclude given whole filename")
        .commit()?;
    finder
        .add_opt("--extension=a")?
        .add_alias("-e")?
        .set_help("Extension category: match file extension")
        .commit()?;
    finder
        .add_opt("--Extension=a")?
        .add_alias("-E")?
        .set_help("Exclude given file extension")
        .commit()?;
    finder
        .add_opt("--ignore-case=b")?
        .add_alias("-i")?
        .set_help("Enable ignore case mode")
        .commit()?;
    finder
        .add_opt("--exclude=a")?
        .add_alias("-x")?
        .set_help("Exclude given file category")
        .commit()?;
    finder
        .add_opt("--only=s")?
        .add_alias("-o")?
        .set_help("Only search given file category")
        .commit()?;
    finder
        .add_opt("--reverse=b/")?
        .add_alias("-r")?
        .set_help("Disable reverse mode")
        .commit()?;
    finder
        .add_opt("--hidden=b")?
        .add_alias("-a")?
        .set_help("Search hidden file")
        .commit()?;
    finder
        .add_opt_cb(
            "path=p@*",
            simple_pos_mut_cb!(move |uid, set: &mut SimpleSet, path, _, _| {
                let metadata = std::fs::metadata(path).map_err(|e| {
                    create_error(format!("Can not get metadata for {}: {:?}", path, e))
                })?;
                if !metadata.is_file() && !metadata.is_dir() {
                    Err(create_error(format!("{} is not a valid path!", path)))
                } else {
                    let mut ret = vec![path.to_string()];

                    if let Some(inner_value) = set[uid].get_value_mut().take_vec() {
                        for value in inner_value {
                            ret.push(value);
                        }
                    }
                    Ok(Some(OptValue::from(ret)))
                }
            }),
        )?
        .set_help("[file or directory]+")
        .commit()?;

    let (sender, receiver) = unbounded();

    if getoptd!(args.into_iter(), finder)?.is_none() || display_help {
        return print_help(finder.get_set(), None).await;
    }

    async_std::task::spawn(find_given_ext_in_directory(loaded, sender, finder, debug));

    while let Ok(Some(data)) = receiver.recv().await {
        println!("{}", data);
    }

    Ok(())
}

fn value_of(set: &SimpleSet, name: &str, default_value: bool) -> bool {
    *set[name].get_value().as_bool().unwrap_or(&default_value)
}

async fn find_given_ext_in_directory(
    loaded: Option<Vec<String>>,
    sender: Sender<Option<String>>,
    mut parser: DynParser<SimpleSet, DefaultService>,
    debug: bool,
) -> color_eyre::Result<()> {
    let mut whos = HashSet::<String>::default();
    let mut exts = HashSet::<String>::default();

    let only = parser["--only"].get_value_mut().take_str();
    let exclude = parser["--exclude"].get_value_mut().take_vec();
    let ex_exts = parser["--Extension"].get_value_mut().take_vec();
    let ex_whos = parser["--Whole"].get_value_mut().take_vec();
    let whole = parser["--whole"].get_value_mut().take_vec();
    let extension = parser["--extension"].get_value_mut().take_vec();

    let ignore_case = value_of(&parser, "--ignore-case", false);
    let reverse = value_of(&parser, "--reverse", true);
    let hidden = value_of(&parser, "--hidden", false);

    let only_checker = |name1: &str, name2: &str| -> bool {
        only.as_ref()
            .and_then(|only| Some(only.eq(name1) || only.eq(name2)))
            .unwrap_or(true)
    };
    let exclude_checker = move |name1: &str, name2: &str| -> bool {
        exclude
            .as_ref()
            .and_then(|ex| Some(ex.iter().any(|v| v.eq(name1) || v.eq(name2))))
            .unwrap_or(false)
    };

    if only_checker("whole", "w") && !exclude_checker("whole", "w") {
        if let Some(opt_exts) = whole {
            for ext in opt_exts {
                whos.insert(ext);
            }
        }
    }
    if only_checker("extension", "e") && !exclude_checker("extension", "e") {
        if let Some(opt_exts) = extension {
            for ext in opt_exts {
                exts.insert(ext);
            }
        }
    }
    if let Some(loaded) = loaded {
        for opt in loaded {
            if only_checker(opt.as_str(), "") && !exclude_checker(opt.as_str(), "") {
                if let Some(opt_exts) = parser[opt.as_ref()].get_value_mut().take_vec() {
                    for ext in opt_exts {
                        exts.insert(ext);
                    }
                }
            }
        }
    }
    if let Some(ex_exts) = ex_exts {
        for ext in ex_exts {
            exts.remove(&ext);
        }
    }
    if let Some(ex_whos) = ex_whos {
        for ext in ex_whos {
            whos.remove(&ext);
        }
    }
    if ignore_case {
        exts = exts.into_iter().map(|v| v.to_lowercase()).collect();
        whos = whos.into_iter().map(|v| v.to_lowercase()).collect();
    }
    if debug {
        eprintln!("match whole filename : {:?}", whos);
        eprintln!("match file extension : {:?}", exts);
    }
    if whos.is_empty() && exts.is_empty() {
        if debug {
            eprintln!("No extension found!");
        }
        return print_help(parser.get_set(), Some(sender)).await;
    }

    let mut paths = vec![];

    if let Some(values) = parser["path"].get_value_mut().take_vec() {
        for value in values {
            paths.push(PathBuf::from(value));
        }
    }
    if !atty::is(atty::Stream::Stdin) {
        let mut buff = String::default();

        while let Ok(count) = std::io::stdin().read_line(&mut buff) {
            if count > 0 {
                paths.push(PathBuf::from(buff.trim()));
            } else {
                break;
            }
        }
    }
    if paths.is_empty() {
        if debug {
            eprintln!("No path need to search!");
        }
        return print_help(parser.get_set(), Some(sender)).await;
    }
    while !paths.is_empty() {
        let mut next_paths = vec![];

        if debug {
            eprintln!("search file in path: {:?}", paths);
        }
        for path in paths {
            let meta = async_std::fs::metadata(&path).await?;

            if reverse && meta.is_dir() {
                let mut entries = path.read_dir().await?;

                while let Some(entry) = entries.next().await {
                    let entry = entry?;

                    next_paths.push(entry.path())
                }
            } else if meta.is_file() {
                if let Some(path_str) = path.to_str() {
                    if let Some(Some(file_name)) = path.file_name().map(|v| v.to_str()) {
                        if !file_name.starts_with('.') || hidden {
                            if debug {
                                eprintln!("checking file {}", path_str);
                            }
                            if ignore_case {
                                if check_file_extension(
                                    file_name.to_lowercase().as_ref(),
                                    &whos,
                                    &exts,
                                ) {
                                    sender.send(Some(path_str.to_owned())).await?;
                                }
                            } else {
                                if check_file_extension(file_name, &whos, &exts) {
                                    sender.send(Some(path_str.to_owned())).await?;
                                }
                            }
                        } else {
                            if debug {
                                eprintln!("IGNORE file {}", path_str);
                            }
                        }
                    }
                }
            } else if debug {
                eprintln!("WARN: {:?} is not a valid file", path);
            }
        }
        paths = next_paths;
    }

    sender.send(None).await?;
    Ok(())
}

fn check_file_extension(path: &str, whos: &HashSet<String>, exts: &HashSet<String>) -> bool {
    match path.rfind('.') {
        None | Some(0) => {
            if whos.contains(path) {
                return true;
            }
        }
        Some(pos) => {
            let (_, ext) = path.split_at(pos + 1);

            if exts.contains(ext) {
                return true;
            }
        }
    }
    false
}

fn try_to_load_configuration(
    config_directories: &[Option<std::path::PathBuf>],
    name: &str,
) -> color_eyre::Result<String> {
    for path in config_directories.iter() {
        if let Some(path) = path {
            let config = path.join(name);

            if config.is_file() {
                return Ok(std::fs::read_to_string(config)?);
            }
        }
    }
    let mut error_message = String::from("Can not find configuration file in ");

    for path in config_directories.iter() {
        if let Some(path) = path {
            error_message += "'";
            error_message += path.to_str().unwrap_or("None");
            error_message += "' ";
        }
    }
    Err(create_error(error_message))?
}

fn get_configuration_directories() -> Vec<Option<std::path::PathBuf>> {
    vec![
        // find configuration in exe directory
        std::env::current_exe().ok().and_then(|mut v| {
            v.pop();
            Some(v)
        }),
        // find configuration in working directory
        std::env::current_dir().ok(),
        // find directory in given directory(runtime)
        std::env::var("FS_CONFIG_DIR")
            .ok()
            .and_then(|v| Some(std::path::PathBuf::from(v))),
        Some(
            // find configuration in given directory(compile time)
            if let Some(env_compile_dir) = option_env!("FS_BUILD_CONFIG_DIR") {
                std::path::PathBuf::from(env_compile_dir)
            } else {
                // or find in current directory
                std::path::PathBuf::from(".")
            },
        ),
    ]
}

async fn print_help(
    set: &SimpleSet,
    sender: Option<Sender<Option<String>>>,
) -> color_eyre::Result<()> {
    let mut help = getopt_help!(set);

    help.print_cmd_help(None)?;

    if let Some(sender) = sender {
        sender.send(None).await?;
    }
    return Ok(());
}

mod opt_serde {
    use aopt::prelude::*;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Debug, Default, Deserialize, Serialize)]
    pub struct JsonOpt {
        pub opts: Vec<OptSerde>,
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    pub struct OptSerde {
        opt: Ustr,
        #[serde(default)]
        hint: Ustr,
        #[serde(default)]
        help: Ustr,
        #[serde(default)]
        alias: Vec<Ustr>,
        #[serde(default)]
        value: Vec<String>,
    }

    impl OptSerde {
        pub fn get_opt(&self) -> &Ustr {
            &self.opt
        }

        pub fn get_hint(&self) -> &Ustr {
            &self.hint
        }

        pub fn get_help(&self) -> &Ustr {
            &self.help
        }

        pub fn get_alias(&self) -> &Vec<Ustr> {
            &self.alias
        }

        pub fn get_value(&self) -> &Vec<String> {
            &self.value
        }

        // pub fn set_opt(&mut self, opt: Ustr) {
        //     self.opt = opt;
        // }

        // pub fn set_hint(&mut self, hint: Ustr) {
        //     self.hint = hint;
        // }

        // pub fn set_help(&mut self, help: Ustr) {
        //     self.help = help;
        // }

        // pub fn set_alias(&mut self, alias: Vec<Ustr>) {
        //     self.alias = alias;
        // }

        // pub fn set_value(&mut self, value: Vec<String>) {
        //     self.value = value;
        // }

        // pub fn get_opt_mut(&mut self) -> &mut Ustr {
        //     &mut self.opt
        // }

        // pub fn get_hint_mut(&mut self) -> &mut Ustr {
        //     &mut self.hint
        // }

        // pub fn get_help_mut(&mut self) -> &mut Ustr {
        //     &mut self.help
        // }

        // pub fn get_alias_mut(&mut self) -> &mut Vec<Ustr> {
        //     &mut self.alias
        // }

        // pub fn get_value_mut(&mut self) -> &mut Vec<String> {
        //     &mut self.value
        // }

        pub fn add_self_to<S: Set>(&self, set: &mut S) -> aopt::Result<()> {
            let prefixs: Vec<Ustr> = set.get_prefix().iter().map(|v| v.clone()).collect();

            if let Some(opt) = set.find_mut(self.get_opt())? {
                if !self.hint.is_empty() {
                    opt.set_hint(self.get_hint().clone());
                }
                if !self.help.is_empty() {
                    opt.set_help(self.get_help().clone());
                }
                if !self.alias.is_empty() {
                    self.insert_alias(opt.as_mut(), &prefixs);
                }
                if !self.value.is_empty() {
                    self.insert_default_value(opt.as_mut());
                }
            } else {
                let mut commit = set.add_opt(self.get_opt())?;

                if !self.hint.is_empty() {
                    commit.set_hint(self.get_hint());
                }
                if !self.help.is_empty() {
                    commit.set_help(self.get_help());
                }
                for alias in self.get_alias() {
                    commit.add_alias(alias.as_str())?;
                }
                if !self.value.is_empty() {
                    commit.set_default_value(OptValue::Array(
                        self.get_value().iter().map(|v| v.clone()).collect(),
                    ));
                }
                commit.commit()?;
            }
            Ok(())
        }

        fn insert_alias(&self, opt: &mut dyn aopt::opt::Opt, prefixs: &[Ustr]) {
            for alias in self.get_alias().iter() {
                for prefix in prefixs.iter() {
                    if alias.starts_with(prefix.as_str()) {
                        if let Some(name) = alias.get(prefix.len()..) {
                            opt.add_alias(prefix.clone(), name.into());
                            break;
                        }
                    }
                }
            }
        }

        fn insert_default_value(&self, opt: &mut dyn aopt::opt::Opt) {
            let mut value = Vec::with_capacity(self.get_value().len());

            opt.get_default_value()
                .as_slice()
                .and_then(|v| Some(value.extend_from_slice(v)));
            value.extend_from_slice(self.get_value());
            opt.set_default_value(OptValue::from(value));
        }
    }
}
