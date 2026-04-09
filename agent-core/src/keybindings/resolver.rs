//! Binding resolver — resolves conflicts and finds the best matching binding.

use super::schema::Keybinding;
use super::context::BindingContext;

/// Resolve the best matching binding from a list, considering context and priority.
pub fn resolve_bindings<'a>(
    bindings: &'a [Keybinding],
    context: &BindingContext,
) -> Vec<&'a Keybinding> {
    let mut matches: Vec<&Keybinding> = bindings.iter()
        .filter(|b| context_matches(b, context))
        .collect();

    // Sort by priority (highest first), then by source specificity
    matches.sort_by(|a, b| {
        let source_priority = |s: &str| -> i32 {
            match s { "reserved" => 100, "user" => 50, "default" => 0, _ => 0 }
        };
        let pa = a.priority + source_priority(&a.source);
        let pb = b.priority + source_priority(&b.source);
        pb.cmp(&pa)
    });

    matches
}

fn context_matches(binding: &Keybinding, context: &BindingContext) -> bool {
    match &binding.when {
        None => true,
        Some(condition) => context.evaluate(condition),
    }
}
