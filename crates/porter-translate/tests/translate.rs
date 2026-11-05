// The AI pass with a scripted model: the compile-and-fix loop, signature checks,
// hook helpers, dry runs, resuming, and `port --force` keeping what was ported.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use anyhow::Result;
use porter_translate::ai::{
    self, Options, Status,
    model::{Model, Reply},
};
use porter_translate::bundle;

/// A model that answers from a script, and keeps the prompts it was sent.
struct Scripted {
    answers: RefCell<Vec<String>>,
    prompts: RefCell<Vec<String>>,
}

impl Scripted {
    fn new(answers: &[&str]) -> Scripted {
        Scripted {
            answers: RefCell::new(answers.iter().rev().map(|a| a.to_string()).collect()),
            prompts: RefCell::new(Vec::new()),
        }
    }
}

impl Model for Scripted {
    fn name(&self) -> String {
        "scripted".into()
    }
    fn complete(&self, _system: &str, shared: &str, prompt: &str) -> Result<Reply> {
        self.prompts
            .borrow_mut()
            .push(format!("{shared}\n{prompt}"));
        let text = self
            .answers
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| "no more answers".into());
        Ok(Reply {
            text,
            input_tokens: 100,
            output_tokens: 10,
        })
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures")
        .join(name)
}

/// Ports a fixture into a fresh folder; the folder and the scan.
fn ported(name: &str, tag: &str) -> (PathBuf, porter_model::ModScan) {
    let scan = porter_java::scan(&fixture(name)).unwrap();
    let b = bundle::generate(&scan, &bundle::load_lang(&scan));
    let dir = std::env::temp_dir().join(format!("porter-translate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    bundle::write(&b, &dir, false).unwrap();
    (dir, scan)
}

const GOOD_TICK: &str = "```rust
pub fn tick_crank(pos: [i32; 3], state: &mut Crank) {
    const MAX_SPEED: f32 = 256.0;
    let _ = pos;
    state.speed = (state.speed + 1.0).min(MAX_SPEED);
    if state.speed >= MAX_SPEED {
        state.turns += 1;
    }
}
```";

#[test]
fn ports_a_block_entity_tick_and_fixes_compile_errors() {
    let (dir, scan) = ported("neoforge-demo", "tick");
    let pending = ai::pending(&dir).unwrap();
    assert!(
        pending.iter().any(|u| u.function == "tick_crank"),
        "{pending:?}"
    );

    // First a version that doesn't compile, then a good one.
    let model = Scripted::new(&[
        "```rust\npub fn tick_crank(pos: [i32; 3], state: &mut Crank) {\n    state.spede += 1.0;\n}\n```",
        GOOD_TICK,
    ]);
    let opts = Options {
        only: Some("tick_crank".into()),
        ..Default::default()
    };
    let out = ai::translate(&dir, &scan, &model, &opts, |_| {}).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].status, Status::Ported);
    assert_eq!(out[0].attempts, 2);
    assert_eq!(out[0].input_tokens, 200);

    // The first prompt had the Java; the second, the compiler's complaint.
    let prompts = model.prompts.borrow();
    assert!(prompts[0].contains("public class CrankBlockEntity extends BlockEntity"));
    assert!(prompts[0].contains("pub struct Crank"));
    assert!(prompts[1].contains("spede"), "{}", prompts[1]);

    let runtime = std::fs::read_to_string(dir.join("mixins/demo/runtime.rs")).unwrap();
    assert!(runtime.contains("state.turns += 1;"));
    assert!(runtime.contains("Ported by horizon-porter translate (scripted): the tick of"));
    assert!(
        !ai::pending(&dir)
            .unwrap()
            .iter()
            .any(|u| u.function == "tick_crank")
    );
    let state = std::fs::read_to_string(dir.join(".porter/translate.json")).unwrap();
    assert!(state.contains("mixins/demo/runtime.rs::tick_crank"));

    // port --force writes the stubs again, but keeps the port.
    let b = bundle::generate(&scan, &bundle::load_lang(&scan));
    bundle::write(&b, &dir, true).unwrap();
    let runtime = std::fs::read_to_string(dir.join("mixins/demo/runtime.rs")).unwrap();
    assert!(runtime.contains("state.turns += 1;"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn leaves_a_function_as_it_was_when_no_answer_works() {
    let (dir, scan) = ported("neoforge-demo", "fail");
    let before = std::fs::read_to_string(dir.join("mixins/demo/runtime.rs")).unwrap();
    // A changed signature, then two that don't compile.
    let model = Scripted::new(&[
        "```rust\npub fn tick_crank(state: &mut Crank) {}\n```",
        "```rust\npub fn tick_crank(pos: [i32; 3], state: &mut Crank) { nope }\n```",
        "```rust\npub fn tick_crank(pos: [i32; 3], state: &mut Crank) { nope2 }\n```",
    ]);
    let opts = Options {
        only: Some("tick_crank".into()),
        ..Default::default()
    };
    let out = ai::translate(&dir, &scan, &model, &opts, |_| {}).unwrap();
    assert!(
        matches!(out[0].status, Status::Failed(_)),
        "{:?}",
        out[0].status
    );
    assert_eq!(out[0].attempts, 3);
    assert!(model.prompts.borrow()[1].contains("changed the function's signature"));
    let after = std::fs::read_to_string(dir.join("mixins/demo/runtime.rs")).unwrap();
    assert_eq!(before, after);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ports_a_hook_helper_from_its_mixin() {
    let (dir, scan) = ported("fabric-mixin-demo", "helper");
    let model = Scripted::new(&[
        "Here:\n```rust\nfn steadyview_item_picked_up(game: &mut crate::game::Game<'_>, player: u128, item: &str, count: u32, canceled: &mut bool) {\n    if !game.native_on(crate::game::ported_steadyview::ID) {\n        return;\n    }\n    let _ = (player, item, canceled);\n    crate::game::ported_steadyview::PICKUPS.fetch_add(count, std::sync::atomic::Ordering::Relaxed);\n}\n```",
    ]);
    let opts = Options {
        only: Some("item_picked_up".into()),
        ..Default::default()
    };
    let out = ai::translate(&dir, &scan, &model, &opts, |_| {}).unwrap();
    assert_eq!(out[0].status, Status::Unchecked);
    let prompt = &model.prompts.borrow()[0];
    // The Mixin's source, the mod's module, and the game's functions.
    assert!(prompt.contains("public class ItemEntityMixin"));
    assert!(prompt.contains("Port the Mixin member `steadyview$countPickup`"));
    assert!(prompt.contains("fn native_chat(&mut self, text: &str)"));
    let helper = std::fs::read_to_string(dir.join("mixins/steadyview/item_picked_up.rs")).unwrap();
    assert!(helper.contains("PICKUPS.fetch_add"));
    assert!(
        helper.contains("Ported by horizon-porter translate (scripted) from these; review it:")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dry_run_calls_nothing() {
    let (dir, scan) = ported("fabric-mixin-demo", "dry");
    let model = Scripted::new(&[]);
    let opts = Options {
        dry_run: true,
        ..Default::default()
    };
    let out = ai::translate(&dir, &scan, &model, &opts, |_| {}).unwrap();
    // hud_lines in runtime.rs, and the three hook helpers.
    let names: Vec<&str> = out.iter().map(|o| o.unit.function.as_str()).collect();
    assert!(names.contains(&"hud_lines"), "{names:?}");
    assert!(names.contains(&"steadyview_compute_fov"), "{names:?}");
    assert!(
        out.iter()
            .all(|o| matches!(o.status, Status::Planned { .. }))
    );
    assert!(model.prompts.borrow().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

/// A model that never answers, as with no key or Ollama not running.
struct Silent;

impl Model for Silent {
    fn name(&self) -> String {
        "silent".into()
    }
    fn complete(&self, _system: &str, _shared: &str, _prompt: &str) -> Result<Reply> {
        anyhow::bail!("is Ollama running?")
    }
}

#[test]
fn stops_when_the_model_gives_no_answer() {
    let (dir, scan) = ported("fabric-mixin-demo", "silent");
    let before = ai::pending(&dir).unwrap().len();
    assert!(before > 1);
    let out = ai::translate(&dir, &scan, &Silent, &Options::default(), |_| {}).unwrap();
    // One try, then it stops: every other function would get the same non-answer.
    assert_eq!(out.len(), 1);
    assert!(matches!(&out[0].status, Status::NoAnswer(why) if why.contains("Ollama")));
    assert_eq!(ai::pending(&dir).unwrap().len(), before);
    std::fs::remove_dir_all(&dir).ok();
}
