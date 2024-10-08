mod config;
mod finder;
mod json;
mod r#macro;

use color_eyre::Result;
use cote::aopt;
use cote::aopt::prelude::AFwdParser;
use cote::aopt::prelude::APreParser;
use cote::aopt::shell::CompleteService;
use cote::aopt::shell::Shell;
use cote::aopt::HashMap;
use cote::aopt_help;
use cote::prelude::*;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Receiver;

use config::default_json_configuration;
use config::get_configuration_directories;
use config::try_to_load_configuration2;
use finder::Finder;
use json::JsonOpt;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    if let Some((cl, shell)) = aopt::shell::try_get_complete()? {
        let args = cl.split(' ').collect::<Vec<&str>>();
        let args = Args::from(args.into_iter());
        let cli = Cli::new(args, false).await?;

        cli.try_auto_complete(shell).await?;
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
    loader: APreParser<'a>,

    finder: AFwdParser<'a>,

    args: Args,

    pre_load: HashMap<String, String>,
}

impl<'a> Cli<'a> {
    pub async fn new(args: Args, allow_debug: bool) -> Result<Cli<'a>> {
        let config_dir = get_configuration_directories();
        let mut loader = APreParser::default();

        loader.add_opt("-d;--debug=b: Print debug message")?;
        loader.add_opt("-?;--help=b: Print help message")?;
        loader.add_opt("-v;--verbose=b: Print more debug message")?;
        loader
            .add_opt("-l;--load=s: Load option setting from configuration name or file")?
            .set_hint("-l,--load CFG|PATH")
            .set_values_t(Vec::<JsonOpt>::new())
            .on(move |set: &mut ASet, _: &mut ASer, ctx: &Ctx| {
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
            .on(move |_: &mut ASet, _: &mut ASer, ctx: &Ctx| {
                let path = ctx.value::<PathBuf>()?;

                if debug {
                    eprintln!("INFO: ... prepare searching path: {:?}", path);
                }
                if !path.is_file() && !path.is_dir() {
                    Err(aopt::raise_error!(
                        "{:?} is not a valid path!",
                        path.as_path()
                    ))
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

    pub async fn try_auto_complete(mut self, shell: Shell) -> Result<()> {
        let mut service = CompleteService::default();

        if let Some(options) = self.loader.take_options() {
            for opt in options {
                self.finder.optset_mut().insert(opt);
            }
        }
        service.parse_with(self.args, self.finder.optset_mut())?;
        service.write_complete_to(self.finder.optset(), &mut std::io::stdout(), shell)?;
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

async fn print_help(set: &ASet, finder_set: &ASet) -> color_eyre::Result<()> {
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
