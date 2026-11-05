//! A mod's machine GUIs, read from its source: each container menu's slots
//! (`addSlot(new Slot(container, index, x, y))`), which of them only give items out,
//! where the player's inventory sits, and how many data slots it syncs; and each
//! container screen's size, texture and menu.
//!
//! It reads the source text: the shapes mods write these in are few and regular.

use porter_model::{MenuInfo, MenuSlot, ScreenInfo};

/// The menu a class's source declares (`class` is its fully qualified name).
pub fn menu(class: &str, source: &str) -> MenuInfo {
    let mut info = MenuInfo {
        class: class.to_string(),
        ..Default::default()
    };
    let mut inventory: Option<[i32; 2]> = None;
    for args in calls(source, "addSlot") {
        let Some(new) = args.trim().strip_prefix("new ") else {
            continue;
        };
        let Some(open) = new.find('(') else {
            continue;
        };
        let ty = new[..open].trim();
        let Some(inner) = balanced(&new[open..]) else {
            continue;
        };
        let parts = split_top(inner);
        if parts.len() < 4 {
            continue;
        }
        let n = parts.len();
        let (container, index, x, y) = (&parts[0], &parts[n - 3], &parts[n - 2], &parts[n - 1]);
        // The player's own inventory: only where it starts matters.
        if is_player_inventory(container) {
            if let (Some(x), Some(y)) = (first_int(x), first_int(y)) {
                inventory = Some(match inventory {
                    Some([ix, iy]) if iy <= y => [ix.min(x), iy],
                    _ => [x, y],
                });
            }
            continue;
        }
        let (Some(sx), Some(sy)) = (int(x), int(y)) else {
            // `8 + i * 18` in a loop: a row of slots the scanner can't count.
            if let (Some(sx), Some(sy)) = (first_int(x), first_int(y)) {
                info.slots.push(MenuSlot {
                    index: int(index),
                    x: sx,
                    y: sy,
                    result: is_result(ty, source),
                });
            }
            info.approximate = true;
            continue;
        };
        info.slots.push(MenuSlot {
            index: int(index),
            x: sx,
            y: sy,
            result: is_result(ty, source),
        });
    }
    info.inventory = inventory;
    if !calls(source, "addDataSlots").is_empty() {
        info.data_slots = calls(source, "SimpleContainerData")
            .iter()
            .filter_map(|a| int(a))
            .next()
            .unwrap_or(0) as usize;
    }
    info
}

