// Java as a prompt shows it. A prompt is paid for by the token, every time it is
// sent, and past 100,000 tokens Claude Haiku 5.5 charges five times as much for each:
// what a port doesn't read is left out here, by rule, the same for every model.

/// Which bytes of `source` are code: not in a comment, a string or a character.
fn code_mask(source: &str) -> Vec<bool> {
    let b = source.as_bytes();
    let mut mask = vec![true; b.len()];
    let mut i = 0;
    while i < b.len() {
        let end = match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                source[i..].find('\n').map_or(b.len(), |e| i + e)
            }
            b'/' if b.get(i + 1) == Some(&b'*') => source[i + 2..]
                .find("*/")
                .map_or(b.len(), |e| i + 2 + e + 2),
            b'"' if source[i..].starts_with("\"\"\"") => source[i + 3..]
                .find("\"\"\"")
                .map_or(b.len(), |e| i + 3 + e + 3),
            quote @ (b'"' | b'\'') => {
                let mut j = i + 1;
                while j < b.len() && b[j] != quote && b[j] != b'\n' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                (j + 1).min(b.len())
            }
            _ => {
                i += 1;
                continue;
            }
        };
        for m in &mut mask[i..end] {
            *m = false;
        }
        i = end;
    }
    mask
}

/// Java without its `package` and `import` lines (a fifth of the text of a large
/// mod's classes, and nothing a port reads; `import static` stays, which says where a
/// bare name comes from) and without runs of blank lines.
pub fn trimmed(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut blank = true;
    for line in source.lines() {
        let t = line.trim_start();
        if (t.starts_with("import ") && !t.starts_with("import static ") && t.ends_with(';'))
            || (t.starts_with("package ") && t.ends_with(';'))
        {
            continue;
        }
        if t.is_empty() {
            if blank {
                continue;
            }
            blank = true;
        } else {
            blank = false;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out
}

/// Whether `text` calls `name`: `name(`, or `::name` handed on as a function.
fn calls(text: &str, name: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = &text[at + name.len()..];
        !before.is_some_and(ident)
            && (after.starts_with('(') || (text[..at].ends_with("::") && !after.starts_with(ident)))
    })
}

/// One member of a class: where it is in the source, and for a method or an inner
/// type, its name and where its body opens.
struct Member {
    start: usize,
    end: usize,
    /// `(name, the body's `{`)` for what has a body that can be left out.
    body: Option<(String, usize)>,
}

/// The members of the first class `source` declares.
fn members(source: &str, mask: &[bool]) -> Option<Vec<Member>> {
    let b = source.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
    let word_at = |i: usize, w: &str| {
        source[i..].starts_with(w)
            && !(i > 0 && ident(b[i - 1]))
            && !b.get(i + w.len()).copied().is_some_and(ident)
    };
    // Where the class's own body opens: the first `{` after its keyword.
    let keyword = (0..b.len()).find(|&i| {
        mask[i]
            && ["class", "interface", "enum", "record"]
                .iter()
                .any(|w| word_at(i, w))
    })?;
    let open = (keyword..b.len()).find(|&i| mask[i] && b[i] == b'{')?;
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = open + 1;
    let mut closed = false;
    for i in open..b.len() {
        if !mask[i] {
            continue;
        }
        let ends = match b[i] {
            b'{' => {
                depth += 1;
                false
            }
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    closed = true;
                    break;
                }
                // A method or an inner type ends here; an initializer's block (an
                // anonymous class, a lambda, an array) goes on to its `;`.
                depth == 1
                    && !(i + 1..b.len())
                        .find(|&j| mask[j] && !b[j].is_ascii_whitespace())
                        .is_some_and(|j| matches!(b[j], b';' | b',' | b')' | b'.'))
            }
            b';' => depth == 1,
            _ => false,
        };
        if ends {
            out.push((start, i + 1));
            start = i + 1;
        }
    }
    if !closed {
        return None;
    }
    Some(
        out.into_iter()
            .map(|(start, end)| {
                let text = &source[start..end];
                let body = (|| {
                    if !text.ends_with('}') {
                        return None;
                    }
                    let brace = (start..end).find(|&i| mask[i] && b[i] == b'{')?;
                    // An inner type by its keyword, a method by the name before its
                    // parameters (an annotation's parentheses are not them).
                    for i in start..brace {
                        if !mask[i] {
                            continue;
                        }
                        if ["class", "interface", "enum", "record"]
                            .iter()
                            .any(|w| word_at(i, w))
                        {
                            let rest = source[i..brace].split_whitespace().nth(1)?;
                            let name: String =
                                rest.chars().take_while(|c| ident(*c as u8)).collect();
                            return Some((name, brace));
                        }
                        if b[i] == b'=' {
                            // A field with an initializer.
                            return None;
                        }
                        if b[i] == b'(' {
                            let name_start =
                                (start..i).rev().take_while(|&j| ident(b[j])).last()?;
                            if name_start > start && b[name_start - 1] == b'@' {
                                continue;
                            }
                            return Some((source[name_start..i].to_string(), brace));
                        }
                    }
                    None
                })();
                Member { start, end, body }
            })
            .collect(),
    )
}

