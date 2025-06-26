mod config;
mod finder;
mod json;
mod r#macro;

use std::borrow::Cow;
use std::fs::read_dir;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::Result;
use cote::aopt;
use cote::aopt::prelude::*;
use cote::aopt::shell::CompletionManager;
use cote::aopt::HashMap;
use cote::aopt_help;
use cote::shell::shell::Complete;
use cote::shell::value::once_values;
use cote::shell::value::Values;
use cote::shell::CompleteCli;

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Receiver;

use config::default_json_configuration;
use config::get_configuration_directories;
use config::try_to_load_configuration2;
use finder::Finder;
use json::JsonOpt;

pub const BIN: &str = "fs";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    if let Ok(cc) = aopt::shell::get_complete_cli() {
        if cc.write_stdout(BIN, BIN).is_err() {
            let cli = Cli::new(Args::from(&cc.args), false).await?;

            cli.try_auto_complete(cc).await?;
        }
    } else {
        let cli = Cli::new(Args::from_env(), true).await?;

        if let Some((paths, finder, mut rx)) = cli.into_finder().await? {
            if finder.is_empty() {
                say!("What extension or filename do you want search, try command: fs -? or fs --help",);
                return Ok(());
            }
            let debug = finder.debug;
            let finder = Arc::new(finder);

            for path in paths {
                let inner_finder = Arc::clone(&finder);

                tokio::spawn(start_worker!(
                    inner_finder,
                    path,
                    Finder::find_in_directory_first,
                    "ERROR: Can not find file in directory `{:?}`: {:?}"
                ));
            }
            drop(finder);
            while let Some(file) = rx.recv().await {
                say!("{}", file);
            }
            if debug {
                note!("INFO: ... Searching end");
            }
        }
    }
    Ok(())
}

struct Cli<'a> {
    loader: AFwdParser<'a>,

    finder: AFwdParser<'a>,

    args: Args,

    pre_load: HashMap<String, String>,
}

impl<'a> Cli<'a> {
    pub async fn new(args: Args, allow_debug: bool) -> Result<Cli<'a>> {
        let config_dir = get_configuration_directories();
        let mut loader = AFwdParser::default();

        loader.set_prepolicy(true);
        loader.add_opt("-d;--debug=b: Print debug message")?;
        loader.add_opt("-?;--help=b: Print help message")?;
        loader.add_opt("-v;--verbose=b: Print more debug message")?;
        loader
            .add_opt("-l;--load=s: Load option setting from configuration name or file")?
            .set_hint("-l,--load CFG|PATH")
            .set_values_t(Vec::<JsonOpt>::new())
            .on(move |set, ctx| {
                let cfg = ctx.value::<String>()?;
                let ret = try_to_load_configuration2(&config_dir, &cfg);

                if allow_debug {
                    let (path, config) = ret?;

                    if *set.find_val::<bool>("--debug")? {
                        eprintln!("INFO: ... loading config {:?} --> {:?}", &path, &config);
                    }
                    Ok(Some(config))
                } else {
                    Ok(ret.map(|(_, config)| config).ok())
                }
            })?;

        // load config name to loader
        let mut ret = loader.parse(args)?;
        let mut debug = *loader.find_val("--debug")?;
        let mut finder = AFwdParser::default();
        let load_jsons = loader.take_vals::<JsonOpt>("--load").unwrap_or_default();

        if !allow_debug {
            debug = false;
        }
        finder
            .add_opt("path=p@1..: Path need to be search")?
            .set_force(true)
            .set_hint("[PATH]+")
            .on(move |_, ctx| {
                let path = ctx.value::<PathBuf>()?;

                if debug {
                    eprintln!("INFO: ... prepare searching path: {:?}", path);
                }
                if !path.is_file() && !path.is_dir() {
                    Err(aopt::error!("{:?} is not a valid path!", path.as_path()))
                } else {
                    Ok(Some(path))
                }
            })?;
        let mut jsonopts: JsonOpt = serde_json::from_str(default_json_configuration()).unwrap();
        let mut pre_loads = HashMap::<String, String>::default();