/// The screen a class's source declares.
pub fn screen(class: &str, source: &str) -> ScreenInfo {
    let menu = ["AbstractContainerScreen<", "HandledScreen<"]
        .iter()
        .find_map(|p| {
            let at = source.find(p)? + p.len();
            let name: String = source[at..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            (!name.is_empty()).then(|| name.rsplit('.').next().unwrap_or(&name).to_string())
        });
    let assigned = |names: &[&str]| {
        names.iter().find_map(|name| {
            let mut from = 0;
            while let Some(i) = source[from..].find(name).map(|i| from + i) {
                let rest = source[i + name.len()..].trim_start();
                if let Some(value) = rest.strip_prefix('=')
                    && !value.starts_with('=')
                {
                    let value: String = value
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if let Ok(v) = value.parse() {
                        return Some(v);
                    }
                }
                from = i + name.len();
            }
            None
        })
    };
    let texture = source.split('"').skip(1).step_by(2).find_map(|s| {
        let path = s.split(':').next_back().unwrap_or(s);
        (path.starts_with("textures/gui/") && path.ends_with(".png")).then(|| path.to_string())
    });
    ScreenInfo {
        class: class.to_string(),
        menu,
        width: assigned(&["imageWidth", "backgroundWidth"]),
        height: assigned(&["imageHeight", "backgroundHeight"]),
        texture,
    }
}

/// Whether a slot's container argument is the player's inventory.
fn is_player_inventory(arg: &str) -> bool {
    let a = arg.to_ascii_lowercase();
    (a.contains("inv") && !a.contains("handler") && !a.contains("items")) || a.contains("player")
}

/// Whether a slot of type `ty` only gives items out: a result or output slot, or
/// one of the file's own slot classes whose `mayPlace` answers false.
fn is_result(ty: &str, source: &str) -> bool {
    let simple = ty.rsplit('.').next().unwrap_or(ty);
    if ["Result", "Output", "Furnace"]
        .iter()
        .any(|w| simple.contains(w))
    {
        return true;
    }
    let Some(at) = source.find(&format!("class {simple} ")) else {
        return false;
    };
    let body = &source[at..];
    let body = body
        .find('{')
        .and_then(|b| balanced_braces(&body[b..]))
        .unwrap_or("");
    body.find("mayPlace").is_some_and(|m| {
        let after = &body[m..];
        let end = after.find('}').unwrap_or(after.len());
        after[..end].contains("return false")
    })
}

/// The argument text of each call to `name` in `source` (`name(` ... `)`).
fn calls(source: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = source[from..].find(name).map(|i| from + i) {
        let before = source[..i].chars().next_back();
        let after = &source[i + name.len()..];
        from = i + name.len();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let rest = after.trim_start();
        if !rest.starts_with('(') {
            continue;
        }
        if let Some(args) = balanced(rest) {
            out.push(args.to_string());
        }
    }
    out
}

/// What is inside the parentheses `text` opens with.
fn balanced(text: &str) -> Option<&str> {
    let mut depth = 0;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in text.char_indices() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&text[1..i]);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

/// What is inside the braces `text` opens with.
fn balanced_braces(text: &str) -> Option<&str> {
    let mut depth = 0;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Arguments split at commas outside parentheses.
fn split_top(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// An integer literal.
fn int(text: &str) -> Option<i32> {
    text.trim().parse().ok()
}

/// The first integer literal in an expression: `84 + row * 18` → 84.
fn first_int(text: &str) -> Option<i32> {
    let digits: String = text
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = r#"
        addSlot(new SlotItemHandler(items, 0, 56, 35));
        addSlot(new OutputSlot(items, 1, 116, 35));
        for (int row = 0; row < 3; row++)
            for (int col = 0; col < 9; col++)
                addSlot(new Slot(inventory, col + row * 9 + 9, 8 + col * 18, 84 + row * 18));
        for (int col = 0; col < 9; col++) addSlot(new Slot(inventory, col, 8 + col * 18, 142));
        addDataSlots(data);
        this(id, inventory, new ItemStackHandler(2), new SimpleContainerData(2));
        static class OutputSlot extends SlotItemHandler {
            OutputSlot(ItemStackHandler items, int index, int x, int y) { super(items, index, x, y); }
            @Override public boolean mayPlace(ItemStack stack) { return false; }
        }
    "#;

    #[test]
    fn reads_a_menus_slots() {
        let m = menu("a.CrusherMenu", MENU);
        assert_eq!(
            m.slots,
            [
                MenuSlot {
                    index: Some(0),
                    x: 56,
                    y: 35,
                    result: false
                },
                MenuSlot {
                    index: Some(1),
                    x: 116,
                    y: 35,
                    result: true
                },
            ]
        );
        assert_eq!(m.inventory, Some([8, 84]));
        assert_eq!(m.data_slots, 2);
        assert!(!m.approximate);
    }

    #[test]
    fn reads_a_screen() {
        let s = screen(
            "a.CrusherScreen",
            r#"class CrusherScreen extends AbstractContainerScreen<CrusherMenu> {
                static final ResourceLocation T = ResourceLocation.fromNamespaceAndPath(Grind.ID, "textures/gui/crusher.png");
                CrusherScreen() { this.imageWidth = 176; this.imageHeight = 166; if (imageWidth == 3) {} }"#,
        );
        assert_eq!(s.menu.as_deref(), Some("CrusherMenu"));
        assert_eq!((s.width, s.height), (Some(176), Some(166)));
        assert_eq!(s.texture.as_deref(), Some("textures/gui/crusher.png"));
    }
}
