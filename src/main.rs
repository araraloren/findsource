use opt_serde::JsonOpt;
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Deref;
use std::path::PathBuf;
use tokio::fs::read_dir;
use tokio::spawn;
use tokio::sync::mpsc::{channel, Sender};

use aopt::prelude::*;
use aopt::Error;
use aopt_help::prelude::Block;
use aopt_help::prelude::Store;
use cote::prelude::*;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let mut loader = APreParser::default();
    let config_directories = get_configuration_directories();
    let mut config_options = vec![];

    loader
        .add_opt("--debug=b")?
        .add_alias("-d")
        .set_help("Print debug message");
    loader
        .add_opt("--help=b")?
        .add_alias("-?")
        .set_help("Print help message");
    loader
        .add_opt("--verbose=b")?
        .add_alias("-v")
        .set_help("Print more debug message");
    loader
        .add_opt("--load=s")?
        .add_alias("-l")
        .set_hint("-l,--load CFG|PATH")
        .set_help("Load option setting from configuration name or file")
        .set_values(Vec::<JsonOpt>::new())
        .on(
            move |set: &mut ASet, ser: &mut ASer, cfg: ctx::Value<String>| {
                let (path, config) = try_to_load_configuration2(&config_directories, cfg.as_str())?;

                if *ser.sve_val(set["--debug"].uid())? {
                    eprintln!("... loading config {:?}", &path);
                }
                Ok(Some(config))
            },
        )?;

    // load config name to loader
    getopt!(std::env::args().skip(1), &mut loader)?;

    let ret_value = loader.take_retval().unwrap_or_default();
    let debug = *loader.find_val("--debug")?;
    let mut display_help = *loader.find_val("--help")?;
    let verbose = *loader.find_val("--verbose")?;
    let mut default_jsons: JsonOpt = serde_json::from_str(default_json_configuration()).unwrap();
    let jsons = loader.find_vals_mut::<JsonOpt>("--load")?;
    let mut finder = Cote::<AFwdPolicy>::default();

    finder.set_auto_help(false);
    finder
        .add_opt("path=p@1..")?
        .set_hint("[PATH]+")
        .set_help("Path need to be search")
        .on(
            move |_: &mut ASet, _: &mut ASer, mut path: ctx::Value<PathBuf>| {
                if debug {
                    eprintln!("... prepare searching path: {:?}", path.deref());
                }
                if !path.is_file() && !path.is_dir() {
                    Err(Error::raise_error(format!(
                        "{:?} is not a valid path!",
                        path
                    )))
                } else {
                    Ok(Some(path.take()))
                }
            },
        )?;
    // merge the json configurations
    for json in jsons {
        for cfg in json.iter().cloned() {
            if !config_options.contains(cfg.option()) {
                config_options.push(cfg.option().clone());
            }
            default_jsons.add_cfg(cfg);
        }
    }
    if debug {
        eprintln!(
            "... loading cfg: {}",
            serde_json::to_string_pretty(&default_jsons)?
        );
        eprintln!("... loading options: {:?}", config_options);
    }
    // add the option to finder
    default_jsons.add_to(&mut finder)?;
    // initialize the option value
    finder.init()?;

    let ret = finder.parse(aopt::Arc::new(Args::from(
        ret_value.into_args().into_iter(),
    )));
    let (sender, mut receiver) = channel(512);

    match ret {
        Ok(None) => {
            display_help = true;
        }
        Err(e) => {
            if debug && e.is_failure() {
                eprintln!("Got a failure: {}\n", e);
                display_help = true;
            } else {
                panic!("{}", e)
            }
        }
        _ => {}
    }
    if display_help {
        return print_help(loader.optset(), finder.optset()).await;
    }
    if debug {
        eprintln!("... Starting search thread ...");
    }
    let printer = spawn(async move {
        while let Some(Some(data)) = receiver.recv().await {
            println!("{}", data);
        }
    });
    find_given_ext_in_directory(config_options, sender, finder, debug, verbose).await?;
    let _ = printer.await?;
    if debug {
        eprintln!("... Searching end");
    }
    Ok(())
}

