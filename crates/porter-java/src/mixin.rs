// Maps Java Mixin members to horizon-mixins kinds and verdicts.
//
// Source of truth: horizon-mixins' docs/spongepowered-feature-gaps.md (0.6.0). A
// "supported" verdict is about the injection *kind*: whether the Java target method
// has a Rust counterpart in Horizon is decided later, through the port ledger (M3).

use porter_model::{MixinMember, Verdict};

use crate::source::{Annotation, Member, Value};

/// Annotations on a Mixin member that matter for porting.
const MIXIN_ANNOTATIONS: &[&str] = &[
    "Inject",
    "Redirect",
    "ModifyArg",
    "ModifyArgs",
    "ModifyVariable",
    "ModifyConstant",
    "Overwrite",
    "Accessor",
    "Invoker",
    "Shadow",
    "Unique",
    "WrapOperation",
    "WrapWithCondition",
    "ModifyExpressionValue",
    "ModifyReturnValue",
    "ModifyReceiver",
    "WrapMethod",
];

/// The members of a Mixin class, mapped. Constructors are skipped: a Mixin's
/// constructor never runs.
pub(crate) fn members(members: &[Member]) -> Vec<MixinMember> {
    let mut out = Vec::new();
    for member in members.iter().filter(|m| !m.is_constructor) {
        let relevant: Vec<&Annotation> = member
            .annotations
            .iter()
            .filter(|a| MIXIN_ANNOTATIONS.contains(&a.simple_name()))
            .collect();
        if relevant.is_empty() {
            out.push(merged(member));
        }
        for annotation in relevant {
            out.push(map(member, annotation));
        }
    }
    out
}

/// A member with no Mixin annotation: Mixin merges it into the target class.
fn merged(member: &Member) -> MixinMember {
    let (horizon_kind, verdict) = if member.is_method {
        (
            None,
            Verdict::Supported(
                "method added to the target: becomes a function in the mod's module".into(),
            ),
        )
    } else {
        (
            Some("STATE".to_string()),
            Verdict::Supported("field added to the target: namespaced per-instance state".into()),
        )
    };
    MixinMember {
        member: member.name.clone(),
        annotation: "merged".into(),
        target_methods: Vec::new(),
        at: None,
        at_target: None,
        cancellable: false,
        horizon_kind,
        verdict,
    }
}

fn map(member: &Member, annotation: &Annotation) -> MixinMember {
    let name = annotation.simple_name();
    let target_methods = annotation
        .arg("method")
        .map(|v| v.items().iter().map(|i| i.text()).collect())
        .unwrap_or_default();
    let at_annotation = annotation.arg("at").and_then(|v| match v {
        Value::Annotation(a) => Some(a.clone()),
        Value::Array(items) => items.iter().find_map(|i| match i {
            Value::Annotation(a) => Some(a.clone()),
            _ => None,
        }),
        _ => None,
    });
    let at = at_annotation
        .as_ref()
        .and_then(|a| a.arg("value"))
        .map(Value::text);
    let at_target = at_annotation
        .as_ref()
        .and_then(|a| a.arg("target"))
        .map(Value::text);
    let shift_after = at_annotation
        .as_ref()
        .and_then(|a| a.arg("shift"))
        .is_some_and(|s| s.text().ends_with("AFTER"));
    let cancellable = annotation.arg("cancellable") == Some(&Value::Bool(true));

    let (kind, verdict) = verdict(
        name,
        at.as_deref(),
        shift_after,
        cancellable,
        member.is_method,
    );
    MixinMember {
        member: member.name.clone(),
        annotation: name.to_string(),
        target_methods,
        at,
        at_target,
        cancellable,
        horizon_kind: kind.map(str::to_string),
        verdict,
    }
}

