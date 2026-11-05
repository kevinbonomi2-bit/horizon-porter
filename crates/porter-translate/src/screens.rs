//! A mod's own screens (`extends Screen`: a guidebook, a menu opened by a key),
//! ported to Horizon's mod screen (API 111): `native_mod_screen_open` shows it over
//! the world, and the screen hooks draw it and take its clicks, keys and scrolling.
//!
//! In `runtime.rs` each screen is a struct (its Java fields, to port) with
//! functions to port from `render`, `mouseClicked`, `keyPressed` and
//! `mouseScrolled`. Drawing records rectangles, text, sprites and items in a `Gui`;
//! the helpers draw them. Ported code opens one with `open_screen(...)`, which the
//! frame hook carries out (opening needs the game).

use std::fmt::Write;

use porter_model::CustomScreenInfo;

/// A screen, ready to generate.
#[derive(Debug, Clone)]
pub struct ModScreen {
    /// The Java class: `net.grind.client.GuideScreen`.
    pub class: String,
    /// Its struct and `Screen` variant: `GuideScreen`, `Guide`.
    pub rust: String,
    pub variant: String,
    /// `guide`, for its functions.
    pub snake: String,
    pub opened_by: Vec<String>,
}

/// The screens to generate, from the scan.
pub fn find(screens: &[CustomScreenInfo]) -> Vec<ModScreen> {
    let mut out: Vec<ModScreen> = Vec::new();
    for s in screens {
        let simple = s.class.rsplit('.').next().unwrap_or(&s.class);
        let base = simple
            .strip_suffix("Screen")
            .filter(|b| !b.is_empty())
            .unwrap_or(simple);
        if out.iter().any(|m| m.variant == base) {
            continue;
        }
        out.push(ModScreen {
            class: s.class.clone(),
            rust: if simple.ends_with("Screen") {
                simple.to_string()
            } else {
                format!("{simple}Screen")
            },
            variant: base.to_string(),
            snake: snake(base),
            opened_by: s.opened_by.clone(),
        });
    }
    out
}

/// The hooks a mod with its own screens needs.
pub const HOOKS: &[&str] = &[
    "frame",
    "screen_render_post",
    "screen_mouse_button_pressed_pre",
    "screen_key_pressed_pre",
    "screen_mouse_scrolled_pre",
];

/// The generated screens: the drawing types, a struct and functions to port for
/// each screen, and the open screen's state.
pub fn runtime(screens: &[ModScreen]) -> String {
    if screens.is_empty() {
        return String::new();
    }
    let mut r = String::from(GUI_TYPES);
    r.push_str("\n/// The mod's own screens, with their state.\n#[derive(Debug, Clone, PartialEq)]\npub enum Screen {\n");
    for s in screens {
        let _ = writeln!(r, "    {}({}),", s.variant, s.rust);
    }
    r.push_str("}\n");
    for s in screens {
        let opened = if s.opened_by.is_empty() {
            String::new()
        } else {
            format!(
                "\n/// Opened by {} (`setScreen(new {}())`): there, call\n/// `open_screen(Screen::{}({}::default()))`.",
                s.opened_by
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                s.class.rsplit('.').next().unwrap_or(&s.class),
                s.variant,
                s.rust
            )
        };
        let _ = write!(
            r,
            r#"
/// `{class}`'s state: its fields that last while it is open.{opened}
///
/// TODO: port the fields of `{class}`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct {rust} {{}}

/// Draws `{class}` (its `render`): `gui` is the screen's size, in GUI pixels from its
/// top left, with the mouse where it is.
///
/// TODO: port the `render` of `{class}`.
pub fn draw_{snake}(screen: &mut {rust}, gui: &mut Gui) {{
    let _ = (screen, gui);
}}

/// A click at `at` (GUI pixels from the screen's top left) with `button` (0 left,
/// 1 right, 2 middle). True when the screen used it (`mouseClicked`).
///
/// TODO: port the `mouseClicked` of `{class}`.
pub fn click_{snake}(screen: &mut {rust}, at: [f32; 2], button: i32) -> bool {{
    let _ = (screen, at, button);
    false
}}

/// A key went down (`key.keyboard.e`). True when the screen used it (`keyPressed`);
/// Escape closes the screen unless it is used.
///
/// TODO: port the `keyPressed` of `{class}`.
pub fn key_{snake}(screen: &mut {rust}, key: &str) -> bool {{
    let _ = (screen, key);
    false
}}

/// The wheel turned by `amount` (up is positive). True when the screen used it
/// (`mouseScrolled`).
///
/// TODO: port the `mouseScrolled` of `{class}`.
pub fn scroll_{snake}(screen: &mut {rust}, amount: f32) -> bool {{
    let _ = (screen, amount);
    false
}}
"#,
            class = s.class,
            rust = s.rust,
            snake = s.snake,
        );
    }
    r.push_str(SCREEN_STATE);
    // Dispatch to the open screen.
    for (f, args, ret, sig) in [
        ("draw", "gui", "", "gui: &mut Gui"),
        ("click", "at, button", "false", "at: [f32; 2], button: i32"),
        ("key", "key", "false", "key: &str"),
        ("scroll", "amount", "false", "amount: f32"),
    ] {
        let out = if ret.is_empty() { "" } else { " -> bool" };
        let _ = write!(
            r,
            "\n/// `{f}_<screen>` for the open screen.\npub fn {f}_open_screen({sig}){out} {{\n    let Ok(mut open) = OPEN.lock() else {{\n        return{none};\n    }};\n    match open.as_mut() {{\n",
            none = if ret.is_empty() {
                String::new()
            } else {
                format!(" {ret}")
            }
        );
        for s in screens {
            let _ = writeln!(
                r,
                "        Some(Screen::{v}(s)) => {f}_{snake}(s, {args}),",
                v = s.variant,
                snake = s.snake
            );
        }
        let unused = if args.contains(',') {
            format!("({args})")
        } else {
            args.to_string()
        };
        let _ = write!(
            r,
            "        None => {{\n            let _ = {unused};\n            {ret}\n        }}\n    }}\n}}\n"
        );
    }
    r
}

