use std::{
    cell::RefCell,
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
    rc::Rc,
};

use anyhow::Result;

const HISTFILE_ENV: &str = "HISTFILE";

#[derive(Clone)]
pub struct History {
    inner: Rc<RefCell<HistoryData>>,
}

struct HistoryData {
    entries: Vec<String>,
    append_index: usize,
}

impl History {
    pub fn open() -> Self {
        let histfile = env::var(HISTFILE_ENV).unwrap_or_default();
        let entries = fs::read_to_string(histfile)
            .map(|s| s.lines().map(String::from).collect::<Vec<_>>())
            .unwrap_or_default();
        let len = entries.len();
        let data = HistoryData {
            entries,
            append_index: len,
        };
        History {
            inner: Rc::new(RefCell::new(data)),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.borrow().entries.len()
    }

    pub fn add(&mut self, command: String) {
        if command.is_empty() {
            return;
        }
        self.inner.borrow_mut().entries.push(command);
    }

    pub fn get(&self, index: usize) -> Option<String> {
        self.inner.borrow().entries.get(index).cloned()
    }

    pub fn set(&mut self, index: usize, command: String) {
        let mut data = self.inner.borrow_mut();
        if index < data.entries.len() {
            data.entries[index] = command;
        }
    }

    pub fn clear(&mut self) {
        self.inner.borrow_mut().entries.clear();
    }

    pub fn print<W: Write>(&self, writer: &mut W, limit: Option<usize>) -> Result<()> {
        let data = self.inner.borrow();
        let limit = limit.unwrap_or(data.entries.len());
        let start = data.entries.len().saturating_sub(limit);
        for (i, cmd) in data.entries.iter().skip(start).enumerate() {
            writeln!(writer, "{:5} {}", start + i + 1, cmd)?;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let histfile = env::var(HISTFILE_ENV).unwrap_or_default();
        self.append_to_file(histfile)
    }

    pub fn append_from_file<P: AsRef<Path>>(&self, path: P) {
        let new_entries = fs::read_to_string(path)
            .map(|s| s.lines().map(String::from).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut data = self.inner.borrow_mut();
        data.entries.extend(new_entries);
    }

    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        for entry in &self.inner.borrow().entries {
            writeln!(writer, "{entry}")?;
        }
        writer.flush()?;
        Ok(())
    }

    pub fn append_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let mut data = self.inner.borrow_mut();
        let file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        let mut writer = BufWriter::new(file);
        for entry in data.entries.iter().skip(data.append_index) {
            writeln!(writer, "{entry}")?;
        }
        writer.flush()?;
        data.append_index = data.entries.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::NamedTempFile;

    fn empty_history() -> History {
        History {
            inner: Rc::new(RefCell::new(HistoryData {
                entries: Vec::new(),
                append_index: 0,
            })),
        }
    }

    #[test]
    fn add_and_get() {
        let mut h = empty_history();
        h.add("ls".into());
        h.add("pwd".into());
        assert_eq!(h.len(), 2);
        assert_eq!(h.get(0).unwrap(), "ls");
        assert_eq!(h.get(1).unwrap(), "pwd");
    }

    #[test]
    fn add_ignores_empty() {
        let mut h = empty_history();
        h.add(String::new());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn get_out_of_bounds() {
        let h = empty_history();
        assert!(h.get(0).is_none());
    }

    #[test]
    fn set_updates_entry() {
        let mut h = empty_history();
        h.add("old".into());
        h.set(0, "new".into());
        assert_eq!(h.get(0).unwrap(), "new");
    }

    #[test]
    fn set_out_of_bounds_is_noop() {
        let mut h = empty_history();
        h.set(5, "nope".into());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn clear_removes_all() {
        let mut h = empty_history();
        h.add("a".into());
        h.add("b".into());
        h.clear();
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn print_all() {
        let mut h = empty_history();
        h.add("first".into());
        h.add("second".into());
        let mut buf = Cursor::new(Vec::new());
        h.print(&mut buf, None).unwrap();
        let output = String::from_utf8(buf.into_inner()).unwrap();
        assert_eq!(output, "    1 first\n    2 second\n");
    }

    #[test]
    fn print_with_limit() {
        let mut h = empty_history();
        h.add("a".into());
        h.add("b".into());
        h.add("c".into());
        let mut buf = Cursor::new(Vec::new());
        h.print(&mut buf, Some(2)).unwrap();
        let output = String::from_utf8(buf.into_inner()).unwrap();
        assert_eq!(output, "    2 b\n    3 c\n");
    }

    #[test]
    fn write_and_read_file() {
        let tmp = NamedTempFile::new().unwrap();
        let mut h = empty_history();
        h.add("cmd1".into());
        h.add("cmd2".into());
        h.write_to_file(tmp.path()).unwrap();

        let contents = fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(contents, "cmd1\ncmd2\n");
    }

    #[test]
    fn append_to_file_only_appends_new() {
        let tmp = NamedTempFile::new().unwrap();
        let mut h = empty_history();
        h.add("first".into());
        h.append_to_file(tmp.path()).unwrap();

        h.add("second".into());
        h.append_to_file(tmp.path()).unwrap();

        let contents = fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(contents, "first\nsecond\n");
    }

    #[test]
    fn append_from_file_extends_entries() {
        let tmp = NamedTempFile::new().unwrap();
        fs::write(tmp.path(), "x\ny\n").unwrap();

        let mut h = empty_history();
        h.add("existing".into());
        h.append_from_file(tmp.path());
        assert_eq!(h.len(), 3);
        assert_eq!(h.get(1).unwrap(), "x");
        assert_eq!(h.get(2).unwrap(), "y");
    }
}