pub struct Context<'a> {
    debug: bool,

    verbose: bool,

    full: bool,

    hidden: bool,

    reverse: bool,

    ignore_case: bool,

    whos: &'a HashSet<String>,

    exts: &'a HashSet<String>,

    sender: &'a Sender<Option<String>>,
}

async fn find_given_ext_in_directory(
    options: Vec<String>,
    sender: Sender<Option<String>>,
    parser: Cote<AFwdPolicy>,
    debug: bool,
    verbose: bool,
) -> color_eyre::Result<()> {
    let mut whos = HashSet::<String>::default();
    let mut exts = HashSet::<String>::default();
    let mut paths = parser.find_vals("path")?.clone();

    if debug {
        eprintln!("... Got search path: {:?}", paths);
    }

    let only = parser.find_val::<String>("--only");
    let exclude = parser.find_vals::<String>("--Exclude");
    let ex_exts = parser.find_vals::<String>("--Extension");
    let ex_whos = parser.find_vals::<String>("--Whole");
    let whole = parser.find_vals::<String>("--whole");
    let extension = parser.find_vals::<String>("--extension");
    let full = *parser.find_val("--full")?;

    let ignore_case = *parser.find_val("--ignore-case")?;
    let reverse = !*parser.find_val::<bool>("--/reverse")?;
    let hidden = *parser.find_val("--hidden")?;

    let only_checker = |name1: &str, name2: &str| -> bool {
        if let Ok(only) = only {
            only.eq(name1) || only.eq(name2)
        } else {
            true
        }
    };
    let exclude_checker = move |name1: &str, name2: &str| -> bool {
        if let Ok(exclude) = exclude {
            exclude.iter().any(|v| v.eq(name1) || v.eq(name2))
        } else {
            false
        }
    };

    if only_checker("whole", "w") && !exclude_checker("whole", "w") {
        if let Ok(whole) = whole {
            for ext in whole {
                whos.insert(ext.clone());
            }
        }
    }
    if only_checker("extension", "e") && !exclude_checker("extension", "e") {
        if let Ok(extension) = extension {
            for ext in extension {
                exts.insert(ext.clone());
            }
        }
    }
    for opt in options {
        if only_checker(opt.as_str(), "") && !exclude_checker(opt.as_str(), "") {
            if let Ok(opt_exts) = parser.find_vals::<String>(opt.as_str()) {
                for ext in opt_exts {
                    exts.insert(ext.clone());
                }
            }
        }
    }
    if let Ok(ex_exts) = ex_exts {
        for ext in ex_exts {
            exts.remove(ext);
        }
    }
    if let Ok(ex_whos) = ex_whos {
        for ext in ex_whos {
            whos.remove(ext);
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

    let ctx: Context<'_> = Context {
        full,
        debug,
        verbose,
        hidden,
        reverse,
        ignore_case,
        whos: &whos,
        exts: &exts,
        sender: &sender,
    };

    while !paths.is_empty() {
        let mut next_paths = vec![];

        if ctx.debug && ctx.verbose {
            eprintln!("search file in path: {:?}", paths);
        }
        for path in paths {
            let meta = tokio::fs::metadata(&path).await?;

            if ctx.reverse && meta.is_dir() {
                if let Err(e) = process_directory(&path, &mut next_paths, &ctx).await {
                    eprintln!("Error: can not access directory `{:?}`: {:?}", path, e);
                }
            } else if meta.is_file() {
                if let Err(e) = process_file(&path, &mut next_paths, &ctx).await {
                    eprintln!("Error: can not access file `{:?}`: {:?}", path, e);
                }
            } else if ctx.debug {
                eprintln!("WARN: {:?} is not a valid file", path);
            }
        }
        if ctx.debug && ctx.verbose {
            eprintln!("next search file in path: {:?}", next_paths);
        }
        paths = next_paths;
    }
    sender.send(None).await?;
    Ok(())
}

pub async fn process_directory<'a>(
    path: &PathBuf,
    next_paths: &mut Vec<PathBuf>,
    &Context {
        debug,
        verbose,
        full: _,
        hidden,
        reverse: _,
        ignore_case: _,
        whos: _,
        exts: _,
        sender: _,
    }: &Context<'a>,
) -> color_eyre::Result<()> {
    let path = if let Some(Some(file_name)) = path.file_name().map(|v| v.to_str()) {
        if !(file_name.starts_with('.') || file_name.starts_with("$")) || hidden {
            Some(path)
        } else {
            None
        }
    } else {
        Some(path)
    };
    if let Some(path) = path {
        if debug {
            eprintln!("checking directory {:?}", path);
        }
        let mut entries = read_dir(path).await?;

        while let Some(entry) = entries.next_entry().await? {
            if debug && verbose {
                eprintln!("add directory to next paths {:?}", path);
            }
            next_paths.push(entry.path())
        }
    } else if debug {
        eprintln!("IGNORE directory {:?}", path);
    }
    Ok(())
}