const GUI_TYPES: &str = r#"
/// One step of drawing a screen, in GUI pixels from its top left.
#[derive(Debug, Clone, PartialEq)]
pub enum GuiOp {
    /// `guiGraphics.fill(x1, y1, x2, y2, argb)` as `[x, y, width, height]`.
    Fill { rect: [f32; 4], argb: u32 },
    /// `guiGraphics.drawString(font, text, x, y, rgb)`.
    Text { text: String, at: [f32; 2], rgb: u32, shadow: bool },
    /// `guiGraphics.blit(texture, ...)`: part `src` of sprite `name`
    /// (`grind:gui/guide`, from `assets/grind/textures/gui/guide.png`) drawn at `dst`.
    Sprite { name: String, src: [f32; 4], dst: [f32; 4] },
    /// `guiGraphics.renderItem(stack, x, y)`: an item's icon, `size` pixels square.
    Item { item: String, at: [f32; 2], size: f32 },
}

/// A screen being drawn: its size and the mouse, and what it draws.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gui {
    /// The screen's width and height (`this.width`, `this.height`).
    pub size: [f32; 2],
    /// The mouse, from the screen's top left.
    pub mouse: [f32; 2],
    pub ops: Vec<GuiOp>,
}

impl Gui {
    pub fn width(&self) -> f32 {
        self.size[0]
    }
    pub fn height(&self) -> f32 {
        self.size[1]
    }
    /// Whether the mouse is over `[x, y, width, height]` (`isHovered`).
    pub fn hovered(&self, rect: [f32; 4]) -> bool {
        let [x, y, w, h] = rect;
        let [mx, my] = self.mouse;
        mx >= x && my >= y && mx < x + w && my < y + h
    }
    pub fn fill(&mut self, rect: [f32; 4], argb: u32) {
        self.ops.push(GuiOp::Fill { rect, argb });
    }
    pub fn text(&mut self, text: &str, at: [f32; 2], rgb: u32) {
        self.ops.push(GuiOp::Text { text: text.to_string(), at, rgb, shadow: true });
    }
    pub fn sprite(&mut self, name: &str, src: [f32; 4], dst: [f32; 4]) {
        self.ops.push(GuiOp::Sprite { name: name.to_string(), src, dst });
    }
    pub fn item(&mut self, item: &str, at: [f32; 2]) {
        self.ops.push(GuiOp::Item { item: item.to_string(), at, size: 16.0 });
    }
}
"#;

const SCREEN_STATE: &str = r#"
static OPEN: std::sync::Mutex<Option<Screen>> = std::sync::Mutex::new(None);
/// What ported code asked for: `Some(screen)` to open one, `None` to close.
static REQUEST: std::sync::Mutex<Option<Option<Screen>>> = std::sync::Mutex::new(None);

/// Opens one of the mod's screens over the world
/// (`Minecraft.getInstance().setScreen(new GuideScreen())`), on the next frame.
pub fn open_screen(screen: Screen) {
    if let Ok(mut r) = REQUEST.lock() {
        *r = Some(Some(screen));
    }
}

/// Closes the mod's open screen (`onClose`, `setScreen(null)`), on the next frame.
pub fn close_screen() {
    if let Ok(mut r) = REQUEST.lock() {
        *r = Some(None);
    }
}

/// The frame hook's part: what was asked for, once.
pub fn take_screen_request() -> Option<Option<Screen>> {
    REQUEST.lock().ok().and_then(|mut r| r.take())
}

/// The frame hook's part: the screen is open now, or was closed (Escape too).
pub fn set_open_screen(screen: Option<Screen>) {
    if let Ok(mut open) = OPEN.lock() {
        *open = screen;
    }
}

/// Whether one of the mod's screens is open.
pub fn screen_is_open() -> bool {
    OPEN.lock().map(|o| o.is_some()).unwrap_or(false)
}
"#;

/// Tests: each screen opens and draws, and requests are taken once.
pub fn tests(screens: &[ModScreen]) -> String {
    let Some(first) = screens.first() else {
        return String::new();
    };
    format!(
        r#"
    #[test]
    fn screens_open_draw_and_close() {{
        use super::*;
        open_screen(Screen::{v}({rust}::default()));
        let Some(Some(screen)) = take_screen_request() else {{
            panic!("the request was lost");
        }};
        assert_eq!(take_screen_request(), None);
        set_open_screen(Some(screen));
        assert!(screen_is_open());
        let mut gui = Gui {{ size: [320.0, 240.0], ..Default::default() }};
        draw_open_screen(&mut gui);
        let _ = click_open_screen([10.0, 10.0], 0);
        let _ = scroll_open_screen(1.0);
        set_open_screen(None);
        assert!(!screen_is_open());
        assert!(!key_open_screen("key.keyboard.e"));
    }}
"#,
        v = first.variant,
        rust = first.rust
    )
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
