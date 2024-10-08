use cote::aopt::prelude::AFwdParser;
use cote::aopt::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::read_dir;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender;

use cote::prelude::*;

use crate::note;
use crate::start_worker;

pub struct Finder {
    full: bool,

    pub(crate) debug: bool,

    verb: bool,

    hidden: bool,

    reverse: bool,

    igcase: bool,

    invert: bool,

    whos: HashSet<String>,

    exts: HashSet<String>,

    sender: Sender<String>,
}

impl Finder {
    pub async fn new(
        opts: HashMap<String, String>,
        parser: AFwdParser<'_>,
        debug: bool,
        verb: bool,
        sender: Sender<String>,
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
        let invert = *parser.find_val("--invert")?;

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
        for (id, opt) in opts {
            if only_checker(id.as_str(), "") && !exclude_checker(id.as_str(), "") {
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
            verb,
            hidden,
            reverse,
            igcase,
            invert,
            whos,
            exts,
            sender,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.whos.is_empty() && self.exts.is_empty()
    }

    pub async fn find_in_directory_first(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.find_in_directory_impl(path, true).await
    }

    pub async fn find_in_directory_left(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.find_in_directory_impl(path, false).await
    }

    pub async fn find_in_directory_impl(
        self: Arc<Self>,
        path: PathBuf,
        first: bool,
    ) -> color_eyre::Result<()> {
        let debug = self.debug;
        let verbose = self.verb;
        let reverse = self.reverse;

        if debug && verbose {
            note!("INFO: search file in path: {:?}", path);
        }
        let meta = tokio::fs::metadata(&path).await?;

        if reverse && meta.is_dir() {
            if first {
                tokio::spawn(start_worker!(
                    self,
                    path,
                    Self::process_directory_frist,
                    "ERROR: Can not access directory `{:?}`: {:?}"
                ));
            } else {
                tokio::spawn(start_worker!(
                    self,
                    path,
                    Self::process_directory_left,
                    "ERROR: Can not access directory `{:?}`: {:?}"
                ));
            }
        } else if meta.is_file() {
            if let Err(e) = self.process_file(path.clone()).await {
                note!("ERROR: Can not access file `{:?}`: {:?}", path, e);
            }
        } else if debug {
            note!("WARN: {:?} is not a valid file", path);
        }
        Ok(())
    }

    #[async_recursion::async_recursion]
    pub async fn process_directory_frist(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.process_directory_impl(path, true).await
    }

    #[async_recursion::async_recursion]
    pub async fn process_directory_left(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        self.process_directory_impl(path, false).await
    }

    #[async_recursion::async_recursion]
    pub async fn process_directory_impl(
        self: Arc<Self>,
        path: PathBuf,
        first: bool,
    ) -> color_eyre::Result<()> {
        let debug = self.debug;
        let verbose = self.verb;
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

    pub async fn process_file(self: Arc<Self>, path: PathBuf) -> color_eyre::Result<()> {
        let debug = self.debug;
        let hidden = self.hidden;
        let full = self.full;
        let igcase = self.igcase;
        let invert = self.invert;

        let may_full_path = if full {
            dunce::canonicalize(&path)?
        } else {
            path.clone()
        };

        if !is_file_hidden(&path).await? || hidden {
            if let Some(path_str) = may_full_path.to_str() {
                if let Some(Some(file_name)) = path.file_name().map(|v| v.to_str()) {
                    let lower_case = file_name.to_lowercase();
                    let lower_case = lower_case.as_ref();
                    let matched = checking_ext(file_name, &self.whos, &self.exts).await
                        || (igcase && checking_ext(lower_case, &self.whos, &self.exts).await);

                    if debug {
                        note!("INFO: checking file {}", path_str);
                    }
                    if matched || invert {
                        self.sender.send(path_str.to_owned()).await?;
                    }
                }
            }
        } else if debug {
            note!("INFO: ignore directory {:?}", path);
        }
        Ok(())
    }
}

pub async fn checking_ext(path: &str, whos: &HashSet<String>, exts: &HashSet<String>) -> bool {
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