        // merge the json configurations
        load_jsons.into_iter().for_each(|json| {
            for cfg in json.opts {
                if !pre_loads.contains_key(cfg.id()) {
                    pre_loads.insert(cfg.id().clone(), cfg.option().clone());
                }
                jsonopts.add_cfg(cfg);
            }
        });
        if debug {
            note!(
                "INFO: ... loading cfg: {}",
                serde_json::to_string_pretty(&jsonopts)?
            );
            note!("INFO: ... loading options: {:?}", pre_loads);
        }
        // add the option to finder
        jsonopts.add_to(&mut finder)?;

        Ok(Self {
            loader,
            finder,
            args: Args::from(ret.take_args()),
            pre_load: pre_loads,
        })
    }

    pub fn list_configurations<O>() -> impl Values<O, Err = cote::Error> {
        once_values(move |_| {
            let mut cfgs = vec![];

            for dir in get_configuration_directories()
                .into_iter()
                .flatten()
                .filter(|v| v.exists() && v.is_dir())
            {
                if let Ok(entrys) = read_dir(&dir) {
                    for entry in entrys {
                        if let Ok(path) = entry.map(|v| v.path()) {
                            if path.is_file()
                                && Some("json") == path.extension().and_then(|v| v.to_str())
                            {
                                if let Some(filename) = path.with_extension("").file_name() {
                                    cfgs.push(filename.to_os_string());
                                }
                            }
                        }
                    }
                }
            }

            Ok(cfgs)
        })
    }

    pub async fn try_auto_complete(mut self, cli: CompleteCli) -> Result<()> {
        if let Some(options) = self.loader.take_options() {
            for opt in options {
                self.finder.optset_mut().insert(opt);
            }
        }
        cli.complete(|shell| {
            let mut ctx = cli.get_context()?;
            let mut manager = CompletionManager::new(self.finder.optset);
            let cfg_uid = manager.optset().find_uid("-l")?;

            shell.set_buff(std::io::stdout());
            manager.set_values(cfg_uid, Self::list_configurations());
            manager.complete(shell, &mut ctx)?;
            Ok(())
        })?;
        Ok(())
    }

    pub async fn into_finder(self) -> Result<Option<(Vec<PathBuf>, Finder, Receiver<String>)>> {
        let mut loader = self.loader;
        let mut finder = self.finder;
        let pre_load = self.pre_load;
        let help = loader.take_val("--help")?;
        let debug = loader.take_val("--debug")?;
        let verbose = loader.take_val("--verbose")?;

        if help {
            if debug {
                note!("INFO: Request display help message: {}", help);
            }
            print_help(loader.optset(), finder.optset()).await?;
            return Ok(None);
        }
        // initialize the option value
        let mut ret = finder.parse(self.args)?;

        if !ret.status() {
            if debug {
                note!(
                    "INFO: Display the help message caused by error: {:?}",
                    ret.failure()
                );
            }
            print_help(loader.optset(), finder.optset()).await?;
            return Err(ret.take_failure().unwrap())?;
        }
        if debug {
            note!("INFO: ... Starting search thread ...");
        }
        let mut paths = finder.take_vals("path").unwrap_or_default();

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
            return Ok(None);
        }
        if debug {
            note!("INFO: ... Got search path: {:?}", paths);
        }
        let (tx, rx) = channel(512);
        let finder = Finder::new(pre_load, finder, debug, verbose, tx).await?;

        Ok(Some((paths, finder, rx)))
    }
}

async fn print_help<'a>(set: &AHCSet<'a>, finder_set: &AHCSet<'a>) -> color_eyre::Result<()> {
    use aopt_help::block::Block;
    use aopt_help::store::Store;

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
                        Cow::from(opt.name()),
                        Cow::from(opt.hint()),
                        Cow::from(opt.help()),
                        Cow::default(),
                        !opt.force(),
                        true,
                    ),
                )?;
            } else if opt.mat_style(Style::Cmd) {
                global.add_store(
                    "command",
                    Store::new(
                        Cow::from(opt.name()),
                        Cow::from(opt.hint()),
                        Cow::from(opt.help()),
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
                        Cow::from(opt.name()),
                        Cow::from(opt.hint()),
                        Cow::from(opt.help()),
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
