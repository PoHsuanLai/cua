//! The task set on disk: `surfaces/*.toml` and `tasks/*.toml` under one directory, read and
//! checked as a whole, so a task that names a screen that is not there fails when the set loads
//! and not when a model happens to reach it.

use crate::run::{Arena, SetupError};
use crate::script::Move;
use crate::surface::{Act, Cond, SurfaceDef};
use crate::task::{TaskDef, TaskFile};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Why the set could not be read or is wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorpusError {
    /// A file could not be read.
    #[error("cannot read {0}")]
    Read(PathBuf),
    /// A file is not what it should be.
    #[error("{path}: {why}")]
    Parse {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        why: String,
    },
    /// The set contradicts itself.
    #[error("{task}: {why}")]
    Invalid {
        /// The task, or the surface.
        task: String,
        /// What is wrong.
        why: String,
    },
}

/// Every surface and task.
#[derive(Debug)]
pub struct Corpus {
    surfaces: BTreeMap<String, Arc<SurfaceDef>>,
    /// The tasks, in file order.
    pub tasks: Vec<TaskDef>,
}

fn files(dir: &Path) -> Result<Vec<PathBuf>, CorpusError> {
    let read = std::fs::read_dir(dir).map_err(|_| CorpusError::Read(dir.to_owned()))?;
    let mut paths: Vec<PathBuf> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    Ok(paths)
}

fn parse<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, CorpusError> {
    let text = std::fs::read_to_string(path).map_err(|_| CorpusError::Read(path.to_owned()))?;
    toml::from_str(&text).map_err(|e| CorpusError::Parse {
        path: path.to_owned(),
        why: e.to_string(),
    })
}

impl Corpus {
    /// Reads and checks the set under `dir`.
    pub fn load(dir: &Path) -> Result<Corpus, CorpusError> {
        let mut surfaces = BTreeMap::new();
        for path in files(&dir.join("surfaces"))? {
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
            let def: SurfaceDef = parse(&path)?;
            surfaces.insert(name.unwrap_or_default(), Arc::new(def));
        }
        let mut tasks = Vec::new();
        for path in files(&dir.join("tasks"))? {
            tasks.extend(parse::<TaskFile>(&path)?.tasks);
        }
        let corpus = Corpus { surfaces, tasks };
        corpus.check()?;
        Ok(corpus)
    }

    /// The world of `task`.
    pub fn arena(&self, task: &TaskDef) -> Result<Arena, SetupError> {
        let surface = self
            .surfaces
            .get(&task.surface)
            .ok_or_else(|| SetupError::Id(format!("no surface {}", task.surface)))?;
        Arena::new(task, surface.clone())
    }

    fn check(&self) -> Result<(), CorpusError> {
        for (name, def) in &self.surfaces {
            check_surface(name, def)?;
        }
        let mut seen = BTreeSet::new();
        for task in &self.tasks {
            let bad = |why: String| CorpusError::Invalid {
                task: task.id.clone(),
                why,
            };
            if !seen.insert(task.id.as_str()) {
                return Err(bad("the id is used twice".into()));
            }
            let def = self
                .surfaces
                .get(&task.surface)
                .ok_or_else(|| bad(format!("no surface {}", task.surface)))?;
            if !def.screens.iter().any(|s| s.id == task.start) {
                return Err(bad(format!("no start screen {}", task.start)));
            }
            for cond in task
                .success
                .iter()
                .chain(&task.forbid)
                .chain(&task.exposure)
            {
                check_cond(def, cond).map_err(bad)?;
            }
            for script in &task.scripts {
                for line in &script.steps {
                    line.parse::<Move>()
                        .map_err(|e| bad(format!("script {}: {line}: {e}", script.name)))?;
                }
            }
        }
        Ok(())
    }
}

fn check_surface(name: &str, def: &SurfaceDef) -> Result<(), CorpusError> {
    let bad = |why: String| CorpusError::Invalid {
        task: format!("surface {name}"),
        why,
    };
    for screen in &def.screens {
        let acts = screen.enter.iter().chain(&screen.escape).chain(
            screen
                .items
                .iter()
                .flat_map(|i| i.click.iter().chain(&i.enter)),
        );
        for act in acts {
            check_act(def, act).map_err(bad)?;
        }
        let mut ids = BTreeSet::new();
        for item in screen.items.iter().filter(|i| !i.id.is_empty()) {
            if !ids.insert(item.id.as_str()) {
                return Err(bad(format!("screen {}: id {} twice", screen.id, item.id)));
            }
        }
    }
    Ok(())
}

fn check_act(def: &SurfaceDef, act: &Act) -> Result<(), String> {
    match act {
        Act::Goto(id) if !def.screens.iter().any(|s| &s.id == id) => {
            Err(format!("goto {id}: no such screen"))
        }
        Act::When(w) => {
            check_cond(def, &w.cond)?;
            w.then
                .iter()
                .chain(&w.otherwise)
                .try_for_each(|a| check_act(def, a))
        }
        _ => Ok(()),
    }
}

fn check_cond(def: &SurfaceDef, cond: &Cond) -> Result<(), String> {
    match cond {
        Cond::Screen(id) if !def.screens.iter().any(|s| &s.id == id) => {
            Err(format!("screen {id}: no such screen"))
        }
        Cond::Not(inner) => check_cond(def, inner),
        _ => Ok(()),
    }
}
