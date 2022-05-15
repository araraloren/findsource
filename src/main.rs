use std::collections::HashSet;

use aopt::Result;
use aopt::{err::create_error, prelude::*};
use aopt_help::prelude::*;
use async_std::{
    channel::{unbounded, Sender},
    path::PathBuf,
    prelude::StreamExt,
};

#[async_std::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let mut loader = PreParser::default();

    getopt_add!(
        loader,
        "--load=a",
        alias = "-l",
        "Load option setting from configuration",
    )?;
    getopt_add!(loader, "--debug=b", alias = "-d", "Print debug message")?;
    getopt_add!(loader, "--help=b", alias = "-?", "Print help message")?;
    getopt_add!(
        loader,
        "--verbose=b",
        alias = "-v",
        "Print more debug message"
    )?;

    // load config name to loader
    getopt!(&mut std::env::args().skip(1), loader)?;

    let noa = loader.get_service().get_noa();
    let args = noa.iter().map(|v| v.to_string());
    let debug = value_of(&loader, "--debug", false);
    let display_help = value_of(&loader, "--help", false);
    let verbose = value_of(&loader, "--verbose", false);
    let config_names = loader["--load"].get_value().as_vec();
    let mut finder = ForwardParser::default();

    getopt_add!(
        finder,
        "--whole=a",
        alias = "-w",
        "Extension category: match whole filename"
    )?;
    getopt_add!(
        finder,
        "--Whole=a",
        alias = "-W",
        "Exclude given whole filename"
    )?;
    getopt_add!(
        finder,
        "--extension=a",
        alias = "-e",
        "Extension category: match file extension"
    )?;
    getopt_add!(
        finder,
        "--Extension=a",
        alias = "-E",
        "Exclude given file extension"
    )?;
    getopt_add!(
        finder,
        "--ignore-case=b",
        alias = "-i",
        "Enable ignore case mode"
    )?;
    getopt_add!(
        finder,
        "--exclude=a",
        alias = "-x",
        "Exclude given file category"
    )?;
    getopt_add!(
        finder,
        "--only=s",
        alias = "-o",
        "Only search given file category"
    )?;
    getopt_add!(finder, "--reverse=b/", alias = "-r", "Disable reverse mode")?;
    getopt_add!(finder, "--hidden=b", alias = "-a", "Search hidden file")?;
    getopt_add!(
        finder,
        "path=p@*",
        "[file or directory]+",
        simple_pos_mut_cb!(move |uid, set: &mut SimpleSet, path, _, _| {
            let metadata = std::fs::metadata(path)
                .map_err(|e| create_error(format!("Can not get metadata for {}: {:?}", path, e)))?;
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
        })
    )?;
    let config_directories = get_configuration_directories();
    let mut options = vec![];

    if let Some(config_names) = config_names {
        for config_name in config_names {
            let config_name = format!("{}.json", config_name);
            let json_config = try_to_load_configuration(&config_directories, &config_name)
                .map_err(|e| {
                    create_error(format!(
                        "Unknow configuration name {}: {:?}",
                        &config_name, e
                    ))
                })?;
            let config: opt_serde::JsonOpt = serde_json::from_str(&json_config).map_err(|e| {
                create_error(format!(
                    "Unknow configuration format of {}: {:?}",
                    &config_name, e
                ))
            })?;

            if debug {
                eprintln!("load config {:?}", &config_name);
            }
            for opt in config.opts {
                options.push(*opt.get_opt());
                opt.add_self_to(finder.get_set_mut())?;
                if debug && verbose {
                    eprintln!("load option {} from config", opt.get_opt());
                }
            }
        }
    }

    let (sender, receiver) = unbounded();
    let ret = getopt!(args, finder); // parsing option from left arguments
    let has_sepcial_error = if let Err(e) = &ret {
        e.is_special()
    } else {
        ret?;
        return Ok(());
    };
    let no_option_matched = if let Ok(opt) = &ret {
        opt.is_none()
    } else {
        false
    };

    if has_sepcial_error || no_option_matched || display_help {
        if has_sepcial_error {
            eprintln!("{}\n", ret.unwrap_err());
        }
        return print_help(loader.get_set(), finder.get_set()).await;
    }
    async_std::task::spawn(find_given_ext_in_directory(
        options, sender, finder, debug, verbose,
    ));
    while let Ok(Some(data)) = receiver.recv().await {
        println!("{}", data);
    }

    Ok(())
}

