//! Reading the command a `<Terminal run="...">` names into the words it runs. This is all the
//! reading there is: no shell is added, so nothing in it expands, pipes or redirects.

/// The most bytes a command may be.
pub const MOST_COMMAND_BYTES: usize = 4096;
/// The most words (the program and its arguments) a command may have.
pub const MOST_COMMAND_WORDS: usize = 64;

/// The words of `run`: spaces separate them, double quotes group (and `\"` and `\\` inside quotes
/// are a quote and a backslash), and nothing else is special. The first word is the program, and
/// what the others are is what it's given. A command that is empty, too long, or has a quote that
/// isn't closed, says so in a sentence.
pub fn split_command(run: &str) -> Result<Vec<String>, String> {
    if run.len() > MOST_COMMAND_BYTES {
        return Err("The command to run is too long.".to_string());
    }
    let mut words: Vec<String> = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = run.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            ' ' if !quoted => words.extend(word.take()),
            '"' => {
                quoted = !quoted;
                word.get_or_insert_with(String::new);
            }
            '\\' if quoted => {
                // Only a quote or a backslash is escaped; any other backslash is itself.
                let mut ahead = chars.clone();
                match ahead.next() {
                    Some(next @ ('"' | '\\')) => {
                        word.get_or_insert_with(String::new).push(next);
                        chars = ahead;
                    }
                    _ => word.get_or_insert_with(String::new).push('\\'),
                }
            }
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    if quoted {
        return Err("The command to run has a quote that isn't closed.".to_string());
    }
    words.extend(word);
    if words.is_empty() {
        return Err("The command to run is empty.".to_string());
    }
    if words.len() > MOST_COMMAND_WORDS {
        return Err("The command to run is too long.".to_string());
    }
    Ok(words)
}
