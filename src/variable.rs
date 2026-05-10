use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
pub struct Variables {
    inner: Arc<RwLock<HashMap<String, String>>>,
}

impl Variables {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set(&self, name: String, value: String) {
        self.inner.write().unwrap().insert(name, value);
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.inner.read().unwrap().get(name).cloned()
    }

    pub fn expand(&self, s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            let name = match chars.peek() {
                Some('{') => {
                    chars.next();
                    let mut n = String::new();
                    for p in chars.by_ref() {
                        if p == '}' {
                            break;
                        }
                        n.push(p);
                    }
                    n
                }
                Some(&p) if p.is_ascii_alphabetic() || p == '_' => {
                    let mut n = String::new();
                    while let Some(&p) = chars.peek() {
                        if p.is_ascii_alphanumeric() || p == '_' {
                            n.push(p);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    n
                }
                _ => {
                    out.push('$');
                    continue;
                }
            };
            if let Some(val) = self.get(&name) {
                out.push_str(&val);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Variables {
        let v = Variables::new();
        for (k, val) in pairs {
            v.set((*k).to_string(), (*val).to_string());
        }
        v
    }

    #[test]
    fn expand_replaces_known_variable() {
        let v = vars(&[("FOO", "bar")]);
        assert_eq!(v.expand("$FOO"), "bar");
    }

    #[test]
    fn expand_replaces_unset_with_empty() {
        let v = Variables::new();
        assert_eq!(v.expand("[$MISSING]"), "[]");
    }

    #[test]
    fn expand_stops_at_non_identifier_char() {
        let v = vars(&[("FOO", "bar")]);
        assert_eq!(v.expand("$FOO/$FOO.txt"), "bar/bar.txt");
    }

    #[test]
    fn expand_leaves_dollar_without_identifier_literal() {
        let v = Variables::new();
        assert_eq!(v.expand("price: $5"), "price: $5");
        assert_eq!(v.expand("end$"), "end$");
    }

    #[test]
    fn expand_handles_underscore_start() {
        let v = vars(&[("_X", "ok")]);
        assert_eq!(v.expand("$_X"), "ok");
    }

    #[test]
    fn expand_multiple_in_one_string() {
        let v = vars(&[("A", "1"), ("B", "2")]);
        assert_eq!(v.expand("$A $B"), "1 2");
    }

    #[test]
    fn expand_brace_form_appends_literal_suffix() {
        let v = vars(&[("Var1", "foo")]);
        assert_eq!(v.expand("${Var1}end"), "fooend");
    }

    #[test]
    fn expand_brace_form_in_middle() {
        let v = vars(&[("Item", "widget")]);
        assert_eq!(v.expand("stock_${Item}_id"), "stock_widget_id");
    }

    #[test]
    fn expand_mixed_brace_and_bare() {
        let v = vars(&[("A", "1"), ("B", "2")]);
        assert_eq!(v.expand("${A}-$B"), "1-2");
    }
}