/// A class the code uses, cut to what that code can reach: its fields, and the methods
/// `calling` calls (and those they call in turn) in full; its other methods and inner
/// types by their signature alone. The source as it is where nothing can be left out,
/// or where it isn't Java this reads.
pub fn sliced(source: &str, calling: &str) -> String {
    let mask = code_mask(source);
    let Some(members) = members(source, &mask) else {
        return source.to_string();
    };
    let mut kept: Vec<bool> = members
        .iter()
        .map(|m| {
            m.body
                .as_ref()
                .is_none_or(|(name, _)| calls(calling, name) || calling_names(calling, name))
        })
        .collect();
    // What a kept member calls is kept too: each kept text read once.
    let mut to_read: Vec<usize> = (0..members.len()).filter(|&i| kept[i]).collect();
    while let Some(reader) = to_read.pop() {
        let text = &source[members[reader].start..members[reader].end];
        for i in 0..members.len() {
            if kept[i] {
                continue;
            }
            if let Some((name, _)) = &members[i].body
                && calls(text, name)
            {
                kept[i] = true;
                to_read.push(i);
            }
        }
    }
    if kept.iter().all(|k| *k) {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for (m, keep) in members.iter().zip(&kept) {
        match (&m.body, keep) {
            (Some((_, brace)), false) => {
                // Its signature, without the comment and blank lines before it.
                let head = source[m.start..*brace].trim_end();
                let first = (m.start..*brace)
                    .find(|&i| mask[i] && !source.as_bytes()[i].is_ascii_whitespace())
                    .unwrap_or(m.start);
                let line = source[..first].rfind('\n').map_or(m.start, |n| n + 1);
                out.push_str(&source[at..m.start]);
                out.push('\n');
                out.push_str(&head[line.max(m.start) - m.start..]);
                out.push_str(" { /* not shown */ }");
            }
            _ => out.push_str(&source[at..m.end]),
        }
        at = m.end;
    }
    out.push_str(&source[at..]);
    out
}

/// Whether `calling` names the inner type `name` (`Mode.CRUSHING`, `new Entry(`).
fn calling_names(calling: &str, name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && calling
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .any(|w| w == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BELT: &str = r#"package net.demo.belt;

import java.util.List;
import net.minecraft.core.BlockPos;
import static net.demo.Shapes.FLAT;

/** A belt. */
public class Belt extends Kinetic {
	public int length = 4; // not a brace: {
	private final List<Item> items = new ArrayList<>();
	Runnable later = () -> {
		length++;
	};

	@Override
	public void tick() {
		super.tick();
		if (length > 0) {
			advance("}");
		}
	}

	/** Moves everything along. */
	@SuppressWarnings("unchecked")
	private void advance(String why) {
		for (Item item : items) {
			item.move(speedOf(item));
		}
	}

	float speedOf(Item item) {
		return 0.125f;
	}

	public boolean isEmpty() {
		return items.isEmpty();
	}

	public abstract int slots();

	enum Mode {
		IDLE, RUNNING
	}
}
"#;

    #[test]
    fn leaves_out_imports_and_keeps_static_ones() {
        let t = trimmed(BELT);
        assert!(!t.contains("import java.util.List;") && !t.contains("package net.demo"));
        assert!(t.contains("import static net.demo.Shapes.FLAT;"));
        assert!(t.starts_with("import static"), "{t}");
        assert!(!t.contains("\n\n\n"));
    }

    #[test]
    fn keeps_what_the_code_calls_and_what_that_calls() {
        let s = sliced(BELT, "belt.advance(\"x\"); if (belt.length > 2) {}");
        // `advance` is called, and calls `speedOf`: both in full.
        assert!(
            s.contains("item.move(speedOf(item));") && s.contains("return 0.125f;"),
            "{s}"
        );
        // The others by their signature alone, annotations and all.
        assert!(
            s.contains("\tpublic void tick() { /* not shown */ }"),
            "{s}"
        );
        assert!(
            s.contains("public boolean isEmpty() { /* not shown */ }"),
            "{s}"
        );
        assert!(!s.contains("super.tick()") && !s.contains("items.isEmpty()"));
        // Fields, a lambda field and a method without a body stay as they are.
        assert!(s.contains("public int length = 4;") && s.contains("length++;"));
        assert!(s.contains("public abstract int slots();"));
        // An inner type the code never names goes too.
        assert!(s.contains("enum Mode { /* not shown */ }"), "{s}");
        assert!(sliced(BELT, "Mode.IDLE").contains("IDLE, RUNNING"));
    }

    #[test]
    fn a_method_handed_on_as_a_function_is_called() {
        let s = sliced(BELT, "forEach(belt::isEmpty)");
        assert!(s.contains("return items.isEmpty();"), "{s}");
    }

    #[test]
    fn what_it_cannot_read_is_left_as_it_is() {
        let broken = "public class A { void f() { ";
        assert_eq!(sliced(broken, "f()"), broken);
        assert_eq!(sliced("not java at all", "x"), "not java at all");
    }
}
