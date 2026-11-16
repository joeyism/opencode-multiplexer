const ARROWS: &[&str] = &[
    "<<-->>", "<<->>", "-->>", "->>", "--x", "-x", "--)", "-)", "-->", "->",
];

const COMMANDS: &[&str] = &[
    "autonumber",
    "participant",
    "actor",
    "create",
    "destroy",
    "box",
    "note",
    "loop",
    "alt",
    "else",
    "opt",
    "par",
    "and",
    "critical",
    "option",
    "break",
    "rect",
    "end",
    "activate",
    "deactivate",
    "title",
    "acctitle",
    "accdescr",
    "link",
    "links",
];

pub fn sanitize_mermaid_source(source: &str) -> String {
    if !is_sequence_diagram(source) {
        return source.to_string();
    }

    let mut found_header = false;
    let mut output = String::with_capacity(source.len() + 16);
    for (index, line) in source.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }

        if found_header {
            output.push_str(&sanitize_sequence_line(line));
        } else {
            output.push_str(line);
            if line.trim_start().starts_with("sequenceDiagram") {
                found_header = true;
            }
        }
    }
    output
}

fn is_sequence_diagram(source: &str) -> bool {
    for line in source.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        return line.starts_with("sequenceDiagram");
    }
    false
}

fn sanitize_sequence_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut output = String::with_capacity(line.len());
    let mut index = 0;

    while index < chars.len() {
        if chars[index] == '#'
            && let Some(end) = entity_end(&chars, index)
        {
            output.extend(&chars[index..=end]);
            index = end + 1;
            continue;
        }

        if chars[index] == ';' {
            let rest: String = chars[index + 1..].iter().collect();
            if remainder_is_statement(&rest) {
                output.push(';');
            } else {
                output.push_str("#59;");
            }
        } else {
            output.push(chars[index]);
        }
        index += 1;
    }

    output
}

fn entity_end(chars: &[char], hash_index: usize) -> Option<usize> {
    let mut index = hash_index + 1;
    if index >= chars.len() {
        return None;
    }

    let numeric = chars[index].is_ascii_digit();
    let named = chars[index].is_ascii_alphabetic();
    if !numeric && !named {
        return None;
    }
    index += 1;

    while index < chars.len()
        && if numeric {
            chars[index].is_ascii_digit()
        } else {
            chars[index].is_ascii_alphabetic()
        }
    {
        index += 1;
    }

    (index < chars.len() && chars[index] == ';').then_some(index)
}

fn remainder_is_statement(rest: &str) -> bool {
    let rest = rest.trim_start();
    rest.is_empty() || starts_with_command(rest) || looks_like_message(rest)
}

fn starts_with_command(rest: &str) -> bool {
    let lower = rest.to_ascii_lowercase();
    COMMANDS.iter().any(|command| {
        lower.starts_with(command)
            && lower[command.len()..]
                .chars()
                .next()
                .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
    })
}

fn looks_like_message(rest: &str) -> bool {
    let chars: Vec<char> = rest.chars().collect();
    let mut index = 0;
    skip_whitespace(&chars, &mut index);
    if !consume_participant(&chars, &mut index) {
        return false;
    }
    skip_whitespace(&chars, &mut index);
    if !consume_arrow(&chars, &mut index) {
        return false;
    }
    skip_whitespace(&chars, &mut index);
    consume_participant(&chars, &mut index)
}

fn skip_whitespace(chars: &[char], index: &mut usize) {
    while *index < chars.len() && chars[*index].is_whitespace() {
        *index += 1;
    }
}

fn consume_participant(chars: &[char], index: &mut usize) -> bool {
    if *index >= chars.len() {
        return false;
    }

    if chars[*index] == '"' {
        *index += 1;
        while *index < chars.len() && chars[*index] != '"' {
            *index += 1;
        }
        if *index < chars.len() {
            *index += 1;
            return true;
        }
        return false;
    }

    if !chars[*index].is_ascii_alphabetic() && chars[*index] != '_' {
        return false;
    }
    *index += 1;
    while *index < chars.len() && (chars[*index].is_ascii_alphanumeric() || chars[*index] == '_') {
        *index += 1;
    }
    true
}

fn consume_arrow(chars: &[char], index: &mut usize) -> bool {
    let rest: String = chars[*index..].iter().collect();
    if let Some(arrow) = ARROWS.iter().find(|arrow| rest.starts_with(**arrow)) {
        *index += arrow.chars().count();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_mermaid_source;

    #[test]
    fn flowchart_semicolons_untouched() {
        let src = "graph TD; A-->B; C-->D";
        assert_eq!(sanitize_mermaid_source(src), src);
    }

    #[test]
    fn note_text_semicolon_becomes_entity() {
        let src = "sequenceDiagram\n    Note over A,B: immediately; independently\n    A->>B: hi\n";
        let out = sanitize_mermaid_source(src);
        assert!(out.contains("immediately#59; independently"), "{out}");
    }

    #[test]
    fn message_text_semicolon_becomes_entity() {
        let src = "sequenceDiagram\n    A->>B: Task stopped; details returned\n";
        let out = sanitize_mermaid_source(src);
        assert!(out.contains("stopped#59; details"), "{out}");
    }

    #[test]
    fn compact_two_statements_on_one_line_kept() {
        let src = "sequenceDiagram\n    A->>B: Hello; B->>C: Hi\n";
        assert_eq!(sanitize_mermaid_source(src), src);
    }

    #[test]
    fn existing_entity_not_double_encoded() {
        let src = "sequenceDiagram\n    A->>B: I #59; you; really\n";
        let out = sanitize_mermaid_source(src);
        assert!(out.contains("I #59; you#59; really"), "{out}");
        assert!(!out.contains("#59#59;"), "{out}");
    }

    #[test]
    fn trailing_semicolon_kept() {
        let src = "sequenceDiagram\n    A->>B: hello;\n";
        assert_eq!(sanitize_mermaid_source(src), src);
    }

    #[test]
    fn keyword_after_semicolon_kept() {
        let src = "sequenceDiagram\n    A->>B: hi; Note over A: x\n";
        assert_eq!(sanitize_mermaid_source(src), src);
    }

    #[test]
    fn arrow_in_note_text_does_not_preserve_later_text_semicolon() {
        let src = "sequenceDiagram\n    Note over A,B: Workflow->>ECS is next; then wait\n";
        let out = sanitize_mermaid_source(src);
        assert!(out.contains("next#59; then wait"), "{out}");
    }

    #[test]
    fn init_directive_still_detected() {
        let src = "%%{init: {'theme':'dark'}}%%\nsequenceDiagram\n    A->>B: a; b\n";
        let out = sanitize_mermaid_source(src);
        assert!(out.contains("a#59; b"), "{out}");
        assert!(out.starts_with("%%{init:"), "{out}");
    }

    #[test]
    fn user_diagram_two_offending_semicolons() {
        let src = "sequenceDiagram\n    Note over Caller,Workflow: Request returns immediately; workflow runs independently\n    Workflow->>ECS: RunTask.sync with batch_id\n    ECS-->>Workflow: Task stopped; completion details returned\n";
        let out = sanitize_mermaid_source(src);
        assert!(
            out.contains("immediately#59; workflow runs independently"),
            "{out}"
        );
        assert!(
            out.contains("stopped#59; completion details returned"),
            "{out}"
        );
        assert!(out.contains("Workflow->>ECS: RunTask.sync"), "{out}");
    }
}
