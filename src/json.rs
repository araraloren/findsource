use std::ops::Deref;
use std::ops::DerefMut;

use cote::aopt::opt::OptConfig;
use cote::aopt::prelude::AFwdParser;
use cote::prelude::*;
use cote::Error;
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
    pub fn add_to(self, parser: &mut AFwdParser) -> Result<(), Error> {
        for meta in self.opts.into_iter() {
            let cfg: OptConfig = meta.build(parser.optset_mut())?;
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
