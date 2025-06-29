use std::ops::Deref;
use std::ops::DerefMut;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct JsonOptCollection {
    pub opts: Vec<JsonConfig>,
}

impl Deref for JsonOptCollection {
    type Target = Vec<JsonConfig>;
    fn deref(&self) -> &Self::Target {
        &self.opts
    }
}

impl DerefMut for JsonOptCollection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.opts
    }
}

impl JsonOptCollection {
    pub fn append_opts(self, parser: &mut AFwdParser) -> Result<(), Error> {
        for meta in self.opts.into_iter() {
            let cfg: OptConfig = meta.build(parser.optset_mut())?;
            parser.add_opt_cfg(cfg)?;
        }
        Ok(())
    }

    pub fn add_json_config(&mut self, mut cfg: JsonConfig) -> &mut Self {
        let config = self.opts.iter_mut().find(|v| v.id == cfg.id);

        match config {
            Some(config) => {
                if config.option.is_empty() {
                    config.set_option(cfg.take_option());
                }
                if config.hint.is_none() {
                    config.set_hint(cfg.take_hint());
                }
                if config.help.is_none() {
                    config.set_help(cfg.take_help());
                }
                if config.action.is_none() {
                    config.set_action(cfg.take_action());
                }
                if config.alias.is_none() {
                    config.set_alias(cfg.take_alias());
                }
                config.merge_value(&mut cfg);
            }
            None => {
                self.opts.push(cfg);
            }
        }
        self
    }
}

use aopt::prelude::*;
use aopt::value::Placeholder;
use aopt::Error;

/// Hold the option information from configuration files.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonConfig {
    pub id: String,

    pub option: String,

    pub hint: Option<String>,

    pub help: Option<String>,

    pub action: Option<Action>,

    pub alias: Option<Vec<String>>,

    pub value: Option<Vec<String>>,
}

impl JsonConfig {
    pub fn take_option(&mut self) -> String {
        std::mem::take(&mut self.option)
    }

    pub fn take_hint(&mut self) -> Option<String> {
        self.hint.take()
    }

    pub fn take_help(&mut self) -> Option<String> {
        self.help.take()
    }

    pub fn take_action(&mut self) -> Option<Action> {
        self.action.take()
    }

    pub fn take_alias(&mut self) -> Option<Vec<String>> {
        self.alias.take()
    }

    pub fn take_value(&mut self) -> Option<Vec<String>> {
        self.value.take()
    }

    pub fn set_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = id.into();
        self
    }

    pub fn set_option(&mut self, option: impl Into<String>) -> &mut Self {
        self.option = option.into();
        self
    }

    pub fn set_hint(&mut self, hint: Option<impl Into<String>>) -> &mut Self {
        self.hint = hint.map(|v| v.into());
        self
    }

    pub fn set_help(&mut self, help: Option<impl Into<String>>) -> &mut Self {
        self.help = help.map(|v| v.into());
        self
    }

    pub fn set_action(&mut self, action: Option<Action>) -> &mut Self {
        self.action = action;
        self
    }

    pub fn set_alias(&mut self, alias: Option<Vec<impl Into<String>>>) -> &mut Self {
        self.alias = alias.map(|alias| alias.into_iter().map(|v| v.into()).collect());
        self
    }

    pub fn set_value(&mut self, value: Option<Vec<String>>) -> &mut Self {
        self.value = value;
        self
    }

    pub fn merge_value(&mut self, other: &mut Self) -> &mut Self {
        match self.value.as_mut() {
            Some(value) => {
                if let Some(other_value) = other.value.as_mut() {
                    value.append(other_value);
                }
            }
            None => {
                self.value = std::mem::take(&mut other.value);
            }
        }
        self
    }
}

impl<C> ConfigBuild<C> for JsonConfig
where
    C: ConfigValue + Default,
{
    type Val = Placeholder;

    fn build<P>(mut self, parser: &P) -> Result<C, Error>
    where
        P: OptParser,
        P::Output: Information,
    {
        let mut cfg: C = self.take_option().build(parser)?;

        if let Some(hint) = self.take_hint() {
            cfg.set_hint(hint);
        }
        if let Some(help) = self.take_help() {
            cfg.set_help(help);
        }
        if let Some(action) = self.take_action() {
            cfg.set_action(action);
        }
        if let Some(values) = self.take_value() {
            cfg.set_initializer(ValInitializer::new_values(values));
        }
        if let Some(alias) = self.take_alias() {
            cfg.set_alias(alias);
        }
        Ok(cfg)
    }
}