/// The horizon-mixins kind and verdict for one annotation.
pub(crate) fn verdict(
    annotation: &str,
    at: Option<&str>,
    shift_after: bool,
    cancellable: bool,
    is_method: bool,
) -> (Option<&'static str>, Verdict) {
    use Verdict::{Blocked, NotNeeded, Partial, Supported};
    let s = |t: &str| t.to_string();
    match annotation {
        "Inject" => match at {
            Some("HEAD") => (
                Some("HEAD"),
                Supported(if cancellable {
                    s("HEAD with cancellable = true")
                } else {
                    s("HEAD")
                }),
            ),
            Some("RETURN") => (
                Some("RETURN"),
                Supported(s("RETURN runs at every normal exit")),
            ),
            Some("TAIL") => (
                Some("RETURN"),
                Partial(s(
                    "horizon-mixins has no TAIL; RETURN runs at every exit, which matches only when the function has one exit",
                )),
            ),
            Some("INVOKE") if shift_after => (
                Some("AFTER_CALL"),
                Supported(s(
                    "AFTER_CALL: the call must be a whole statement, a let initializer or a block tail",
                )),
            ),
            Some("INVOKE") => (
                Some("BEFORE_CALL"),
                Supported(s(
                    "BEFORE_CALL: the call must be a whole statement, a let initializer or a block tail",
                )),
            ),
            Some("INVOKE_ASSIGN") => (Some("AFTER_ASSIGN"), Supported(s("AFTER_ASSIGN"))),
            Some(
                p @ ("FIELD" | "NEW" | "JUMP" | "CONSTANT" | "LOAD" | "STORE" | "INVOKE_STRING"),
            ) => (
                None,
                Blocked(format!(
                    "horizon-mixins cannot select {p} injection points yet"
                )),
            ),
            Some(p) if p.starts_with("MIXINEXTRAS:") => (
                None,
                Blocked(s(
                    "MixinExtras expression injection points have no counterpart",
                )),
            ),
            Some(p) => (None, Partial(format!("unknown injection point {p}"))),
            None => (None, Partial(s("no injection point found"))),
        },
        "Redirect" => match at {
            Some("INVOKE") | None => (
                Some("WRAP_OPERATION"),
                Partial(s(
                    "no @Redirect in horizon-mixins: rewrite as a WRAP_OPERATION that skips the original call",
                )),
            ),
            Some(p) => (
                None,
                Blocked(format!(
                    "@Redirect of a {p} site has no horizon-mixins counterpart"
                )),
            ),
        },
        "ModifyArg" => (
            Some("MODIFY_ARG"),
            Supported(s("MODIFY_ARG: same input and output type")),
        ),
        "ModifyArgs" => (Some("MODIFY_ARGS"), Supported(s("MODIFY_ARGS"))),
        "ModifyVariable" => (
            Some("MODIFY_VARIABLE"),
            Supported(s("MODIFY_VARIABLE: plain locals and parameters only")),
        ),
        "ModifyConstant" => (
            Some("MODIFY_CONSTANT"),
            Supported(s("MODIFY_CONSTANT: exact literals only")),
        ),
        "Overwrite" => (
            Some("OVERWRITE"),
            Supported(s(
                "OVERWRITE replaces the whole body and conflicts with every other hook on it",
            )),
        ),
        "WrapOperation" => (Some("WRAP_OPERATION"), Supported(s("WRAP_OPERATION"))),
        "WrapWithCondition" => (
            Some("WRAP_OPERATION"),
            Supported(s(
                "a WRAP_OPERATION that calls the original only when the condition holds",
            )),
        ),
        "ModifyExpressionValue" => (
            Some("MODIFY_EXPRESSION_VALUE"),
            Supported(s("MODIFY_EXPRESSION_VALUE")),
        ),
        "ModifyReturnValue" => (
            Some("RETURN"),
            Supported(s("RETURN with result replacement")),
        ),
        "ModifyReceiver" => (
            None,
            Blocked(s("horizon-mixins cannot modify a call's receiver")),
        ),
        "WrapMethod" => (
            None,
            Blocked(s(
                "no whole-method wrapper; OVERWRITE replaces the body instead",
            )),
        ),
        "Accessor" => (
            Some("GET/SET"),
            Supported(s("GET / SET bridge (owned non-generic structs)")),
        ),
        "Invoker" => (Some("INVOKE"), Supported(s("INVOKE bridge"))),
        "Shadow" => (
            None,
            NotNeeded(s(
                "helpers get the target as a binding and reach its fields and methods directly",
            )),
        ),
        "Unique" if is_method => (None, Supported(s("a function in the mod's module"))),
        "Unique" => (Some("STATE"), Supported(s("namespaced per-instance state"))),
        other => (None, Partial(format!("@{other} is not mapped yet"))),
    }
}

/// An interface a Mixin adds to its target (`implements` on the Mixin class).
pub(crate) fn interface_member(interface: &str) -> MixinMember {
    MixinMember {
        member: interface.to_string(),
        annotation: "implements".into(),
        target_methods: Vec::new(),
        at: None,
        at_target: None,
        cancellable: false,
        horizon_kind: None,
        verdict: Verdict::Blocked(
            "horizon-mixins cannot add trait implementations to the game's types yet".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_injections() {
        assert_eq!(
            verdict("Inject", Some("HEAD"), false, true, true).0,
            Some("HEAD")
        );
        assert_eq!(
            verdict("Inject", Some("INVOKE"), true, false, true).0,
            Some("AFTER_CALL")
        );
        assert_eq!(
            verdict("Inject", Some("TAIL"), false, false, true).1.name(),
            "partial"
        );
        assert_eq!(
            verdict("Inject", Some("FIELD"), false, false, true)
                .1
                .name(),
            "blocked"
        );
        assert_eq!(
            verdict("Redirect", Some("INVOKE"), false, false, true)
                .1
                .name(),
            "partial"
        );
        assert_eq!(
            verdict("Shadow", None, false, false, false).1.name(),
            "not needed"
        );
        assert_eq!(
            verdict("Unique", None, false, false, false).0,
            Some("STATE")
        );
    }
}
