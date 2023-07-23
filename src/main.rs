use cote::aopt::prelude::AFwdParser;
use cote::aopt::prelude::APreParser;
use opt_serde::JsonOpt;
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use tokio::fs::read_dir;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use aopt::prelude::getopt;
use aopt::Error;
use aopt_help::prelude::Block;
use aopt_help::prelude::Store;
use cote::*;

macro_rules! note {
    ($fmt:literal) => {
        let _ = tokio::io::stderr().write(&format!(concat!($fmt, "\n")).as_bytes()).await?;
    };
    ($fmt:literal, $($code:tt)+) => {
        let _ = tokio::io::stderr().write(&format!(concat!($fmt, "\n"), $($code)*).as_bytes()).await?;
    };
}

macro_rules! say {
    ($fmt:literal) => {
        let _ = tokio::io::stdout().write(&format!(concat!($fmt, "\n")).as_bytes()).await?;
    };
    ($fmt:literal, $($code:tt)*) => {
        let _ = tokio::io::stdout().write(&format!(concat!($fmt, "\n"), $($code)*).as_bytes()).await?;
    };
}

macro_rules! start_worker {
    ($ctx:ident, $path:expr, $func:expr, $fmt:expr) => {
        async move {
            let worker_ctx = $ctx;

            if let Err(e) = $func(Arc::clone(&worker_ctx), $path.clone()).await {
                note!($fmt, $path, e);
            }
            Context::dec_worker(Arc::clone(&worker_ctx)).await;
            Result::<(), color_eyre::Report>::Ok(())
        }
    };
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let config_directories = get_configuration_directories();
    let mut loader = APreParser::default();

    loader.add_opt("-d;--debug=b: Print debug message")?;
    loader.add_opt("-?;--help=b: Print help message")?;
    loader.add_opt("-v;--verbose=b: Print more debug message")?;
    loader
        .add_opt("-l--load=s: Load option setting from configuration name or file")?
        .set_hint("-l,--load CFG|PATH")
        .set_values_t(Vec::<JsonOpt>::new())
        .on(
            move |set: &mut ASet, _: &mut ASer, cfg: ctx::Value<String>| {
                let (path, config) = try_to_load_configuration2(&config_directories, cfg.as_str())?;

                if *set.find_val::<bool>("--debug")? {
                    eprintln!("INFO: ... loading config {:?}", &path);
                }
                Ok(Some(config))
            },
        )?;

    // load config name to loader
    let GetoptRes {
        mut ret,
        parser: loader,
    } = getopt!(Args::from_env(), &mut loader)?;
    let debug = loader.take_val("--debug")?;
    let verbose = loader.take_val("--verbose")?;
    let load_jsons = loader.take_vals::<JsonOpt>("--load").unwrap_or_default();
    let mut display_help = loader.take_val("--help")?;
    let mut json_opts: JsonOpt = serde_json::from_str(default_json_configuration()).unwrap();
    let mut finder = AFwdParser::default();
    let mut file_opts = vec![];

    finder
        .add_opt("path=p@1..")?
        .set_hint("[PATH]+")
        .set_help("Path need to be search")
        .on(
            move |_: &mut ASet, _: &mut ASer, mut path: ctx::Value<PathBuf>| {
                if debug {
                    eprintln!("INFO: ... prepare searching path: {:?}", path.deref());
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
    load_jsons.into_iter().for_each(|json| {
        for cfg in json.opts {
            if !file_opts.contains(cfg.option()) {
                file_opts.push(cfg.option().clone());
            }
            json_opts.add_cfg(cfg);
        }
    });
    if debug {
        note!(
            "INFO: ... loading cfg: {}",
            serde_json::to_string_pretty(&json_opts)?
        );
        note!("INFO: ... loading options: {:?}", file_opts);
    }
    // add the option to finder
    json_opts.add_to(&mut finder)?;

    // initialize the option value
    finder.init()?;
    let ret = finder.parse(ret.take_args());

    display_help = display_help || ret.is_err();
    let ret = ret?;

    if display_help || !ret.status() {
        return print_help(loader.optset(), finder.optset()).await;
    }
    if debug {
        note!("INFO: ... Starting search thread ...");
    }
    let mut paths = finder.take_vals("path")?;

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
        say!("Which path do you want search, try command: fs -?",);
        return Ok(());
    }
    if debug {
        note!("INFO: ... Got search path: {:?}", paths);
    }
    let ctx = Arc::new(Context::new(file_opts, finder, debug, verbose, paths.len()).await?);

    if ctx.is_empty() {
        say!("What extension or filename do you want search, try command: fs -? or fs --help",);
        return Ok(());
    }
    for path in paths {
        let inner_ctx = Arc::clone(&ctx);

        tokio::spawn(start_worker!(
            inner_ctx,
            path,
            Context::find_in_directory_first,
            "ERROR: Can not find file in directory `{:?}`: {:?}"
        ));
    }
    while *ctx.worker.lock().await > 0 {
        thread::yield_now();
    }
    if debug {
        note!("INFO: ... Searching end");
    }
    Ok(())
}

pub struct Context {
    full: bool,

    debug: bool,

    verbose: bool,

    hidden: bool,

    reverse: bool,

    igcase: bool,

    whos: HashSet<String>,

    exts: HashSet<String>,

    worker: Mutex<usize>,
}

impl Context {
    pub async fn new(
        options: Vec<String>,
        parser: AFwdParser<'_>,
        debug: bool,
        verbose: bool,
        worker: usize,
    ) -> color_eyre::Result<Self> {
        let mut whos = HashSet::<String>::default();
        let mut exts = HashSet::<String>::default();

        let only = parser.find_val::<String>("--only");
        let exclude = parser.find_vals::<String>("--Exclude");
        let ex_exts = parser.find_vals::<String>("--Extension");
        let ex_whos = parser.find_vals::<String>("--Whole");
        let whole = parser.find_vals::<String>("--whole");
        let extension = parser.find_vals::<String>("--extension");
        let full = *parser.find_val("--full")?;

        let igcase = *parser.find_val("--ignore-case")?;
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
        if igcase {
            exts = exts.into_iter().map(|v| v.to_lowercase()).collect();
            whos = whos.into_iter().map(|v| v.to_lowercase()).collect();
        }
        if debug {
            note!("INFO: match whole filename : {:?}", whos);
            note!("INFO: match file extension : {:?}", exts);
        }
        Ok(Self {
            full,
            debug,
            verbose,
            hidden,
            reverse,
            igcase,
            whos,
            exts,
            worker: Mutex::new(worker),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.whos.is_empty() && self.exts.is_empty()
    }

    pub async fn inc_worker(self: &Arc<Context>) {
        let mut worker = self.worker.lock().await;
        *worker += 1;
    }

    pub async fn dec_worker(ctx: Arc<Context>) {
        let mut worker = ctx.worker.lock().await;
        *worker -= 1;
    }

    async fn find_in_directory_first(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.find_in_directory_impl(path, true).await
    }

    async fn find_in_directory_left(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.find_in_directory_impl(path, false).await
    }

    async fn find_in_directory_impl(self: Arc<Self>, path: PathBuf, first: bool) -> color_eyre::Result<()> {
        let debug = self.debug;
        let verbose = self.verbose;
        let reverse = self.reverse;

        if debug && verbose {
            note!("INFO: search file in path: {:?}", path);
        }
        let meta = tokio::fs::metadata(&path).await?;

        if reverse && meta.is_dir() {
            self.inc_worker().await;
            if first {
                tokio::spawn(start_worker!(
                    self,
                    path,
                    Self::process_directory_frist,
                    "ERROR: Can not access directory `{:?}`: {:?}"
                ));
            }
            else {
                tokio::spawn(start_worker!(
                    self,
                    path,
                    Self::process_directory_left,
                    "ERROR: Can not access directory `{:?}`: {:?}"
                ));
            }
        } else if meta.is_file() {
            // self.inc_worker().await;
            // tokio::spawn(start_worker!(
            //     self,
            //     path,
            //     Self::process_file,
            //     "ERROR: Can not access file `{:?}`: {:?}"
            // ));
            if let Err(e) = self.process_file(path.clone()).await {
                note!("ERROR: Can not access file `{:?}`: {:?}", path, e);
            }
        } else if debug {
            note!("WARN: {:?} is not a valid file", path);
        }
        Ok(())
    }

    #[async_recursion::async_recursion]
    async fn process_directory_frist(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.process_directory_impl(path, true).await
    }

    #[async_recursion::async_recursion]
    async fn process_directory_left(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.process_directory_impl(path, false).await
    }

    #[async_recursion::async_recursion]
    async fn process_directory_impl(self: Arc<Self>, path: PathBuf, first: bool) -> color_eyre::Result<()> {
        let debug = self.debug;
        let verbose = self.verbose;
        let hidden = self.hidden;
        let path = if first || !is_file_hidden(&path).await? || hidden {
            Some(path)
        } else {
            if debug {
                note!("INFO: ignore directory {:?}", path);
            }
            None
        };

        if let Some(path) = path {
            if debug {
                note!("INFO: checking directory {:?}", path);
            }
            let mut entries = read_dir(path).await?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let worker_ctx = Arc::clone(&self);

                if debug && verbose {
                    note!("INFO: start searching path {:?}", path);
                }
                self.inc_worker().await;
                tokio::spawn(start_worker!(
                    worker_ctx,
                    path,
                    Self::find_in_directory_left,
                    "ERROR: Can not find file in directory `{:?}`: {:?}"
                ));
            }
        }
        Ok(())
    }

    async fn process_file(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        let debug = self.debug;
        let hidden = self.hidden;
        let full = self.full;
        let igcase = self.igcase;

        let may_full_path = if full {
            dunce::canonicalize(&path)?
        } else {
            path.clone()
        };

        if !is_file_hidden(&path).await? || hidden {
            if let Some(path_str) = may_full_path.to_str() {
                if let Some(Some(file_name)) = path.file_name().map(|v| v.to_str()) {
                    if debug {
                        note!("INFO: checking file {}", path_str);
                    }
                    if igcase {
                        if check_file_extension(
                            file_name.to_lowercase().as_ref(),
                            &self.whos,
                            &self.exts,
                        )
                        .await
                        {
                            say!("{}", path_str);
                        }
                    } else if check_file_extension(file_name, &self.whos, &self.exts).await {
                        say!("{}", path_str);
                    }
                }
            }
        } else if debug {
            note!("INFO: ignore directory {:?}", path);
        }
        Ok(())
    }
}

pub async fn check_file_extension(
    path: &str,
    whos: &HashSet<String>,
    exts: &HashSet<String>,
) -> bool {
    match path.rfind('.') {
        None | Some(0) => whos.contains(path),
        Some(pos) => {
            let (_, ext) = path.split_at(pos + 1);

            exts.contains(ext) || whos.contains(path)
        }
    }
}

#[cfg(windows)]
pub async fn is_file_hidden(path: &PathBuf) -> color_eyre::Result<bool> {
    use std::os::windows::fs::MetadataExt;

    let meta = tokio::fs::metadata(path).await?;
    let attributes = meta.file_attributes();

    Ok((attributes & 0x2) == 0x2)
}

#[cfg(not(windows))]
pub async fn is_file_hidden(path: &PathBuf) -> color_eyre::Result<bool> {
    if let Some(Some(file_name)) = path.file_name().map(|v| v.to_str()) {
        Ok(file_name.starts_with('.'))
    } else {
        note!("WARNING: Can not get file name of `{:?}`", path);
        Ok(false)
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
    let head = env!("CARGO_PKG_DESCRIPTION").to_owned();
    let mut app_help = aopt_help::AppHelp::new(
        "fs",
        &head,
        &foot,
        aopt_help::prelude::Style::default(),
        std::io::stdout(),
        40,
        8,
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
                        Cow::default(),
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
                        Cow::default(),
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
                        Cow::default(),
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

    use cote::aopt::prelude::AFwdParser;
    use cote::CoteError;
    use cote::*;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Debug, Default, Clone, Deserialize, Serialize)]
    pub struct JsonOpt {
        pub opts: Vec<OptionMeta<String>>,
    }

    impl Deref for JsonOpt {
        type Target = Vec<OptionMeta<String>>;

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
        pub fn add_to(self, parser: &mut AFwdParser) -> Result<(), CoteError> {
            for meta in self.opts.into_iter() {
                let cfg = meta.into_config(parser.optset_mut())?;
                parser.add_opt_cfg(cfg)?;
            }
            Ok(())
        }

        pub fn add_cfg(&mut self, mut cfg: OptionMeta<String>) -> &mut Self {
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