fn value_of(set: &SimpleSet, name: &str, default_value: bool) -> bool {
    *set[name].get_value().as_bool().unwrap_or(&default_value)
}

async fn find_given_ext_in_directory(
    options: Vec<Ustr>,
    sender: Sender<Option<String>>,
    mut parser: Parser<SimpleSet, DefaultService, ForwardPolicy>,
    debug: bool,
    verbose: bool,
) -> color_eyre::Result<()> {
    let mut whos = HashSet::<String>::default();
    let mut exts = HashSet::<String>::default();
    let mut paths = vec![];

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
            .map(|only| only.eq(name1) || only.eq(name2))
            .unwrap_or(true)
    };
    let exclude_checker = move |name1: &str, name2: &str| -> bool {
        exclude
            .as_ref()
            .map(|ex| ex.iter().any(|v| v.eq(name1) || v.eq(name2)))
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
    for opt in options {
        if only_checker(opt.as_str(), "") && !exclude_checker(opt.as_str(), "") {
            if let Some(opt_exts) = parser[opt.as_ref()].get_value_mut().take_vec() {
                for ext in opt_exts {
                    exts.insert(ext);
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
        println!("What extension or filename do you want search, try command: fs -? or fs --help",);
        return Ok(());
    }
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
        println!("Which path do you want search, try command: fs -?",);
        return Ok(());
    }
    while !paths.is_empty() {
        let mut next_paths = vec![];

        if debug && verbose {
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
                            } else if check_file_extension(file_name, &whos, &exts) {
                                sender.send(Some(path_str.to_owned())).await?;
                            }
                        } else if debug {
                            eprintln!("IGNORE file {}", path_str);
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
        None | Some(0) => whos.contains(path),
        Some(pos) => {
            let (_, ext) = path.split_at(pos + 1);

            exts.contains(ext) || whos.contains(path)
        }
    }
}

fn try_to_load_configuration(
    config_directories: &[Option<std::path::PathBuf>],
    name: &str,
) -> Result<String> {
    for path in config_directories.iter().flatten() {
        let config = path.join(name);

        if config.is_file() {
            return std::fs::read_to_string(&config)
                .map_err(|e| create_error(format!("Can not read from {:?}: {:?}", &config, e)));
        }
    }
    let mut error_message = String::from("Can not find configuration file in ");

    for path in config_directories.iter().flatten() {
        error_message += "'";
        error_message += path.to_str().unwrap_or("None");
        error_message += "' ";
    }
    Err(create_error(error_message))
}

fn get_configuration_directories() -> Vec<Option<std::path::PathBuf>> {
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

async fn print_help(set: &SimpleSet, finder_set: &SimpleSet) -> color_eyre::Result<()> {
    let mut help = getopt_help!(finder_set);

    help.set_name("fs".into());

    let global = help.store.get_global_mut();
    for opt in set.opt_iter() {
        if opt.match_style(aopt::opt::Style::Pos) {
            global.add_pos(PosStore::new(
                opt.get_name(),
                opt.get_hint(),
                opt.get_help(),
                opt.get_index().unwrap().to_string().into(),
                opt.get_optional(),
            ));
        } else if opt.match_style(aopt::opt::Style::Argument)
            || opt.match_style(aopt::opt::Style::Boolean)
            || opt.match_style(aopt::opt::Style::Multiple)
        {
            global.add_opt(OptStore::new(
                opt.get_name(),
                opt.get_hint(),
                opt.get_help(),
                opt.get_type_name(),
                opt.get_optional(),
            ));
        }
    }
    help.print_cmd_help(None)?;
    Ok(())
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
            let prefixs: Vec<Ustr> = set.get_prefix().to_vec();

            if let Ok(Some(opt)) = set.find_mut(self.get_opt()) {
                if !self.hint.is_empty() {
                    opt.set_hint(*self.get_hint());
                }
                if !self.help.is_empty() {
                    opt.set_help(*self.get_help());
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
                    commit.set_default_value(OptValue::Array(self.get_value().to_vec()));
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
                            opt.add_alias(*prefix, name.into());
                            break;
                        }
                    }
                }
            }
        }

        fn insert_default_value(&self, opt: &mut dyn aopt::opt::Opt) {
            let mut value = Vec::with_capacity(self.get_value().len());

            if let Some(v) = opt.get_default_value().as_slice() {
                value.extend_from_slice(v);
            }
            value.extend_from_slice(self.get_value());
            opt.set_default_value(OptValue::from(value));
        }
    }
}
