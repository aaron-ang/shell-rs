use crate::completion::Completions;
use crate::history::History;
use crate::job::Jobs;
use crate::variable::Variables;

#[derive(Clone)]
pub struct Shell {
    pub history: History,
    pub jobs: Jobs,
    pub completions: Completions,
    pub variables: Variables,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            history: History::open(),
            jobs: Jobs::new(),
            completions: Completions::new(),
            variables: Variables::new(),
        }
    }
}
