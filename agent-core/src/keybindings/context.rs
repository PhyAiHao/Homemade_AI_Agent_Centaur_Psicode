//! Binding context — context conditions for keybinding activation.

/// Current UI context for evaluating binding conditions.
#[derive(Debug, Clone, Default)]
pub struct BindingContext {
    pub input_focused: bool,
    pub suggestions_visible: bool,
    pub dialog_open: bool,
    pub vim_mode: bool,
    pub vim_normal: bool,
    pub vim_insert: bool,
    pub plan_mode: bool,
}

impl BindingContext {
    /// Evaluate a condition string against the current context.
    pub fn evaluate(&self, condition: &str) -> bool {
        match condition {
            "inputFocused" => self.input_focused,
            "suggestionsVisible" => self.suggestions_visible,
            "dialogOpen" => self.dialog_open,
            "vimMode" => self.vim_mode,
            "vimNormal" => self.vim_normal,
            "vimInsert" => self.vim_insert,
            "planMode" => self.plan_mode,
            // Negation: "!condition"
            c if c.starts_with('!') => !self.evaluate(&c[1..]),
            // Unknown conditions are treated as false
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate() {
        let ctx = BindingContext {
            input_focused: true,
            suggestions_visible: false,
            ..Default::default()
        };
        assert!(ctx.evaluate("inputFocused"));
        assert!(!ctx.evaluate("suggestionsVisible"));
        assert!(ctx.evaluate("!suggestionsVisible"));
        assert!(!ctx.evaluate("!inputFocused"));
    }
}
