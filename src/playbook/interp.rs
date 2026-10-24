use std::collections::HashMap;

pub fn interpolate(template: &str, vars: &HashMap<String, String>) -> Result<String, InterpError> {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }

        // Peek: if next is '{', this is ${VAR}; otherwise it's shell-style, copy literally.
        if chars.peek().map(|(_, c)| *c) != Some('{') {
            out.push('$');
            continue;
        }
        chars.next(); // consume '{'

        let mut name = String::new();
        let mut closed = false;
        for (_, c) in chars.by_ref() {
            if c == '}' {
                closed = true;
                break;
            }
            name.push(c);
        }

        if !closed {
            out.push('$');
            out.push('{');
            out.push_str(&name);
            continue;
        }

        match vars.get(&name) {
            Some(val) => out.push_str(val),
            None => return Err(InterpError::UnknownVar(name)),
        }
    }

    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpError {
    UnknownVar(String),
}

impl std::fmt::Display for InterpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpError::UnknownVar(name) => write!(f, "unknown variable: {name}"),
        }
    }
}

impl std::error::Error for InterpError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_known_vars() {
        let mut vars = HashMap::new();
        vars.insert("GITHUB_PR_URL".into(), "https://example/pr/1".into());
        let out = interpolate("review ${GITHUB_PR_URL} now", &vars).unwrap();
        assert_eq!(out, "review https://example/pr/1 now");
    }

    #[test]
    fn unknown_var_is_error() {
        let vars = HashMap::new();
        let err = interpolate("x ${NOPE}", &vars).unwrap_err();
        assert!(format!("{err}").contains("NOPE"));
    }

    #[test]
    fn ignores_shell_style_in_message() {
        let vars = HashMap::new();
        let out = interpolate("use \"$COMMENTS\" as-is", &vars).unwrap();
        assert_eq!(out, "use \"$COMMENTS\" as-is");
    }
}