pub async fn process_file<'a>(
    path: &PathBuf,
    _next_paths: &mut Vec<PathBuf>,
    &Context {
        debug,
        verbose: _,
        full,
        hidden,
        reverse: _,
        ignore_case,
        whos,
        exts,
        sender,
    }: &Context<'a>,
) -> color_eyre::Result<()> {
    let may_full_path = if full {
        dunce::canonicalize(&path)?
    } else {
        path.clone()
    };

    if let Some(path_str) = may_full_path.to_str() {
        if let Some(Some(file_name)) = path.file_name().map(|v| v.to_str()) {
            if !file_name.starts_with('.') && !file_name.starts_with("$") || hidden {
                if debug {
                    eprintln!("checking file {}", path_str);
                }
                if ignore_case {
                    if check_file_extension(file_name.to_lowercase().as_ref(), &whos, &exts) {
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

fn try_to_load_configuration2(
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

fn default_json_configuration() -> &'static str {
    r#"
    {
        "opts": [
            {
                "id": "whole",
                "option": "-w=s",
                "help": "Extension category: match whole filename",
                "alias": [
                    "--whole"
                ],
                "value": []
            },
            {
                "id": "Whole",
                "option": "-W=s",
                "help": "Exclude given whole filename",
                "alias": [
                    "--Whole"
                ],
                "value": []
            },
            {
                "id": "ext",
                "option": "-e=s",
                "help": "Extension category: match file extension",
                "alias": [
                    "--extension"
                ],
                "value": []
            },
            {
                "id": "Ext",
                "option": "-E=s",
                "help": "Exclude given file extension",
                "alias": [
                    "--Extension"
                ],
                "value": []
            },
            {
                "id": "X",
                "option": "-X=s",
                "help": "Exclude given file category",
                "alias": [
                    "--Exclude"
                ],
                "value": []
            },
            {
                "id": "ignore",
                "option": "--ignore-case=b",
                "help": "Enable ignore case mode",
                "alias": [
                    "-i"
                ]
            },
            {
                "id": "only",
                "option": "--only=s",
                "help": "Only search given file category",
                "alias": [
                    "-o"
                ],
                "value": []
            },
            {
                "id": "reverse",
                "option": "--/reverse=b",
                "help": "Disable reverse mode",
                "alias": [
                    "-/r"
                ]
            },
            {
                "id": "hidden",
                "option": "--hidden=b",
                "help": "Search hidden file",
                "alias": [
                    "-a"
                ]
            },
            {
                "id": "full",
                "option": "--full=b",
                "help": "Display absolute path of matched file",
                "alias": [
                    "-f"
                ]
            }
        ]
    }
    "#
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

async fn print_help(set: &ASet, finder_set: &ASet) -> color_eyre::Result<()> {
    let foot = format!(
        "Create by {} v{}",
        env!("CARGO_PKG_AUTHORS"),
        env!("CARGO_PKG_VERSION")
    );
    let head = format!("{}", env!("CARGO_PKG_DESCRIPTION"));
    let mut app_help = aopt_help::AppHelp::new(
        "fs",
        &head,
        &foot,
        aopt_help::prelude::Style::default(),
        std::io::stdout(),
    );
    let global = app_help.global_mut();
    let sets = [set, finder_set];

    global.add_block(Block::new("command", "<COMMAND>", "", "COMMAND:", ""))?;
    global.add_block(Block::new("option", "", "", "OPTION:", ""))?;
    global.add_block(Block::new("args", "[ARGS]", "", "ARGS:", ""))?;
    for set in sets {
        for opt in set.iter() {
            if opt.mat_style(Style::Pos) {
                global.add_store(
                    "args",
                    Store::new(
                        Cow::from(opt.name().as_str()),
                        Cow::from(opt.hint().as_str()),
                        Cow::from(opt.help().as_str()),
                        Cow::from(opt.r#type().to_string()),
                        !opt.force(),
                        true,
                    ),
                )?;
            } else if opt.mat_style(Style::Cmd) {
                global.add_store(
                    "command",
                    Store::new(
                        Cow::from(opt.name().as_str()),
                        Cow::from(opt.hint().as_str()),
                        Cow::from(opt.help().as_str()),
                        Cow::from(opt.r#type().to_string()),
                        !opt.force(),
                        true,
                    ),
                )?;
            } else if opt.mat_style(Style::Argument)
                || opt.mat_style(Style::Boolean)
                || opt.mat_style(Style::Combined)
            {
                global.add_store(
                    "option",
                    Store::new(
                        Cow::from(opt.name().as_str()),
                        Cow::from(opt.hint().as_str()),
                        Cow::from(opt.help().as_str()),
                        Cow::from(opt.r#type().to_string()),
                        !opt.force(),
                        false,
                    ),
                )?;
            }
        }
    }
    app_help.display(true)?;

    Ok(())
}

mod opt_serde {
    use std::ops::Deref;
    use std::ops::DerefMut;

    use cote::prelude::aopt::prelude::AFwdPolicy;
    use cote::prelude::MetaConfig;
    use cote::Cote;
    use cote::Error;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Debug, Default, Clone, Deserialize, Serialize)]
    pub struct JsonOpt {
        opts: Vec<MetaConfig<String>>,
    }

    impl Deref for JsonOpt {
        type Target = Vec<MetaConfig<String>>;

        fn deref(&self) -> &Self::Target {
            &self.opts
        }
    }

    impl DerefMut for JsonOpt {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.opts
        }
    }

    impl JsonOpt {
        pub fn add_to(self, cote: &mut Cote<AFwdPolicy>) -> Result<(), Error> {
            for meta in self.opts.into_iter() {
                cote.add_meta(meta)?;
            }
            Ok(())
        }

        pub fn add_cfg(&mut self, mut cfg: MetaConfig<String>) -> &mut Self {
            let opt = self.opts.iter_mut().find(|v| v.id() == cfg.id());

            match opt {
                Some(opt) => {
                    if opt.option().is_empty() {
                        opt.set_option(cfg.take_option());
                    }
                    if opt.hint().is_none() {
                        opt.set_hint(cfg.take_hint());
                    }
                    if opt.help().is_none() {
                        opt.set_help(cfg.take_help());
                    }
                    if opt.action().is_none() {
                        opt.set_action(cfg.take_action());
                    }
                    if opt.assoc().is_none() {
                        opt.set_assoc(cfg.take_assoc());
                    }
                    if opt.alias().is_none() {
                        opt.set_alias(cfg.take_alias());
                    }
                    opt.merge_value(&mut cfg);
                }
                None => {
                    self.opts.push(cfg);
                }
            }
            self
        }
    }
}
